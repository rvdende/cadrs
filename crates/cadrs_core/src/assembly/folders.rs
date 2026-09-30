//! Folders in the Instances list and the Mate Features list (P3B.4, `intro-to-assemblies.md`
//! A18; `lesson-assembly-folders.png`, `ex3-step3.png`).
//!
//! The folder model is the Part Studio feature list's (P3.6, P3.9): a [`FeatureFolder`] holds a
//! run of the list's items, kept contiguous and in list order by
//! [`crate::commands::normalize_folders`]; it is shown at its first item. An assembly's items are
//! instances and mate features, named in a folder by their uuid as a [`FeatureId`] ([`item`],
//! [`mate_item`]). An **empty** folder (New folder with nothing selected) is listed at the end.
//!
//! Commands: [`CreateAssemblyFolder`] (New folder, Add selection to folder…: the selection is
//! gathered at its first item, A18.3), [`MoveListItems`] (drag rows or a folder: reorder, into or
//! out of a folder, A18.2), [`SetAssemblyFolder`] (open / close, Rename),
//! [`UnpackAssemblyFolder`] and [`DeleteAssemblyFolder`] (with its contents, A18.4). A folder's
//! eye, Hide / Show and Suppress act on its contents with the instance and mate commands.

use super::commands::{DeleteInstances, DeleteMateFeatures, assembly_mut};
use super::mate::MateId;
use super::{Assembly, InstanceId};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, FeatureFolder};
use crate::ids::{ElementId, FeatureId};

/// Which list a folder is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FolderList {
    Instances,
    Mates,
}

/// An instance as a folder item.
pub fn item(i: InstanceId) -> FeatureId {
    FeatureId(i.0)
}

/// A mate feature as a folder item.
pub fn mate_item(m: MateId) -> FeatureId {
    FeatureId(m.0)
}

/// The items of a list, in order.
pub fn order(asm: &Assembly, list: FolderList) -> Vec<FeatureId> {
    match list {
        FolderList::Instances => asm.instances.iter().map(|i| item(i.id)).collect(),
        FolderList::Mates => asm.mates.iter().map(|m| mate_item(m.id)).collect(),
    }
}

pub fn folders(asm: &Assembly, list: FolderList) -> &[FeatureFolder] {
    match list {
        FolderList::Instances => &asm.folders,
        FolderList::Mates => &asm.mate_folders,
    }
}

fn folders_mut(asm: &mut Assembly, list: FolderList) -> &mut Vec<FeatureFolder> {
    match list {
        FolderList::Instances => &mut asm.folders,
        FolderList::Mates => &mut asm.mate_folders,
    }
}

/// The folder an item is in.
pub fn folder_of(asm: &Assembly, list: FolderList, it: FeatureId) -> Option<&FeatureFolder> {
    folders(asm, list).iter().find(|f| f.features.contains(&it))
}

/// Keeps both lists' folders in list order and contiguous (after items left or moved).
pub fn tidy(asm: &mut Assembly) {
    for list in [FolderList::Instances, FolderList::Mates] {
        let o = order(asm, list);
        crate::commands::normalize_folders(folders_mut(asm, list), &o);
    }
}

/// Moves `items` (in list order) so the first lands at `to`, an index in the list without them.
fn reorder(asm: &mut Assembly, list: FolderList, items: &[FeatureId], to: usize) -> Result<(), CommandError> {
    fn go<T>(v: &mut Vec<T>, key: impl Fn(&T) -> FeatureId, items: &[FeatureId], to: usize) -> Result<(), CommandError> {
        let mut moved = Vec::new();
        let mut k = 0;
        while k < v.len() {
            if items.contains(&key(&v[k])) {
                moved.push(v.remove(k));
            } else {
                k += 1;
            }
        }
        if moved.len() != items.len() {
            return Err(CommandError::Invalid("item not found".into()));
        }
        if to > v.len() {
            return Err(CommandError::Invalid("no such place in the list".into()));
        }
        for (j, m) in moved.into_iter().enumerate() {
            v.insert(to + j, m);
        }
        Ok(())
    }
    match list {
        FolderList::Instances => go(&mut asm.instances, |i| item(i.id), items, to),
        FolderList::Mates => go(&mut asm.mates, |m| mate_item(m.id), items, to),
    }
}

fn non_empty(name: &str) -> Result<String, CommandError> {
    let n = name.trim();
    if n.is_empty() { Err(CommandError::Invalid("a folder needs a name".into())) } else { Ok(n.to_string()) }
}

/// A new folder (the list header's New folder, Add selection to folder…, A18.2, A18.3): named
/// `name` (else "Folder n"), holding `items`, which gather at the first of them. With no items
/// it is empty, at the end of the list.
#[derive(Debug, Clone)]
pub struct CreateAssemblyFolder {
    pub element: ElementId,
    pub list: FolderList,
    pub folder: FeatureId,
    pub name: Option<String>,
    pub items: Vec<FeatureId>,
}

impl Command for CreateAssemblyFolder {
    fn label(&self) -> String {
        "Create folder".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        let list = order(asm, self.list);
        if self.items.iter().any(|i| !list.contains(i)) {
            return Err(CommandError::Invalid("item not found".into()));
        }
        let all = [folders(asm, FolderList::Instances), folders(asm, FolderList::Mates)].concat();
        if all.iter().any(|f| f.id == self.folder) {
            return Err(CommandError::Invalid("folder id already in use".into()));
        }
        let name = match &self.name {
            Some(n) => non_empty(n)?,
            None => (1..).map(|n| format!("Folder {n}")).find(|n| !all.iter().any(|f| &f.name == n)).unwrap_or_default(),
        };
        // In list order, gathered at the first (A18.3).
        let mut items: Vec<FeatureId> = list.iter().copied().filter(|i| self.items.contains(i)).collect();
        items.dedup();
        if let Some(first) = items.first() {
            let at = list.iter().position(|i| i == first).unwrap_or(0);
            let before = list[..at].iter().filter(|i| items.contains(i)).count();
            reorder(asm, self.list, &items, at - before)?;
        }
        let fs = folders_mut(asm, self.list);
        for f in fs.iter_mut() {
            f.features.retain(|x| !items.contains(x));
        }
        fs.push(FeatureFolder { id: self.folder, name, features: items, open: true });
        tidy(asm);
        Ok(())
    }
}

/// Moves rows (instances or mates, or a folder's contents) so the first lands at `to` (an index
/// in the list without them), into `folder` or out of every folder (drag in the list, A18.2).
#[derive(Debug, Clone)]
pub struct MoveListItems {
    pub element: ElementId,
    pub list: FolderList,
    pub items: Vec<FeatureId>,
    pub to: usize,
    pub folder: Option<FeatureId>,
    pub label: String,
}

impl Command for MoveListItems {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        let list = order(asm, self.list);
        let items: Vec<FeatureId> = list.iter().copied().filter(|i| self.items.contains(i)).collect();
        if items.len() != self.items.len() || items.is_empty() {
            return Err(CommandError::Invalid("item not found".into()));
        }
        reorder(asm, self.list, &items, self.to)?;
        let fs = folders_mut(asm, self.list);
        // A whole folder being moved keeps its items; otherwise they leave their folders.
        let whole: Vec<FeatureId> = fs
            .iter()
            .filter(|f| !f.features.is_empty() && f.features.iter().all(|x| items.contains(x)))
            .map(|f| f.id)
            .collect();
        for f in fs.iter_mut() {
            let keeps = Some(f.id) == self.folder || (self.folder.is_none() && whole.contains(&f.id));
            if !keeps {
                f.features.retain(|x| !items.contains(x));
            }
        }
        if let Some(target) = self.folder {
            let f = fs.iter_mut().find(|f| f.id == target).ok_or_else(|| CommandError::Invalid("folder not found".into()))?;
            for x in &items {
                if !f.features.contains(x) {
                    f.features.push(*x);
                }
            }
        }
        tidy(asm);
        Ok(())
    }
}

/// Opens, closes or renames a folder (A18.4 Rename).
#[derive(Debug, Clone)]
pub struct SetAssemblyFolder {
    pub element: ElementId,
    pub list: FolderList,
    pub folder: FeatureId,
    pub open: Option<bool>,
    pub name: Option<String>,
}

impl Command for SetAssemblyFolder {
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
        let asm = assembly_mut(doc, self.element)?;
        let f = folders_mut(asm, self.list)
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

/// **Unpack folder** (A18.4): the folder goes, its items stay where they are.
#[derive(Debug, Clone)]
pub struct UnpackAssemblyFolder {
    pub element: ElementId,
    pub list: FolderList,
    pub folder: FeatureId,
}

impl Command for UnpackAssemblyFolder {
    fn label(&self) -> String {
        "Unpack folder".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let fs = folders_mut(assembly_mut(doc, self.element)?, self.list);
        let n = fs.len();
        fs.retain(|f| f.id != self.folder);
        if fs.len() == n {
            return Err(CommandError::Invalid("folder not found".into()));
        }
        Ok(())
    }
}

/// **Delete** a folder (A18.4): the folder *and its contents* (instances with their mates, or
/// mates).
#[derive(Debug, Clone)]
pub struct DeleteAssemblyFolder {
    pub element: ElementId,
    pub list: FolderList,
    pub folder: FeatureId,
}

impl Command for DeleteAssemblyFolder {
    fn label(&self) -> String {
        "Delete folder".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        let inside = folders(asm, self.list)
            .iter()
            .find(|f| f.id == self.folder)
            .map(|f| f.features.clone())
            .ok_or_else(|| CommandError::Invalid("folder not found".into()))?;
        folders_mut(asm, self.list).retain(|f| f.id != self.folder);
        if !inside.is_empty() {
            match self.list {
                FolderList::Instances => {
                    let instances = inside.iter().map(|f| InstanceId(f.0)).collect();
                    DeleteInstances { element: self.element, instances }.apply(doc)?;
                }
                FolderList::Mates => {
                    let mates = inside.iter().map(|f| MateId(f.0)).collect();
                    DeleteMateFeatures { element: self.element, mates }.apply(doc)?;
                }
            }
        }
        Ok(())
    }
}
