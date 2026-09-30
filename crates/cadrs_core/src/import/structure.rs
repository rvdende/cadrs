//! **Import as** (P3F.2; `essential-tips.md` T8, X5, `intro-to-parametric-cad.md` P3.4): a
//! STEP or IGES file brought in as new tabs, or as a new document, with the choice of how its
//! assembly structure comes in. The parts themselves are an ordinary Import feature
//! ([`super::ImportFeature`], the file stored as a blob) read with
//! [`super::ImportFeature::structure`] set.
//!
//! - **Reading.** The kernel reads the file with its assembly structure (the fork's XDE
//!   bindings, `cadrs_kernel::exchange`): each distinct part once, with its product name, and
//!   every use of it (an **occurrence**: the part, its placement, the instance's name).
//!   [`crate::rebuild::exchange::plan_import`] does this on the rebuild worker and returns an
//!   [`ImportPlan`]: the names, how many solids each part has, and the placements.
//! - **Import as** (T8.1): **Part Studio (flatten)** puts every occurrence where the file has
//!   it, as parts of one new Part Studio. **Keep assembly structure** puts each distinct part
//!   once, where its first occurrence is, in a new Part Studio, and adds an Assembly tab with
//!   an instance of it per occurrence, placed relative to that ([`ImportAs`]).
//! - **In the document** the file becomes the first feature of its Part Studio, "Import 1": it
//!   rebuilds through the kernel like any other feature (`rebuild::kernel_ops::import`), so the
//!   parts are ordinary parts (mass properties, mates, drawings, export).
//! - [`ImportFile`] adds the tabs as **one undo step**; [`imported_document`] makes a new
//!   document of them (the documents page's Create ▸ Import files…).
//!
//! Part ids are `PartId(import feature, k)`, `k` counting solids in the order the parts (or,
//! flattened, the occurrences) come, which is also the order the rebuild makes them in.

use crate::assembly::{Instance, InstanceId, InstanceSource, Pose};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, Feature, FeatureKind, PartProps};
use crate::ids::{ElementId, FeatureId, PartId};

use super::{ImportFeature, ImportFormat};

/// What an Import read with its structure makes ([`ImportFeature::structure`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ImportMode {
    /// Every occurrence where the file puts it (Import as Part Studio).
    Flatten,
    /// Each distinct part once, where its first occurrence is (Keep assembly structure: an
    /// assembly places them).
    Parts,
}

/// A part of the file: its name and how many solids it has.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanPart {
    pub name: String,
    pub solids: usize,
}

/// One use of a part: which, where (the file's coordinates, mm) and the instance's name.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanOccurrence {
    pub part: usize,
    pub pose: Pose,
    pub name: String,
}

/// What a file holds, read by the kernel ([`crate::rebuild::exchange::plan_import`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    pub format: ImportFormat,
    /// The top-level product's name.
    pub name: String,
    pub parts: Vec<PlanPart>,
    pub occurrences: Vec<PlanOccurrence>,
}

impl ImportPlan {
    /// An assembly: some part used more than once, or placed away from the origin.
    pub fn is_assembly(&self) -> bool {
        let moved = self.occurrences.iter().any(|o| o.pose != Pose::IDENTITY);
        let repeated = (0..self.parts.len()).any(|p| self.occurrences.iter().filter(|o| o.part == p).count() > 1);
        moved || repeated
    }

    /// How many parts the Part Studio gets in `mode`.
    pub fn part_count(&self, mode: ImportMode) -> usize {
        match mode {
            ImportMode::Parts => self.parts.iter().map(|p| p.solids.max(1)).sum(),
            ImportMode::Flatten => self.occurrences.iter().map(|o| self.parts.get(o.part).map_or(1, |p| p.solids.max(1))).sum(),
        }
    }

    /// The part names of the Part Studio in `mode`, in part id order. A part without a name
    /// is "Part n"; a part used more than once (flattened) is "<name> <k>"; a part of several
    /// solids gives "<name> (k)" for the second one on.
    pub fn part_names(&self, mode: ImportMode) -> Vec<String> {
        let base = |i: usize| -> String {
            let n = self.parts.get(i).map(|p| p.name.trim()).unwrap_or("");
            if n.is_empty() { format!("Part {}", i + 1) } else { n.to_string() }
        };
        let solids = |i: usize| self.parts.get(i).map_or(1, |p| p.solids.max(1));
        let mut out = Vec::new();
        let push = |name: String, n: usize, out: &mut Vec<String>| {
            for k in 0..n {
                out.push(if k == 0 { name.clone() } else { format!("{name} ({})", k + 1) });
            }
        };
        match mode {
            ImportMode::Parts => (0..self.parts.len()).for_each(|i| push(base(i), solids(i), &mut out)),
            ImportMode::Flatten => {
                let mut seen = vec![0usize; self.parts.len()];
                for o in &self.occurrences {
                    let uses = self.occurrences.iter().filter(|x| x.part == o.part).count();
                    let name = if uses > 1 {
                        seen[o.part] += 1;
                        format!("{} <{}>", base(o.part), seen[o.part])
                    } else {
                        base(o.part)
                    };
                    push(name, solids(o.part), &mut out);
                }
            }
        }
        out
    }

    /// The part ids of each distinct part in [`ImportMode::Parts`] (several for a part of
    /// several solids).
    pub fn parts_of(&self, feature: FeatureId) -> Vec<Vec<PartId>> {
        let mut k = 0u32;
        self.parts
            .iter()
            .map(|p| {
                let ids = (0..p.solids.max(1)).map(|j| PartId::new(feature, k + j as u32)).collect();
                k += p.solids.max(1) as u32;
                ids
            })
            .collect()
    }
}

/// How the file comes in (T8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportAs {
    /// One Part Studio with every occurrence (Import as Part Studio, flatten).
    PartStudio,
    /// A Part Studio of the distinct parts and an Assembly of their instances.
    Assembly,
}

/// The file name without its folder and extension.
pub fn stem(file_name: &str) -> String {
    let p = std::path::Path::new(file_name);
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Import".into())
}

/// `name`, or `name (2)`, … : a tab name not in `taken`.
fn unique_name(name: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == name) {
        return name.to_string();
    }
    (2..).map(|n| format!("{name} ({n})")).find(|c| !taken.iter().any(|t| t == c)).expect("a free name")
}

/// Ids for the tabs an import makes (fresh ones, or fixed ones for tests and scenarios).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImportIds {
    pub studio: ElementId,
    pub assembly: ElementId,
    pub feature: FeatureId,
    /// The instances get `instance_base + k`.
    pub instance_base: u128,
}

impl ImportIds {
    pub fn fresh() -> Self {
        Self { studio: ElementId::new(), assembly: ElementId::new(), feature: FeatureId::new(), instance_base: uuid::Uuid::new_v4().as_u128() & !0xffff }
    }
}

/// The tabs importing `plan` (the file `file_name`, contents `data`) adds to `doc`: a Part
/// Studio named after the file, and with [`ImportAs::Assembly`] an Assembly tab. The file's
/// bytes go into the blob cache ([`crate::blobs`]), and are saved with the document.
pub fn import_elements(doc: &Document, plan: &ImportPlan, file_name: &str, data: impl Into<Vec<u8>>, how: ImportAs, ids: ImportIds) -> Vec<Element> {
    let mode = match how {
        ImportAs::PartStudio => ImportMode::Flatten,
        ImportAs::Assembly => ImportMode::Parts,
    };
    let taken: Vec<String> = doc.elements.iter().map(|e| e.name.clone()).collect();
    let base = stem(file_name);
    let mut studio = Element::part_studio(unique_name(&base, &taken));
    studio.id = ids.studio;
    let feature = Feature {
        id: ids.feature,
        name: "Import 1".into(),
        kind: FeatureKind::Import(ImportFeature {
            blob: crate::blobs::insert(data.into()),
            file_name: file_name.to_string(),
            format: plan.format,
            y_axis_up: false,
            flatten: mode == ImportMode::Flatten,
            units: None,
            structure: Some(mode),
        }),
    };
    if let Some(features) = studio.features_mut() {
        features.push(feature);
    }
    if let Some(props) = studio.part_props_mut() {
        for (k, name) in plan.part_names(mode).into_iter().enumerate() {
            let mut p = PartProps::new(PartId::new(ids.feature, k as u32));
            p.name = Some(name);
            props.push(p);
        }
    }
    let mut out = vec![studio.clone()];
    if how == ImportAs::Assembly {
        let asm_name = if plan.name.trim().is_empty() || plan.name.trim() == base { format!("{base} Assembly") } else { plan.name.trim().to_string() };
        let mut taken = taken;
        taken.push(studio.name.clone());
        let mut asm = Element::assembly(unique_name(&asm_name, &taken));
        asm.id = ids.assembly;
        let ids_of = plan.parts_of(ids.feature);
        let model = asm.assembly_model_mut().expect("an assembly");
        let mut n = 0u128;
        for o in &plan.occurrences {
            // The part sits in its Part Studio where its first occurrence is: the instance is
            // placed relative to that.
            let first = plan.occurrences.iter().find(|x| x.part == o.part).map_or(Pose::IDENTITY, |x| x.pose);
            let pose = first.inverse().then(&o.pose);
            for part in ids_of.get(o.part).cloned().unwrap_or_default() {
                let source = InstanceSource::Part { element: ids.studio, part };
                let mut inst = Instance::new(InstanceId::from_u128(ids.instance_base + n), source, pose);
                n += 1;
                inst.index = model.next_index(&source);
                // As an insert: the first instance is fixed where the file puts it.
                inst.fixed = model.instances.is_empty();
                model.instances.push(inst);
            }
        }
        out.push(asm);
    }
    out
}

/// **Import…** into a document: adds the tabs [`import_elements`] made, as one undo step.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportFile {
    pub file_name: String,
    pub elements: Vec<Element>,
}

impl Command for ImportFile {
    fn label(&self) -> String {
        format!("Import {}", self.file_name)
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.elements.is_empty() {
            return Err(CommandError::Invalid("nothing to import".into()));
        }
        for e in &self.elements {
            if doc.element(e.id).is_some() {
                return Err(CommandError::Invalid("tab id already in use".into()));
            }
            doc.elements.push(e.clone());
        }
        Ok(())
    }
}

/// The turn a Y-up file is read with: +90° about X, so the file's +Y is the model's +Z.
pub fn y_up_turn() -> Pose {
    Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], std::f64::consts::FRAC_PI_2)
}

/// The import option **File is Y axis up** (P3F.2 judge) applied to the tabs
/// [`import_elements`] made: the parts are turned so the file's +Y is up (+Z), and the
/// assembly's instances with them.
pub fn file_is_y_up(elements: &mut [Element]) {
    let f = y_up_turn();
    for e in elements.iter_mut() {
        if let Some(features) = e.features_mut() {
            for x in features.iter_mut() {
                if let FeatureKind::Import(i) = &mut x.kind {
                    i.y_axis_up = true;
                }
            }
        }
        if let Some(model) = e.assembly_model_mut() {
            for inst in &mut model.instances {
                inst.pose = f.inverse().then(&inst.pose).then(&f);
            }
        }
    }
}

/// A new document holding an imported file (the documents page's Create ▸ Import files…),
/// named after the file.
pub fn imported_document(plan: &ImportPlan, file_name: &str, data: impl Into<Vec<u8>>, how: ImportAs, ids: ImportIds) -> Document {
    let mut doc = Document::empty(stem(file_name));
    let elements = import_elements(&doc, plan, file_name, data, how, ids);
    doc.elements.extend(elements);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> ImportPlan {
        let moved = Pose::translation([100.0, 0.0, 0.0]);
        ImportPlan {
            format: ImportFormat::Step,
            name: "Bracket set".into(),
            parts: vec![PlanPart { name: "Bracket".into(), solids: 1 }, PlanPart { name: "".into(), solids: 2 }],
            occurrences: vec![
                PlanOccurrence { part: 0, pose: Pose::IDENTITY, name: "Bracket <1>".into() },
                PlanOccurrence { part: 0, pose: moved, name: "Bracket <2>".into() },
                PlanOccurrence { part: 1, pose: Pose::IDENTITY, name: "".into() },
            ],
        }
    }

    #[test]
    fn names_and_counts_by_mode() {
        let p = plan();
        assert!(p.is_assembly());
        assert_eq!(p.part_count(ImportMode::Parts), 3);
        assert_eq!(p.part_count(ImportMode::Flatten), 4);
        assert_eq!(p.part_names(ImportMode::Parts), ["Bracket", "Part 2", "Part 2 (2)"]);
        assert_eq!(p.part_names(ImportMode::Flatten), ["Bracket <1>", "Bracket <2>", "Part 2", "Part 2 (2)"]);
    }

    #[test]
    fn keep_structure_adds_a_studio_and_an_assembly_as_one_step() {
        let p = plan();
        let mut doc = Document::new("Doc");
        let ids = ImportIds { studio: ElementId::from_u128(1), assembly: ElementId::from_u128(2), feature: FeatureId::from_u128(3), instance_base: 0x100 };
        let els = import_elements(&doc, &p, "/tmp/Brackets.step", "ISO-10303-21;", ImportAs::Assembly, ids);
        assert_eq!(els.len(), 2);
        assert_eq!(els[0].name, "Brackets");
        assert_eq!(els[1].name, "Bracket set");
        let asm = els[1].assembly_model().unwrap();
        // 2 instances of the bracket and one of each solid of the second part.
        assert_eq!(asm.instances.len(), 4);
        assert_eq!(asm.instances[1].pose.translation, [100.0, 0.0, 0.0]);
        assert_eq!(asm.instances[1].source, InstanceSource::Part { element: ids.studio, part: PartId::new(ids.feature, 0) });
        assert_eq!(asm.instances[3].source, InstanceSource::Part { element: ids.studio, part: PartId::new(ids.feature, 2) });
        assert!(asm.instances[0].fixed && !asm.instances[1].fixed);
        let mut history = crate::History::default();
        let before = doc.clone();
        history.execute(&mut doc, &ImportFile { file_name: "Brackets.step".into(), elements: els }).unwrap();
        assert_eq!(doc.elements.len(), before.elements.len() + 2);
        history.undo(&mut doc);
        assert_eq!(doc, before);
    }

    #[test]
    fn flatten_makes_one_studio_named_after_the_file() {
        let p = plan();
        let doc = imported_document(&p, "Brackets.STP", "x", ImportAs::PartStudio, ImportIds::fresh());
        assert_eq!(doc.name, "Brackets");
        assert_eq!(doc.elements.len(), 1);
        assert_eq!(doc.elements[0].part_props().len(), 4);
        let FeatureKind::Import(f) = &doc.elements[0].features()[0].kind else { panic!() };
        assert_eq!(f.structure, Some(ImportMode::Flatten));
        assert_eq!(crate::blobs::get(&f.blob).as_deref().map(|b| &b[..]), Some(&b"x"[..]));
    }
}
