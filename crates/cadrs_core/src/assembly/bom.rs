//! **Bill of Materials** (P3B.6, `intro-to-assemblies.md` A20, X13; used again by the drawings'
//! BOM tables, P3C.5): a live table computed from an assembly's instances and their
//! [`properties`](crate::properties), with the per-assembly settings ([`BomSettings`]: columns,
//! view, rows suppressed from this BOM, the top-level row) kept on the [`Assembly`].
//!
//! **Rows.** One row per *item*: a part or subassembly source, in the order its first instance
//! has in the Instances list (A20.3), with the number of instances as its quantity. Suppressed
//! instances are left out.
//! - **Structured** ([`BomView::Structured`], A20.4): the top level's items, numbered 1, 2, …; a
//!   subassembly's items under it (2.1, 2.2, …, quantities per one subassembly) when it is
//!   expanded. Each subassembly's **Subassembly BOM behavior** (A20.5, [`SubassemblyBom`]) says
//!   whether it shows with its components, alone, or only as its components (merged into the
//!   parent's rows as if inserted there).
//! - **Flattened** ([`BomView::Flattened`]): every part at any depth as if it were at the top
//!   level, quantities summed over the whole tree; a subassembly shown "assembly only" is one
//!   item.
//!
//! **Suppress from this BOM** (A20.8): a row whose [`BomRowKey`] is in
//! [`BomSettings::excluded`] (with its children) gets no item number and counts in no total; it
//! is listed with "–" as its item only while [`BomSettings::show_excluded`] is on.
//!
//! **Top-level assembly row** (A20.9): the assembly itself first, with the total quantity and
//! mass of the rows ([`Bom::total_quantity`], [`Bom::total_mass`]).
//!
//! **Templates** (A20.7): [`BomTemplate`]s saved in the document ([`SaveBomTemplate`],
//! [`ApplyBomTemplate`]). **Copy table** and **Export to CSV**: [`Bom::to_tsv`], [`Bom::to_csv`].
//!
//! Cells are the owners' properties ([`crate::properties::text`]), so editing a cell is setting
//! the property ([`crate::properties::SetProperties`]) and every property edit shows in the BOM
//! (two-way, A20.10).

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{Assembly, InstanceId, InstanceSource};
use super::structure::{derive, occurrences};
use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;
use crate::properties::{self, PropertyKey, PropertyOwner, PropertySettings, SubassemblyBom};
use crate::rebuild::Build;
use cadrs_sketch::units::Units;

/// How deep subassemblies are followed (a guard).
const MAX_DEPTH: usize = 16;

/// A BOM column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BomColumn {
    /// The item number ("2.1"; "–" for a row suppressed from the BOM).
    Item,
    Quantity,
    /// A property of the row's part or assembly.
    Property(PropertyKey),
}

impl BomColumn {
    pub fn label(&self, settings: &PropertySettings) -> String {
        match self {
            BomColumn::Item => "Item".into(),
            BomColumn::Quantity => "Quantity".into(),
            BomColumn::Property(k) => k.label(settings),
        }
    }

    /// A cell of this column is edited in place (text), or with the material picker.
    pub fn editable(&self) -> bool {
        matches!(self, BomColumn::Property(k) if k.is_text() || *k == PropertyKey::Material)
    }

    /// The property the column shows, if any.
    pub fn property(&self) -> Option<PropertyKey> {
        match self {
            BomColumn::Property(k) => Some(*k),
            _ => None,
        }
    }
}

/// The default columns (Onshape's default template).
pub fn default_columns() -> Vec<BomColumn> {
    vec![
        BomColumn::Item,
        BomColumn::Quantity,
        BomColumn::Property(PropertyKey::PartNumber),
        BomColumn::Property(PropertyKey::Name),
        BomColumn::Property(PropertyKey::Description),
        BomColumn::Property(PropertyKey::Material),
    ]
}

/// Structured or Flattened (A20.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BomView {
    #[default]
    Structured,
    Flattened,
}

/// Which row: the item's owner and the subassembly tabs it sits in (outermost first; empty at
/// the top level and in the flattened view).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BomRowKey {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<ElementId>,
    pub owner: PropertyOwner,
}

/// An assembly's BOM settings (saved with the assembly; changed with [`SetBomSettings`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BomSettings {
    #[serde(default = "default_columns")]
    pub columns: Vec<BomColumn>,
    #[serde(default)]
    pub view: BomView,
    /// Rows suppressed from this BOM (A20.8).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<BomRowKey>,
    /// **Show excluded/suppressed** (A20.8).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_excluded: bool,
    /// **Show top-level assembly row** (A20.9).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub top_level_row: bool,
}

impl Default for BomSettings {
    fn default() -> Self {
        Self { columns: default_columns(), view: BomView::default(), excluded: Vec::new(), show_excluded: false, top_level_row: false }
    }
}

impl BomSettings {
    pub fn is_default(&self) -> bool {
        *self == BomSettings::default()
    }

    /// The settings with `template`'s columns and view options.
    pub fn with_template(&self, template: &BomTemplate) -> Self {
        Self { columns: template.columns.clone(), view: template.view, show_excluded: template.show_excluded, top_level_row: template.top_level_row, excluded: self.excluded.clone() }
    }
}

/// A saved BOM layout (A20.7): the columns and view options, not the rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BomTemplate {
    pub name: String,
    pub columns: Vec<BomColumn>,
    #[serde(default)]
    pub view: BomView,
    #[serde(default)]
    pub show_excluded: bool,
    #[serde(default)]
    pub top_level_row: bool,
}

impl BomTemplate {
    /// The name of the built-in template (listed first under Apply template).
    pub const DEFAULT_NAME: &'static str = "Default";

    /// The built-in **Default** template: the default columns, structured, nothing extra shown.
    pub fn builtin_default() -> Self {
        Self::of(Self::DEFAULT_NAME, &BomSettings::default())
    }

    /// A template of `settings`' layout.
    pub fn of(name: impl Into<String>, settings: &BomSettings) -> Self {
        Self { name: name.into(), columns: settings.columns.clone(), view: settings.view, show_excluded: settings.show_excluded, top_level_row: settings.top_level_row }
    }
}

/// How to show the BOM (view state that is not saved with it).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BomOptions {
    /// Every subassembly expanded in the structured view.
    pub expand_all: bool,
    /// The subassembly rows expanded (double-clicking the item number, A20.4).
    pub expanded: Vec<BomRowKey>,
    /// Sort by a column (double-clicking its header, A20.3): the column and ascending. Rows are
    /// sorted among their siblings; the top-level row stays first.
    pub sort: Option<(BomColumn, bool)>,
}

/// One BOM row.
#[derive(Debug, Clone, PartialEq)]
pub struct BomRow {
    pub key: BomRowKey,
    /// Its item number ("2.1"); `None` for a row suppressed from the BOM and for the top-level
    /// row.
    pub item: Option<String>,
    /// 0 at the top level.
    pub depth: usize,
    /// Instances of the item (per one parent in the structured view).
    pub quantity: u32,
    /// Suppressed from this BOM (itself or a parent).
    pub excluded: bool,
    /// The top-level assembly row (A20.9).
    pub top_level: bool,
    /// A subassembly row with components listed under it (expanded or not).
    pub has_children: bool,
    pub expanded: bool,
    /// The instances of the assembly this row stands for (top-level instances: a row inside a
    /// subassembly gives the subassembly's instances).
    pub instances: Vec<InstanceId>,
    /// The occurrences (parts at any depth, [`super::structure::Occurrence::id`]) the row
    /// covers: what hovering it highlights (A20.2).
    pub occurrences: Vec<InstanceId>,
    /// One of the item, kg (`None` without a material).
    pub unit_mass: Option<f64>,
    /// The cells, one per column.
    pub cells: Vec<String>,
}

/// A computed BOM.
#[derive(Debug, Clone, PartialEq)]
pub struct Bom {
    pub columns: Vec<BomColumn>,
    /// The column headers.
    pub labels: Vec<String>,
    /// The rows shown, in order.
    pub rows: Vec<BomRow>,
    /// The number of parts and subassemblies of the top-level rows not suppressed (A20.9).
    pub total_quantity: u32,
    /// Their mass, kg (`None` if one has none).
    pub total_mass: Option<f64>,
}

/// An item while the rows are gathered.
#[derive(Debug, Clone)]
struct Node {
    owner: PropertyOwner,
    path: Vec<ElementId>,
    count: u32,
    instances: Vec<InstanceId>,
    occurrences: Vec<InstanceId>,
    /// A subassembly listed with its components.
    children: Option<Vec<Node>>,
}

fn merge_into(nodes: &mut Vec<Node>, n: Node, add_count: bool) {
    match nodes.iter_mut().find(|x| x.owner == n.owner && x.path == n.path) {
        Some(x) => {
            if add_count {
                x.count += n.count;
            }
            for i in n.instances {
                if !x.instances.contains(&i) {
                    x.instances.push(i);
                }
            }
            x.occurrences.extend(n.occurrences);
            if let (Some(xc), Some(nc)) = (x.children.as_mut(), n.children) {
                // Per one subassembly: the children's counts stay.
                for c in nc {
                    merge_into(xc, c, false);
                }
            }
        }
        None => nodes.push(n),
    }
}

/// The occurrences (in the root's ids, through `wrap`) of every part of the subassembly
/// instance `inst` whose tab is `child`.
fn sub_occurrences(doc: &Document, child: &Assembly, inst: InstanceId, wrap: &dyn Fn(InstanceId) -> InstanceId) -> Vec<InstanceId> {
    occurrences(doc, child).into_iter().map(|o| wrap(derive(inst, o.id))).collect()
}

/// The structured items of `asm` (at `path`, under the root's instance `top`).
fn structured(doc: &Document, el: ElementId, asm: &Assembly, path: &[ElementId], top: Option<InstanceId>, wrap: &dyn Fn(InstanceId) -> InstanceId, depth: usize) -> Vec<Node> {
    let mut nodes: Vec<Node> = Vec::new();
    for inst in asm.instances.iter().filter(|i| !i.suppressed) {
        let owner = PropertyOwner::from(inst.source);
        let top_id = top.unwrap_or(inst.id);
        match inst.source {
            InstanceSource::Part { .. } => {
                merge_into(&mut nodes, Node { owner, path: path.to_vec(), count: 1, instances: vec![top_id], occurrences: vec![wrap(inst.id)], children: None }, true);
            }
            // A rigid Part Studio instance (A2.4): each of its parts is an item.
            InstanceSource::Studio { element } => {
                for n in studio_nodes(inst, element, path, top_id, wrap) {
                    merge_into(&mut nodes, n, true);
                }
            }
            InstanceSource::Assembly { element } => {
                let Some(child) = doc.element(element).and_then(|e| e.assembly_model()) else { continue };
                if depth >= MAX_DEPTH {
                    continue;
                }
                let id = inst.id;
                let inner = move |x: InstanceId| wrap(derive(id, x));
                let occ = sub_occurrences(doc, child, id, wrap);
                match child.properties.bom_behavior {
                    SubassemblyBom::ComponentsOnly => {
                        for n in structured(doc, element, child, path, Some(top_id), &inner, depth + 1) {
                            merge_into(&mut nodes, n, true);
                        }
                    }
                    SubassemblyBom::AssemblyOnly => {
                        merge_into(&mut nodes, Node { owner, path: path.to_vec(), count: 1, instances: vec![top_id], occurrences: occ, children: None }, true);
                    }
                    SubassemblyBom::AssemblyAndComponents => {
                        let mut sub_path = path.to_vec();
                        sub_path.push(element);
                        let kids = structured(doc, element, child, &sub_path, Some(top_id), &inner, depth + 1);
                        merge_into(&mut nodes, Node { owner, path: path.to_vec(), count: 1, instances: vec![top_id], occurrences: occ, children: Some(kids) }, true);
                    }
                }
            }
        }
    }
    // A1.7: its Items, after the instances.
    for n in item_nodes(asm, el, path) {
        merge_into(&mut nodes, n, true);
    }
    nodes
}

/// The items of a rigid Part Studio instance: one per part.
fn studio_nodes(inst: &super::Instance, element: ElementId, path: &[ElementId], top_id: InstanceId, wrap: &dyn Fn(InstanceId) -> InstanceId) -> Vec<Node> {
    inst.parts
        .iter()
        .map(|part| Node {
            owner: PropertyOwner::Part { element, part: *part },
            path: path.to_vec(),
            count: 1,
            instances: vec![top_id],
            occurrences: vec![wrap(derive(inst.id, super::structure::studio_part_key(*part)))],
            children: None,
        })
        .collect()
}

/// The assembly `el`'s non-geometric Items (A1.7), with their quantities.
fn item_nodes(asm: &Assembly, el: ElementId, path: &[ElementId]) -> Vec<Node> {
    asm.items
        .iter()
        .map(|i| Node { owner: PropertyOwner::Item { element: el, item: i.id }, path: path.to_vec(), count: i.quantity, instances: Vec::new(), occurrences: Vec::new(), children: None })
        .collect()
}

/// The flattened items of `asm`: every part at any depth, counted over the whole tree.
fn flattened(doc: &Document, el: ElementId, asm: &Assembly, top: Option<InstanceId>, wrap: &dyn Fn(InstanceId) -> InstanceId, depth: usize, nodes: &mut Vec<Node>) {
    for inst in asm.instances.iter().filter(|i| !i.suppressed) {
        let owner = PropertyOwner::from(inst.source);
        let top_id = top.unwrap_or(inst.id);
        match inst.source {
            InstanceSource::Part { .. } => {
                merge_into(nodes, Node { owner, path: Vec::new(), count: 1, instances: vec![top_id], occurrences: vec![wrap(inst.id)], children: None }, true);
            }
            InstanceSource::Studio { element } => {
                for n in studio_nodes(inst, element, &[], top_id, wrap) {
                    merge_into(nodes, n, true);
                }
            }
            InstanceSource::Assembly { element } => {
                let Some(child) = doc.element(element).and_then(|e| e.assembly_model()) else { continue };
                if depth >= MAX_DEPTH {
                    continue;
                }
                let id = inst.id;
                if child.properties.bom_behavior == SubassemblyBom::AssemblyOnly {
                    let occ = sub_occurrences(doc, child, id, wrap);
                    merge_into(nodes, Node { owner, path: Vec::new(), count: 1, instances: vec![top_id], occurrences: occ, children: None }, true);
                } else {
                    let inner = move |x: InstanceId| wrap(derive(id, x));
                    flattened(doc, element, child, Some(top_id), &inner, depth + 1, nodes);
                }
            }
        }
    }
    for n in item_nodes(asm, el, &[]) {
        merge_into(nodes, n, true);
    }
}

/// A mass in the workspace units ("0.484 lb").
pub fn format_mass(kg: f64, units: &Units) -> String {
    let v = kg / units.mass.kg();
    format!("{:.*} {}", units.decimals as usize, v, units.mass.symbol())
}

/// Compares two cells: numbers as numbers, item numbers ("2.10") by their parts, else text
/// (case-insensitive).
fn compare_cells(a: &str, b: &str) -> Ordering {
    let parts = |s: &str| -> Option<Vec<u32>> { s.split('.').map(|p| p.parse().ok()).collect() };
    if let (Some(x), Some(y)) = (parts(a), parts(b)) {
        return x.cmp(&y);
    }
    let num = |s: &str| s.split_whitespace().next().and_then(|w| w.parse::<f64>().ok());
    if let (Some(x), Some(y)) = (num(a), num(b)) {
        return x.total_cmp(&y);
    }
    // Empty cells last.
    match (a.is_empty(), b.is_empty()) {
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// The BOM of the assembly `element` with its saved settings. `build_of` gives each Part
/// Studio's rebuild (for part names, colours and masses); `units` formats the Mass column.
pub fn compute(doc: &Document, element: ElementId, options: &BomOptions, units: &Units, mut build_of: impl FnMut(ElementId) -> Option<Arc<Build>>) -> Result<Bom, CommandError> {
    let asm = doc.element(element).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(element))?;
    compute_with(doc, element, &asm.bom, options, units, &mut build_of)
}

/// [`compute`] with other `settings` (a template's preview, the drawings' BOM tables).
pub fn compute_with(
    doc: &Document,
    element: ElementId,
    settings: &BomSettings,
    options: &BomOptions,
    units: &Units,
    build_of: &mut dyn FnMut(ElementId) -> Option<Arc<Build>>,
) -> Result<Bom, CommandError> {
    let asm = doc.element(element).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(element))?;
    let id = |x: InstanceId| x;
    let nodes = match settings.view {
        BomView::Structured => structured(doc, element, asm, &[], None, &id, 0),
        BomView::Flattened => {
            let mut n = Vec::new();
            flattened(doc, element, asm, None, &id, 0, &mut n);
            n
        }
    };
    let columns = settings.columns.clone();
    let labels = columns.iter().map(|c| c.label(&doc.properties)).collect();
    // Each studio's rebuild and each owner's mass, asked for once.
    let cache: RefCell<HashMap<ElementId, Option<Arc<Build>>>> = RefCell::new(HashMap::new());
    let build_of = RefCell::new(build_of);
    let build = |e: ElementId| -> Option<Arc<Build>> {
        if let Some(b) = cache.borrow().get(&e) {
            return b.clone();
        }
        let b = (build_of.borrow_mut())(e);
        cache.borrow_mut().insert(e, b.clone());
        b
    };
    let masses: RefCell<HashMap<PropertyOwner, Option<f64>>> = RefCell::new(HashMap::new());
    let mass_of = |o: PropertyOwner| -> Option<f64> {
        if let Some(m) = masses.borrow().get(&o) {
            return *m;
        }
        let m = properties::mass(doc, o, &mut |e| build(e));
        masses.borrow_mut().insert(o, m);
        m
    };
    let ctx = Ctx { doc, settings, options, units, build: &build, mass: &mass_of };
    let mut rows = Vec::new();
    emit(&ctx, &nodes, 0, None, false, &mut rows);
    // Totals over the top level's rows not suppressed.
    let top: Vec<&BomRow> = rows.iter().filter(|r| r.depth == 0 && !r.excluded).collect();
    let total_quantity = top.iter().map(|r| r.quantity).sum();
    let total_mass = top.iter().try_fold(0.0, |acc, r| r.unit_mass.map(|m| acc + m * r.quantity as f64));
    if settings.top_level_row {
        let owner = PropertyOwner::Assembly { element };
        let mut row = BomRow {
            key: BomRowKey { path: Vec::new(), owner },
            item: None,
            depth: 0,
            quantity: total_quantity,
            excluded: false,
            top_level: true,
            has_children: false,
            expanded: false,
            instances: asm.instances.iter().filter(|i| !i.suppressed).map(|i| i.id).collect(),
            occurrences: occurrences(doc, asm).into_iter().map(|o| o.id).collect(),
            unit_mass: properties::properties(doc, owner).mass_override.or(total_mass),
            cells: Vec::new(),
        };
        row.cells = columns.iter().map(|c| cell(&ctx, c, &row)).collect();
        rows.insert(0, row);
    }
    Ok(Bom { columns, labels, rows, total_quantity, total_mass })
}

/// What the rows are made with.
struct Ctx<'a> {
    doc: &'a Document,
    settings: &'a BomSettings,
    options: &'a BomOptions,
    units: &'a Units,
    build: &'a dyn Fn(ElementId) -> Option<Arc<Build>>,
    mass: &'a dyn Fn(PropertyOwner) -> Option<f64>,
}

fn cell(ctx: &Ctx, col: &BomColumn, row: &BomRow) -> String {
    match col {
        BomColumn::Item if row.top_level => String::new(),
        BomColumn::Item => row.item.clone().unwrap_or_else(|| "–".into()),
        BomColumn::Quantity => row.quantity.to_string(),
        BomColumn::Property(PropertyKey::Mass) => row.unit_mass.map(|m| format_mass(m, ctx.units)).unwrap_or_default(),
        BomColumn::Property(k) => {
            let b = (ctx.build)(row.key.owner.element());
            properties::text(ctx.doc, row.key.owner, *k, b.as_deref())
        }
    }
}

/// The rows of `nodes` (and of the expanded ones' children), numbered in list order, sorted
/// among siblings.
fn emit(ctx: &Ctx, nodes: &[Node], depth: usize, prefix: Option<&str>, parent_excluded: bool, rows: &mut Vec<BomRow>) {
    let mut k = 0;
    let mut level: Vec<(BomRow, Option<&Vec<Node>>)> = Vec::new();
    for n in nodes {
        let key = BomRowKey { path: n.path.clone(), owner: n.owner };
        let excluded = parent_excluded || ctx.settings.excluded.contains(&key);
        let item = (!excluded).then(|| {
            k += 1;
            match prefix {
                Some(p) => format!("{p}.{k}"),
                None => k.to_string(),
            }
        });
        let expanded = n.children.is_some() && (ctx.options.expand_all || ctx.options.expanded.contains(&key));
        let mut row = BomRow {
            key,
            item,
            depth,
            quantity: n.count,
            excluded,
            top_level: false,
            has_children: n.children.as_ref().is_some_and(|c| !c.is_empty()),
            expanded,
            instances: n.instances.clone(),
            occurrences: n.occurrences.clone(),
            unit_mass: (ctx.mass)(n.owner),
            cells: Vec::new(),
        };
        row.cells = ctx.settings.columns.iter().map(|c| cell(ctx, c, &row)).collect();
        level.push((row, n.children.as_ref()));
    }
    if let Some((col, asc)) = ctx.options.sort
        && let Some(i) = ctx.settings.columns.iter().position(|c| *c == col)
    {
        level.sort_by(|a, b| {
            let o = compare_cells(&a.0.cells[i], &b.0.cells[i]);
            if asc { o } else { o.reverse() }
        });
    }
    for (row, kids) in level {
        if row.excluded && !ctx.settings.show_excluded {
            continue;
        }
        let (expanded, excluded, item) = (row.expanded, row.excluded, row.item.clone());
        rows.push(row);
        if let (true, Some(kids)) = (expanded, kids) {
            emit(ctx, kids, depth + 1, item.as_deref(), excluded, rows);
        }
    }
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

impl Bom {
    /// **Export to CSV** (A20.7): the headers and the rows shown, comma-separated, quoted where
    /// needed (RFC 4180), one line each.
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        let line = |cells: &mut dyn Iterator<Item = &String>| cells.map(|c| csv_field(c)).collect::<Vec<_>>().join(",");
        out.push_str(&line(&mut self.labels.iter()));
        out.push('\n');
        for r in &self.rows {
            out.push_str(&line(&mut r.cells.iter()));
            out.push('\n');
        }
        out
    }

    /// **Copy table** (A20.7): tab-separated, for pasting into a spreadsheet.
    pub fn to_tsv(&self) -> String {
        let clean = |s: &String| s.replace(['\t', '\n', '\r'], " ");
        let mut out = self.labels.iter().map(clean).collect::<Vec<_>>().join("\t");
        out.push('\n');
        for r in &self.rows {
            out.push_str(&r.cells.iter().map(clean).collect::<Vec<_>>().join("\t"));
            out.push('\n');
        }
        out
    }

    /// Every owner in the BOM, each once, in row order: what **Generate missing part numbers**
    /// numbers (A20.11). The top-level row is left out.
    pub fn owners(&self) -> Vec<PropertyOwner> {
        let mut out: Vec<PropertyOwner> = Vec::new();
        for r in self.rows.iter().filter(|r| !r.top_level) {
            if !out.contains(&r.key.owner) {
                out.push(r.key.owner);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

/// Sets an assembly's BOM settings (columns added, removed or moved, the view, a row suppressed
/// or unsuppressed, show excluded, the top-level row). One undo step.
#[derive(Debug, Clone)]
pub struct SetBomSettings {
    pub element: ElementId,
    pub settings: BomSettings,
    /// Shown in the undo menu ("Add BOM column").
    pub label: String,
}

impl Command for SetBomSettings {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.settings.columns.is_empty() {
            return Err(CommandError::Invalid("a BOM needs a column".into()));
        }
        let mut seen = Vec::new();
        for c in &self.settings.columns {
            if seen.contains(c) {
                return Err(CommandError::Invalid("a column is there twice".into()));
            }
            seen.push(*c);
        }
        super::commands::assembly_mut(doc, self.element)?.bom = self.settings.clone();
        Ok(())
    }
}

/// **Save as template** (A20.7): the assembly's BOM layout saved in the document under `name`
/// (replacing a template of that name). One undo step.
#[derive(Debug, Clone)]
pub struct SaveBomTemplate {
    pub element: ElementId,
    pub name: String,
}

impl Command for SaveBomTemplate {
    fn label(&self) -> String {
        format!("Save BOM template {}", self.name.trim())
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(CommandError::Invalid("the name can't be empty".into()));
        }
        let settings = doc.element(self.element).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(self.element))?.bom.clone();
        let t = BomTemplate::of(name, &settings);
        let list = &mut doc.properties.bom_templates;
        match list.iter_mut().find(|x| x.name == name) {
            Some(x) => *x = t,
            None => list.push(t),
        }
        Ok(())
    }
}

/// **Apply template** (A20.7): a saved template's layout on the assembly's BOM (its suppressed
/// rows stay). One undo step. The name [`BomTemplate::DEFAULT_NAME`] applies the built-in
/// default layout unless the document saved a template of that name.
#[derive(Debug, Clone)]
pub struct ApplyBomTemplate {
    pub element: ElementId,
    pub name: String,
}

impl Command for ApplyBomTemplate {
    fn label(&self) -> String {
        format!("Apply BOM template {}", self.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let t = doc
            .properties
            .bom_templates
            .iter()
            .find(|t| t.name == self.name)
            .cloned()
            .or_else(|| (self.name == BomTemplate::DEFAULT_NAME).then(BomTemplate::builtin_default))
            .ok_or_else(|| CommandError::Invalid(format!("no template {}", self.name)))?;
        let asm = super::commands::assembly_mut(doc, self.element)?;
        asm.bom = asm.bom.with_template(&t);
        Ok(())
    }
}
