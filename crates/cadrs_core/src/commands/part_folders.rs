//! Commands for the Parts list's folders: making one, moving parts in and out, opening,
//! closing, renaming and unpacking. A folder only groups the parts' rows: no rebuild follows.

use super::*;
use crate::document::PartFolder;

fn part_folders(
    doc: &mut Document,
    element: ElementId,
) -> Result<&mut Vec<PartFolder>, CommandError> {
    doc.element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?
        .part_folders_mut()
        .ok_or_else(|| CommandError::Invalid("part folders need a Part Studio".into()))
}

/// Puts parts into a new folder ("Folder N", or `name`), open, out of any other folder.
#[derive(Debug, Clone)]
pub struct CreatePartFolder {
    pub element: ElementId,
    pub folder: FeatureId,
    pub name: Option<String>,
    pub parts: Vec<PartId>,
}

impl Command for CreatePartFolder {
    fn label(&self) -> String {
        "Create part folder".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = self.name.as_deref().map(non_empty).transpose()?;
        let folders = part_folders(doc, self.element)?;
        if folders.iter().any(|f| f.id == self.folder) {
            return Err(CommandError::Invalid("folder id already in use".into()));
        }
        let name = name.unwrap_or_else(|| {
            (1..)
                .map(|n| format!("Folder {n}"))
                .find(|n| !folders.iter().any(|f| f.name == *n))
                .unwrap_or_default()
        });
        for f in folders.iter_mut() {
            f.parts.retain(|p| !self.parts.contains(p));
        }
        folders.push(PartFolder {
            id: self.folder,
            name,
            parts: self.parts.clone(),
            open: true,
        });
        Ok(())
    }
}

/// Moves parts into a folder, or (`folder: None`) out of every folder.
#[derive(Debug, Clone)]
pub struct MovePartsToFolder {
    pub element: ElementId,
    pub parts: Vec<PartId>,
    pub folder: Option<FeatureId>,
}

impl Command for MovePartsToFolder {
    fn label(&self) -> String {
        match self.folder {
            Some(_) => "Move parts to folder".into(),
            None => "Remove parts from folder".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let folders = part_folders(doc, self.element)?;
        if let Some(id) = self.folder
            && !folders.iter().any(|f| f.id == id)
        {
            return Err(CommandError::Invalid("folder not found".into()));
        }
        for f in folders.iter_mut() {
            f.parts.retain(|p| !self.parts.contains(p));
            if Some(f.id) == self.folder {
                f.parts.extend(self.parts.iter().copied());
            }
        }
        Ok(())
    }
}

/// Opens or closes a part folder, or renames it.
#[derive(Debug, Clone)]
pub struct SetPartFolder {
    pub element: ElementId,
    pub folder: FeatureId,
    pub open: Option<bool>,
    pub name: Option<String>,
}

impl Command for SetPartFolder {
    fn label(&self) -> String {
        match (&self.name, self.open) {
            (Some(n), _) => format!("Rename folder to {}", n.trim()),
            (None, Some(true)) => "Open folder".into(),
            _ => "Close folder".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = self.name.as_deref().map(non_empty).transpose()?;
        let f = part_folders(doc, self.element)?
            .iter_mut()
            .find(|f| f.id == self.folder)
            .ok_or_else(|| CommandError::Invalid("folder not found".into()))?;
        if let Some(o) = self.open {
            f.open = o;
        }
        if let Some(n) = name {
            f.name = n;
        }
        Ok(())
    }
}

/// Unpacks a part folder: its parts stay in the list, the folder goes.
#[derive(Debug, Clone)]
pub struct UnpackPartFolder {
    pub element: ElementId,
    pub folder: FeatureId,
}

impl Command for UnpackPartFolder {
    fn label(&self) -> String {
        "Unpack folder".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let folders = part_folders(doc, self.element)?;
        let n = folders.len();
        folders.retain(|f| f.id != self.folder);
        if folders.len() == n {
            return Err(CommandError::Invalid("folder not found".into()));
        }
        Ok(())
    }
}
