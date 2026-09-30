//! Tab folders (P3E.2; `test-drive.md` TD5.3, TD5.4, X6; `essential-tips.md` T1.2, X1).
//!
//! A document's tabs can be organized into **folders**, which can hold tabs and other folders.
//! The tree is stored in [`Document::tab_tree`] as an order over element ids and folder ids; the
//! elements themselves stay in [`Document::elements`], untouched (their ids, contents and every
//! reference between tabs are unaffected by moving a tab into a folder or reordering it).
//!
//! **Normal form.** The stored tree is read through [`layout`], which is robust to every other
//! edit of the element list:
//! - ids of elements that no longer exist (deleted, moved to another document) are skipped;
//! - an element the tree doesn't mention (a new tab, a duplicate, an undone delete) is placed
//!   right after the element before it in [`Document::elements`], in that element's folder (so a
//!   tab made next to the active tab inside a folder lands in that folder), or first at the top
//!   when there is none;
//! - a folder no other folder or the top level mentions goes at the end of the top level.
//!
//! Every command here writes the tree back with [`store`]: the tree in normal form, and
//! [`Document::elements`] reordered to the tree's depth-first order, so lists elsewhere read the
//! tabs in the same order as the tab bar. A document without folders keeps an **empty** tree
//! (its order is just the element order), so older files load and save unchanged.

use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;

/// The stored tab tree. Empty for a document without folders.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TabTree {
    /// The top level, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub root: Vec<TabItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<TabFolder>,
}

impl TabTree {
    pub fn is_empty(&self) -> bool {
        self.root.is_empty() && self.folders.is_empty()
    }

    pub fn folder(&self, id: ElementId) -> Option<&TabFolder> {
        self.folders.iter().find(|f| f.id == id)
    }
}

/// A tab folder: its id (a pseudo-element id: it names no element), its name and its contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabFolder {
    pub id: ElementId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<TabItem>,
}

/// One entry of a level: a tab or a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TabItem {
    Tab(ElementId),
    Folder(ElementId),
}

impl TabItem {
    pub fn id(self) -> ElementId {
        match self {
            TabItem::Tab(e) | TabItem::Folder(e) => e,
        }
    }
}

/// The resolved tree: what the tab bar and the Tab manager show.
#[derive(Debug, Clone, PartialEq)]
pub enum TabNode {
    Tab(ElementId),
    Folder { id: ElementId, name: String, children: Vec<TabNode> },
}

impl TabNode {
    pub fn item(&self) -> TabItem {
        match self {
            TabNode::Tab(e) => TabItem::Tab(*e),
            TabNode::Folder { id, .. } => TabItem::Folder(*id),
        }
    }

    /// The tabs in it, depth first.
    pub fn tabs(&self) -> Vec<ElementId> {
        let mut out = Vec::new();
        flatten_into(std::slice::from_ref(self), &mut out);
        out
    }
}

fn flatten_into(nodes: &[TabNode], out: &mut Vec<ElementId>) {
    for n in nodes {
        match n {
            TabNode::Tab(e) => out.push(*e),
            TabNode::Folder { children, .. } => flatten_into(children, out),
        }
    }
}

/// Every tab of `nodes`, depth first.
pub fn flatten(nodes: &[TabNode]) -> Vec<ElementId> {
    let mut out = Vec::new();
    flatten_into(nodes, &mut out);
    out
}

/// The document's tab tree in normal form (see the module docs).
pub fn layout(doc: &Document) -> Vec<TabNode> {
    let tree = &doc.tab_tree;
    if tree.is_empty() {
        return doc.elements.iter().map(|e| TabNode::Tab(e.id)).collect();
    }
    let exists = |e: ElementId| doc.elements.iter().any(|x| x.id == e);
    let mut seen_tabs = std::collections::HashSet::new();
    let mut seen_folders = std::collections::HashSet::new();
    fn build(
        items: &[TabItem],
        tree: &TabTree,
        exists: &dyn Fn(ElementId) -> bool,
        seen_tabs: &mut std::collections::HashSet<ElementId>,
        seen_folders: &mut std::collections::HashSet<ElementId>,
    ) -> Vec<TabNode> {
        let mut out = Vec::new();
        for item in items {
            match *item {
                TabItem::Tab(e) => {
                    if exists(e) && seen_tabs.insert(e) {
                        out.push(TabNode::Tab(e));
                    }
                }
                TabItem::Folder(f) => {
                    let Some(folder) = tree.folder(f) else { continue };
                    if !seen_folders.insert(f) {
                        continue;
                    }
                    let children = build(&folder.items, tree, exists, seen_tabs, seen_folders);
                    out.push(TabNode::Folder { id: f, name: folder.name.clone(), children });
                }
            }
        }
        out
    }
    let mut root = build(&tree.root, tree, &exists, &mut seen_tabs, &mut seen_folders);
    // Folders nobody mentions: at the end of the top level.
    for f in &tree.folders {
        if !seen_folders.contains(&f.id) {
            seen_folders.insert(f.id);
            let children = build(&f.items, tree, &exists, &mut seen_tabs, &mut seen_folders);
            root.push(TabNode::Folder { id: f.id, name: f.name.clone(), children });
        }
    }
    // Tabs nobody mentions: after the element before them.
    for (i, e) in doc.elements.iter().enumerate() {
        if seen_tabs.contains(&e.id) {
            continue;
        }
        let prev = doc.elements[..i].iter().rev().map(|x| x.id).find(|x| seen_tabs.contains(x));
        match prev.and_then(|p| path_to(&root, TabItem::Tab(p))) {
            Some(mut path) => {
                let k = path.pop().unwrap_or(0);
                if let Some(level) = level_mut(&mut root, &path) {
                    level.insert(k + 1, TabNode::Tab(e.id));
                }
            }
            None => root.insert(0, TabNode::Tab(e.id)),
        }
        seen_tabs.insert(e.id);
    }
    root
}

/// The indices leading to `item` (the folders' indices, then its own).
fn path_to(nodes: &[TabNode], item: TabItem) -> Option<Vec<usize>> {
    for (k, n) in nodes.iter().enumerate() {
        if n.item() == item {
            return Some(vec![k]);
        }
        if let TabNode::Folder { children, .. } = n
            && let Some(mut p) = path_to(children, item)
        {
            p.insert(0, k);
            return Some(p);
        }
    }
    None
}

/// The level at the end of `path` (a list of folder indices; empty: the top level).
fn level_mut<'a>(nodes: &'a mut Vec<TabNode>, path: &[usize]) -> Option<&'a mut Vec<TabNode>> {
    let mut cur = nodes;
    for k in path {
        match cur.get_mut(*k)? {
            TabNode::Folder { children, .. } => cur = children,
            TabNode::Tab(_) => return None,
        }
    }
    Some(cur)
}

/// The children of `folder` (`None`: the top level).
pub fn level_of(nodes: &[TabNode], folder: Option<ElementId>) -> Option<&[TabNode]> {
    match folder {
        None => Some(nodes),
        Some(f) => find_folder(nodes, f).map(|n| match n {
            TabNode::Folder { children, .. } => children.as_slice(),
            TabNode::Tab(_) => &[],
        }),
    }
}

fn find_folder(nodes: &[TabNode], f: ElementId) -> Option<&TabNode> {
    for n in nodes {
        if let TabNode::Folder { id, children, .. } = n {
            if *id == f {
                return Some(n);
            }
            if let Some(x) = find_folder(children, f) {
                return Some(x);
            }
        }
    }
    None
}

/// The folder that holds `item` (`None`: the top level, or not found).
pub fn parent_of(nodes: &[TabNode], item: TabItem) -> Option<ElementId> {
    fn walk(nodes: &[TabNode], item: TabItem, parent: Option<ElementId>) -> Option<Option<ElementId>> {
        for n in nodes {
            if n.item() == item {
                return Some(parent);
            }
            if let TabNode::Folder { id, children, .. } = n
                && let Some(p) = walk(children, item, Some(*id))
            {
                return Some(p);
            }
        }
        None
    }
    walk(nodes, item, None).flatten()
}

/// The folders from the top down to `folder` (itself last): (id, name).
pub fn folder_path(nodes: &[TabNode], folder: ElementId) -> Vec<(ElementId, String)> {
    fn walk(nodes: &[TabNode], f: ElementId, acc: &mut Vec<(ElementId, String)>) -> bool {
        for n in nodes {
            if let TabNode::Folder { id, name, children } = n {
                acc.push((*id, name.clone()));
                if *id == f || walk(children, f, acc) {
                    return true;
                }
                acc.pop();
            }
        }
        false
    }
    let mut acc = Vec::new();
    walk(nodes, folder, &mut acc);
    acc
}

/// The tabs inside `folder`, at any depth.
pub fn tabs_in(nodes: &[TabNode], folder: ElementId) -> Vec<ElementId> {
    find_folder(nodes, folder).map(|n| n.tabs()).unwrap_or_default()
}

/// Every folder's (id, name), depth first.
pub fn folders(nodes: &[TabNode]) -> Vec<(ElementId, String, usize)> {
    fn walk(nodes: &[TabNode], depth: usize, out: &mut Vec<(ElementId, String, usize)>) {
        for n in nodes {
            if let TabNode::Folder { id, name, children } = n {
                out.push((*id, name.clone(), depth));
                walk(children, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(nodes, 0, &mut out);
    out
}

/// Writes `nodes` back into `doc`: the tree in normal form (empty when there are no folders)
/// and the elements in the tree's order.
pub fn store(doc: &mut Document, nodes: Vec<TabNode>) {
    let order = flatten(&nodes);
    let mut rest: Vec<crate::document::Element> = Vec::new();
    let mut by_id: std::collections::HashMap<ElementId, crate::document::Element> = std::collections::HashMap::new();
    for e in std::mem::take(&mut doc.elements) {
        if order.contains(&e.id) {
            by_id.insert(e.id, e);
        } else {
            rest.push(e);
        }
    }
    doc.elements = order.iter().filter_map(|id| by_id.remove(id)).collect();
    doc.elements.extend(rest);
    let has_folders = !folders(&nodes).is_empty();
    if !has_folders {
        doc.tab_tree = TabTree::default();
        return;
    }
    let mut tree = TabTree::default();
    fn put(nodes: &[TabNode], tree: &mut TabTree) -> Vec<TabItem> {
        nodes
            .iter()
            .map(|n| {
                if let TabNode::Folder { id, name, children } = n {
                    let items = put(children, tree);
                    tree.folders.push(TabFolder { id: *id, name: name.clone(), items });
                }
                n.item()
            })
            .collect()
    }
    tree.root = put(&nodes, &mut tree);
    doc.tab_tree = tree;
}

/// Takes `item` out of `nodes` (anywhere).
fn take(nodes: &mut Vec<TabNode>, item: TabItem) -> Option<TabNode> {
    if let Some(k) = nodes.iter().position(|n| n.item() == item) {
        return Some(nodes.remove(k));
    }
    for n in nodes.iter_mut() {
        if let TabNode::Folder { children, .. } = n
            && let Some(x) = take(children, item)
        {
            return Some(x);
        }
    }
    None
}

fn children_mut(nodes: &mut Vec<TabNode>, folder: Option<ElementId>) -> Option<&mut Vec<TabNode>> {
    let Some(f) = folder else { return Some(nodes) };
    for n in nodes.iter_mut() {
        if let TabNode::Folder { id, children, .. } = n {
            if *id == f {
                return Some(children);
            }
            if let Some(x) = children_mut(children, Some(f)) {
                return Some(x);
            }
        }
    }
    None
}

/// The next free default folder name ("Folder 1", "Folder 2", …).
pub fn next_folder_name(doc: &Document) -> String {
    let names: Vec<String> = doc.tab_tree.folders.iter().map(|f| f.name.clone()).collect();
    let mut n = 1;
    loop {
        let name = format!("Folder {n}");
        if !names.contains(&name) {
            return name;
        }
        n += 1;
    }
}

fn non_empty(s: &str) -> Result<String, CommandError> {
    let t = s.trim();
    if t.is_empty() {
        return Err(CommandError::Invalid("a folder needs a name".into()));
    }
    Ok(t.to_string())
}

/// Moves `items` (in the order given) into `parent` (`None`: the top level), before `before`
/// (`None`: at the end). Shared by the commands below.
fn move_items(nodes: &mut Vec<TabNode>, items: &[TabItem], parent: Option<ElementId>, before: Option<TabItem>) -> Result<(), CommandError> {
    // A folder can't go into itself or its own subfolders.
    if let Some(p) = parent {
        for it in items {
            if let TabItem::Folder(f) = it
                && (*f == p || folder_path(nodes, p).iter().any(|(id, _)| id == f))
            {
                return Err(CommandError::Invalid("a folder can't go inside itself".into()));
            }
        }
        if find_folder(nodes, p).is_none() {
            return Err(CommandError::ElementNotFound(p));
        }
    }
    let before = before.filter(|b| !items.contains(b));
    let mut taken = Vec::new();
    for it in items {
        match take(nodes, *it) {
            Some(n) => taken.push(n),
            None => return Err(CommandError::ElementNotFound(it.id())),
        }
    }
    let level = children_mut(nodes, parent).ok_or(CommandError::Invalid("no such folder".into()))?;
    let at = before.and_then(|b| level.iter().position(|n| n.item() == b)).unwrap_or(level.len());
    for (k, n) in taken.into_iter().enumerate() {
        level.insert(at + k, n);
    }
    Ok(())
}

/// "+" → **Create folder** (and the Tab manager's New folder): a new folder in `parent`, before
/// `before` (`None`: at the end of that level), holding `items`.
#[derive(Debug, Clone)]
pub struct CreateTabFolder {
    pub id: ElementId,
    /// `None` picks the next "Folder N".
    pub name: Option<String>,
    pub parent: Option<ElementId>,
    pub before: Option<TabItem>,
    pub items: Vec<TabItem>,
}

impl Command for CreateTabFolder {
    fn label(&self) -> String {
        "Create folder".into()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if doc.tab_tree.folder(self.id).is_some() || doc.element(self.id).is_some() {
            return Err(CommandError::Invalid("folder id already in use".into()));
        }
        let name = match &self.name {
            Some(n) => non_empty(n)?,
            None => next_folder_name(doc),
        };
        let mut nodes = layout(doc);
        // The folder goes where the first of its items was (in `parent`), unless a place is given.
        let before = self.before.or_else(|| self.items.first().copied());
        let level = children_mut(&mut nodes, self.parent).ok_or(CommandError::Invalid("no such folder".into()))?;
        let at = before.and_then(|b| level.iter().position(|n| n.item() == b)).unwrap_or(level.len());
        level.insert(at, TabNode::Folder { id: self.id, name, children: Vec::new() });
        move_items(&mut nodes, &self.items, Some(self.id), None)?;
        store(doc, nodes);
        Ok(())
    }
}

/// Renames a folder.
#[derive(Debug, Clone)]
pub struct RenameTabFolder {
    pub id: ElementId,
    pub name: String,
}

impl Command for RenameTabFolder {
    fn label(&self) -> String {
        format!("Rename folder to {}", self.name.trim())
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = non_empty(&self.name)?;
        let f = doc.tab_tree.folders.iter_mut().find(|f| f.id == self.id).ok_or(CommandError::ElementNotFound(self.id))?;
        f.name = name;
        Ok(())
    }
}

/// Deletes a folder. `delete_tabs`: its tabs (and subfolders) go too; otherwise they move up to
/// the folder's place in its parent.
#[derive(Debug, Clone)]
pub struct DeleteTabFolder {
    pub id: ElementId,
    pub delete_tabs: bool,
}

impl Command for DeleteTabFolder {
    fn label(&self) -> String {
        if self.delete_tabs { "Delete folder and its tabs".into() } else { "Delete folder".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let mut nodes = layout(doc);
        let parent = parent_of(&nodes, TabItem::Folder(self.id));
        let level = children_mut(&mut nodes, parent).ok_or(CommandError::ElementNotFound(self.id))?;
        let k = level.iter().position(|n| n.item() == TabItem::Folder(self.id)).ok_or(CommandError::ElementNotFound(self.id))?;
        let TabNode::Folder { children, .. } = level.remove(k) else { unreachable!() };
        if self.delete_tabs {
            let gone: Vec<ElementId> = flatten(&children);
            if gone.len() >= doc.elements.len() {
                return Err(CommandError::Invalid("cannot delete the last tab".into()));
            }
            doc.elements.retain(|e| !gone.contains(&e.id));
        } else {
            for (j, c) in children.into_iter().enumerate() {
                level.insert(k + j, c);
            }
        }
        store(doc, nodes);
        Ok(())
    }
}

/// Moves tabs and folders into a folder (or to the top level) at a place: dragging tabs in and
/// out of folders, and reordering them, in the tab bar and the Tab manager.
#[derive(Debug, Clone)]
pub struct MoveTabItems {
    pub items: Vec<TabItem>,
    /// `None`: the top level.
    pub parent: Option<ElementId>,
    /// `None`: at the end of that level.
    pub before: Option<TabItem>,
    pub label: String,
}

impl Command for MoveTabItems {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.items.is_empty() {
            return Err(CommandError::Invalid("nothing to move".into()));
        }
        let mut nodes = layout(doc);
        move_items(&mut nodes, &self.items, self.parent, self.before)?;
        store(doc, nodes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Element;

    fn doc(n: usize) -> Document {
        let mut d = Document::empty("T");
        for k in 0..n {
            d.elements.push(Element::part_studio(format!("Tab {}", k + 1)));
        }
        d
    }

    #[test]
    fn no_folders_is_the_element_order() {
        let d = doc(3);
        let l = layout(&d);
        assert_eq!(flatten(&l), d.elements.iter().map(|e| e.id).collect::<Vec<_>>());
    }

    #[test]
    fn a_new_tab_lands_after_its_neighbour_in_its_folder() {
        let mut d = doc(3);
        let ids: Vec<ElementId> = d.elements.iter().map(|e| e.id).collect();
        let f = ElementId::new();
        CreateTabFolder { id: f, name: None, parent: None, before: None, items: vec![TabItem::Tab(ids[1])] }.apply(&mut d).unwrap();
        // A tab inserted right after Tab 2 in the element list joins Folder 1.
        let e = Element::part_studio("New");
        let new = e.id;
        let i = d.element_index(ids[1]).unwrap();
        d.elements.insert(i + 1, e);
        let l = layout(&d);
        assert_eq!(tabs_in(&l, f), vec![ids[1], new]);
    }
}
