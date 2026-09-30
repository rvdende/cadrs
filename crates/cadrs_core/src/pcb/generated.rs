//! **Create assembly** (P3H.6, PCB7, X7): what a PCB Studio remembers about the Part Studios and
//! Assemblies it generated from a board ([`GeneratedAssembly`]), and the command that adds them
//! to the document in one undo step ([`CreatePcbAssembly`]; the tabs themselves are built by
//! `cadrs_pcb::create_assembly`).
//!
//! Each component instance of a generated assembly is **tied to its placement** by designator
//! ([`GeneratedAssembly::components`]), with the package frame in its part's coordinates, so a
//! later Sync (PCB5.5, PCB9.6) reads the instance back as that placement without relying on part
//! names. The assembly generated from a board is also that board's sync target: syncing it
//! updates the board in place rather than adding another one.

use serde::{Deserialize, Serialize};

use super::{BoardId, studio_mut};
use crate::assembly::{InstanceId, Pose};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element};
use crate::ids::ElementId;

/// A component instance of a generated assembly: its designator and its package frame (the
/// IDF package's coordinates) in its part's own coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinkedComponent {
    pub instance: InstanceId,
    pub refdes: String,
    #[serde(default)]
    pub frame: Pose,
}

/// A package's part in the components Part Studio (so a later Create assembly reuses it rather
/// than making it again): the package and part number, the part, its package frame and the x
/// range it takes in the studio (the packages are laid side by side along x).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneratedPackage {
    pub package: String,
    #[serde(default)]
    pub part_number: String,
    pub part: crate::ids::PartId,
    #[serde(default)]
    pub frame: Pose,
    #[serde(default)]
    pub x_range: [f64; 2],
}

/// The tabs Create assembly made from a board.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneratedAssembly {
    pub board: BoardId,
    /// The Part Studio with the board (and keep parts), named after the board.
    pub studio: ElementId,
    /// The Part Studio with the component parts (while components are in-document parts; P3H.7
    /// moves them to component documents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components_studio: Option<ElementId>,
    /// The Assembly, named after the board.
    pub assembly: ElementId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<LinkedComponent>,
    /// The packages' parts in `components_studio` (the ones reused from an earlier Create
    /// included).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<GeneratedPackage>,
}

impl GeneratedAssembly {
    /// The component instance `instance` of the assembly stands for.
    pub fn component(&self, instance: InstanceId) -> Option<&LinkedComponent> {
        self.components.iter().find(|c| c.instance == instance)
    }
}

impl super::PcbStudio {
    /// The generation that made the tab `element` (its board studio or its assembly), newest
    /// first.
    pub fn generated_from(&self, element: ElementId) -> Option<&GeneratedAssembly> {
        self.generated.iter().rev().find(|g| g.assembly == element || g.studio == element)
    }
}

/// Create assembly's OK, once the tabs are built: adds them right of the PCB Studio tab (in
/// order) and records what was generated, as **one undo step** (it adds and fills several
/// tabs, so its scope is the whole document).
#[derive(Debug, Clone)]
pub struct CreatePcbAssembly {
    /// The PCB Studio tab.
    pub element: ElementId,
    pub board_name: String,
    /// The new tabs, in tab order.
    pub elements: Vec<Element>,
    /// Tabs already in the document that it changes: the components Part Studio of an earlier
    /// Create, reused, with the parts of the packages it didn't have yet.
    pub replaced: Vec<Element>,
    pub generated: GeneratedAssembly,
}

impl Command for CreatePcbAssembly {
    fn label(&self) -> String {
        format!("Create assembly from {}", self.board_name)
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.elements.iter().any(|e| doc.element(e.id).is_some()) {
            return Err(CommandError::Invalid("the generated tabs are already in the document".into()));
        }
        let s = studio_mut(doc, self.element)?;
        if s.board(self.generated.board).is_none() {
            return Err(CommandError::Invalid("the board is gone".into()));
        }
        s.generated.push(self.generated.clone());
        for e in &self.replaced {
            let Some(old) = doc.elements.iter_mut().find(|x| x.id == e.id) else {
                return Err(CommandError::Invalid("the reused components Part Studio is gone".into()));
            };
            *old = e.clone();
        }
        let at = doc.element_index(self.element).map_or(doc.elements.len(), |i| i + 1);
        for (k, e) in self.elements.iter().enumerate() {
            doc.elements.insert(at + k, e.clone());
        }
        Ok(())
    }
}
