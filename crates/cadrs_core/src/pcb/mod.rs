//! The PCB Studio element (stage 3H, P3H.3–P3H.4; PCB1–PCB4, PCB11, X1, X6, X10): a tab that
//! holds **many boards** ([`StudioBoard`], each an IDF board converted to mm with stable item
//! ids, [`board::PcbBoard`]), which one is active, and its copies of the workspace settings
//! ([`PcbSettings`]: the component library document and the folder for component documents) and
//! of the component library's mappings ([`library::ComponentLibrary`]).
//!
//! Every edit goes through a command (and so the undo history): [`ImportBoard`] (Import ECAD
//! files, PCB4.3–4.4), [`DeleteBoard`] (right-click a board → Delete this board, PCB3.6),
//! [`SetPcbSettings`] (the settings dialog's Update, PCB2.2), [`SetRefdes`] (double-click a
//! designator in the BOM, PCB3.10) and [`SetRepresentation`] (the Component pane, PCB11.5–11.6).
//! Clicking a board to show it (PCB3.4) is view state kept by the app, not an edit: `active` is
//! the board the last import or delete showed. The element serialises with the document
//! (`ElementKind::PcbStudio`).
//!
//! [`import`] pairs the files picked in Import ECAD files (`.emn` with its `.emp`) and parses
//! them with `cadrs_idf`; [`bom`] groups the BOM, [`search`] finds boards and components, and
//! [`library`] holds the mappings and the workspace settings and keeps the copies in step.

pub mod board;
pub mod bom;
pub mod generated;
pub mod import;
pub mod library;
pub mod search;
pub mod sync;

use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind};
use crate::ids::{DocumentId, ElementId, FolderId};

pub use board::{ItemId, KeepArea, KeepIds, KeepKind, PcbBoard};
pub use generated::{CreatePcbAssembly, GeneratedAssembly, GeneratedPackage, LinkedComponent};
pub use sync::{McadSource, SyncBoard, SyncPlaneChoice};
pub use library::{ComponentLibrary, CustomPart, DEFAULT_LIBRARY_FILE, PartSource, PartTransform, PcbWorkspace, Representation};

/// A board's id within its PCB Studio, stable while the board exists (the tree, the view cache
/// and the active board refer to boards by it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BoardId(pub u64);

/// Where a board came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BoardSource {
    /// Imported from IDF files (their file names; the `.emp` may be missing).
    Idf { emn: String, emp: Option<String> },
    /// Synced from a Part Studio or Assembly tab (P3H.5, PCB5): see [`sync`].
    Mcad(sync::McadSource),
}

/// One board of a PCB Studio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudioBoard {
    pub id: BoardId,
    pub board: PcbBoard,
    pub source: BoardSource,
}

impl StudioBoard {
    pub fn name(&self) -> &str {
        self.board.name()
    }
}

/// The component library document (X6, PCB2.1): a cadrs document in the store whose PCB
/// Studio tab holds the mappings. The name is kept for display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryRef {
    pub document: DocumentId,
    pub name: String,
}

/// The folder for new component documents (PCB2.1): a folder of the documents page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderRef {
    pub id: FolderId,
    pub name: String,
}

/// The PCB settings (X6, PCB2.1–2.2), workspace-level: stored once per document store
/// ([`library::PcbWorkspace`]) and copied into every PCB Studio ([`library::LibrarySync`]).
/// `None` means not chosen. (P3H.3 kept plain paths under the names `library` and
/// `component_folder`; the new names make old files read those as unknown fields, i.e. unset.)
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcbSettings {
    /// The component library document. `None`: the default library file in the documents
    /// folder ([`DEFAULT_LIBRARY_FILE`]).
    #[serde(default, rename = "library_document", skip_serializing_if = "Option::is_none")]
    pub library: Option<LibraryRef>,
    /// The folder new component documents are created in (Create assembly, PCB7). `None`: not
    /// chosen yet ("Select a folder…").
    #[serde(default, rename = "component_folder_ref", skip_serializing_if = "Option::is_none")]
    pub component_folder: Option<FolderRef>,
}

impl PcbSettings {
    /// The library's short name for the settings field: the document's name, or "Default
    /// library".
    pub fn library_name(&self) -> String {
        self.library.as_ref().map(|l| l.name.clone()).unwrap_or_else(|| "Default library".into())
    }
}

/// The build string shown at the bottom of the settings dialog and written into exported IDF
/// headers.
pub const BUILD_STRING: &str = "cadrs PCB Studio v0.1";

/// A PCB Studio tab's contents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PcbStudio {
    /// The boards, in the order they were added (the Boards list).
    #[serde(default)]
    pub boards: Vec<StudioBoard>,
    /// The board shown (bold and blue in the Boards list) after the last import or delete;
    /// `None` when there are no boards. Clicking another board is view state (the app's).
    #[serde(default)]
    pub active: Option<BoardId>,
    /// This studio's copy of the workspace settings.
    #[serde(default)]
    pub settings: PcbSettings,
    /// This studio's copy of the component library's mappings (the representations shown).
    #[serde(default)]
    pub library: ComponentLibrary,
    /// The next [`BoardId`] to give out (never reused, so a deleted board's id stays free).
    #[serde(default)]
    next_board: u64,
    /// What Create assembly made from its boards (P3H.6, [`generated`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated: Vec<GeneratedAssembly>,
}

impl PcbStudio {
    pub fn board(&self, id: BoardId) -> Option<&StudioBoard> {
        self.boards.iter().find(|b| b.id == id)
    }

    pub fn board_mut(&mut self, id: BoardId) -> Option<&mut StudioBoard> {
        self.boards.iter_mut().find(|b| b.id == id)
    }

    /// The board shown.
    pub fn active_board(&self) -> Option<&StudioBoard> {
        self.active.and_then(|id| self.board(id))
    }

    /// Adds a board at the end and returns its id.
    pub fn add_board(&mut self, board: PcbBoard, source: BoardSource) -> BoardId {
        let n = self.next_board.max(self.boards.iter().map(|b| b.id.0 + 1).max().unwrap_or(0));
        let id = BoardId(n);
        self.next_board = n + 1;
        self.boards.push(StudioBoard { id, board, source });
        id
    }

    /// Removes a board. If it was active, its neighbour (the next board, else the previous one)
    /// becomes active.
    pub fn remove_board(&mut self, id: BoardId) -> Option<StudioBoard> {
        let i = self.boards.iter().position(|b| b.id == id)?;
        let b = self.boards.remove(i);
        if self.active == Some(id) {
            self.active = self.boards.get(i).or_else(|| self.boards.get(i.wrapping_sub(1))).map(|b| b.id);
        }
        Some(b)
    }

    /// A board name not yet used in this studio: `name`, else "name (1)", "name (2)", ...
    pub fn free_name(&self, name: &str) -> String {
        if !self.boards.iter().any(|b| b.name() == name) {
            return name.to_string();
        }
        (1..).map(|n| format!("{name} ({n})")).find(|c| !self.boards.iter().any(|b| b.name() == c)).unwrap()
    }
}

impl Element {
    /// A PCB Studio tab (P3H.3) with no boards.
    pub fn pcb_studio(name: impl Into<String>) -> Self {
        Self {
            id: ElementId::new(),
            name: name.into(),
            kind: ElementKind::PcbStudio(Box::default()),
            assembly: Default::default(),
            context: None,
            simulation: Default::default(),
        }
    }

    /// A PCB Studio tab's contents.
    pub fn pcb(&self) -> Option<&PcbStudio> {
        match &self.kind {
            ElementKind::PcbStudio(p) => Some(p),
            _ => None,
        }
    }

    pub fn pcb_mut(&mut self) -> Option<&mut PcbStudio> {
        match &mut self.kind {
            ElementKind::PcbStudio(p) => Some(p),
            _ => None,
        }
    }
}

fn studio_mut(doc: &mut Document, element: ElementId) -> Result<&mut PcbStudio, CommandError> {
    doc.element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?
        .pcb_mut()
        .ok_or_else(|| CommandError::Invalid("not a PCB Studio".into()))
}

/// Import ECAD files (PCB4.4): adds a board to the studio and shows it. A board whose name is
/// already in the studio gets " (1)", " (2)", ... so the Boards list can tell them apart.
#[derive(Debug, Clone)]
pub struct ImportBoard {
    pub element: ElementId,
    pub board: Box<PcbBoard>,
    pub source: BoardSource,
}

impl Command for ImportBoard {
    fn label(&self) -> String {
        format!("Import {}", self.board.name())
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let s = studio_mut(doc, self.element)?;
        let mut board = (*self.board).clone();
        board.board.name = s.free_name(board.name());
        let id = s.add_board(board, self.source.clone());
        s.active = Some(id);
        Ok(())
    }
}

/// Right-click a board → Delete this board (PCB3.6). Tabs made from it earlier (Create
/// assembly) are not touched.
#[derive(Debug, Clone)]
pub struct DeleteBoard {
    pub element: ElementId,
    pub board: BoardId,
}

impl Command for DeleteBoard {
    fn label(&self) -> String {
        "Delete board".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let s = studio_mut(doc, self.element)?;
        s.remove_board(self.board).map(|_| ()).ok_or_else(|| CommandError::Invalid("no such board".into()))
    }
}

/// The settings dialog's Update (PCB2.2, X6).
#[derive(Debug, Clone)]
pub struct SetPcbSettings {
    pub element: ElementId,
    pub settings: PcbSettings,
}

impl Command for SetPcbSettings {
    fn label(&self) -> String {
        "Update PCB Studio settings".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let s = studio_mut(doc, self.element)?;
        s.settings = self.settings.clone();
        Ok(())
    }
}

/// Double-click a designator in the BOM and type a new one (PCB3.10). The designator must not
/// be empty or contain spaces, and no other component of the board may have it (compared
/// without regard to case).
#[derive(Debug, Clone)]
pub struct SetRefdes {
    pub element: ElementId,
    pub board: BoardId,
    pub item: ItemId,
    pub refdes: String,
}

impl Command for SetRefdes {
    fn label(&self) -> String {
        format!("Rename {}", self.refdes.trim())
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let new = self.refdes.trim();
        if new.is_empty() {
            return Err(CommandError::Invalid("A designator can't be empty".into()));
        }
        if new.contains(char::is_whitespace) {
            return Err(CommandError::Invalid("A designator can't contain spaces".into()));
        }
        let s = studio_mut(doc, self.element)?;
        let b = &mut s.board_mut(self.board).ok_or_else(|| CommandError::Invalid("no such board".into()))?.board;
        let i = b.component_ids.iter().position(|c| *c == self.item).ok_or_else(|| CommandError::Invalid("no such component".into()))?;
        if b.board.placements.iter().enumerate().any(|(j, p)| j != i && p.refdes.eq_ignore_ascii_case(new)) {
            return Err(CommandError::Invalid(format!("{new} is already used on this board")));
        }
        b.board.placements[i].refdes = new.to_string();
        Ok(())
    }
}

/// The Component pane's Representation (PCB11.5–11.6): how a package is shown, in the
/// component library (so every PCB Studio using the library follows, X6).
#[derive(Debug, Clone)]
pub struct SetRepresentation {
    pub element: ElementId,
    pub package: String,
    pub representation: Representation,
    /// The undo label; empty: "Set representation of <package>".
    pub label: String,
}

impl Command for SetRepresentation {
    fn label(&self) -> String {
        if self.label.is_empty() { format!("Set representation of {}", self.package) } else { self.label.clone() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let s = studio_mut(doc, self.element)?;
        s.library.set(&self.package, self.representation.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests;
