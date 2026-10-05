//! cadrs_drawing: the drawing element's model (stage 3C), free of Bevy.
//!
//! A [`Drawing`] is a list of [`Sheet`]s plus drawing-wide settings: the template it came from,
//! the projection method, the units, the drawing properties ([`style::DrawingStyle`]) and the
//! title-block properties. Each sheet has a format (standard size and orientation), a scale,
//! border/zone/title-block switches and the part or assembly it references (the title block
//! and parametric notes read that one's properties).
//!
//! Every edit is a [`DrawingOp`], applied through `cadrs_core`'s command layer
//! (`commands::EditDrawing`) so it is undoable.
//!
//! Each sheet holds its [`View`]s (P3C.2, see [`view`]): a part or Part Studio seen from a
//! direction at a scale, placed by the sheet point of the model origin, with projected and
//! auxiliary views aligned to their parent. Views reference model elements through
//! [`ObjectRef`]; the lines themselves come from the kernel's hidden-line removal
//! (`cadrs_kernel::Kernel::project`) and are drawn with [`view::view_lines`]. Each view holds
//! its annotations (P3C.3, see [`annotation`]): centerlines, centermarks, virtual sharps,
//! driven dimensions and hole callouts, attached to the model through persistent topology names
//! (`cadrs_kernel::ProjSource`), so they move and go to other sheets with their view.
//!
//! **Updating (P3C.6, D13).** A drawing shows the model as of its last update, not the live
//! workspace: [`Drawing::sources`] keeps a snapshot of each referenced Part Studio
//! ([`ModelSource`]) with the dependency hash of each of its parts, and each view the hash of the
//! part it shows ([`View::source_hash`]). A view is out of date when the workspace's hash of its
//! part differs ([`Drawing::out_of_date`]); "Update from this workspace" (Ctrl+Q) replaces the
//! snapshot and refreshes the views' annotations in one [`DrawingOp::Update`] (see [`update`]).

pub mod annotation;
pub mod assembly;
pub mod annotation_more;
pub mod graphics;
pub mod note;
pub mod rich;
pub mod sheet_sketch;
pub mod export;
pub mod pdf;
pub mod dxf;
pub mod dwg;
pub mod raster;
pub mod standard;
pub mod style;
pub mod table;
pub mod template;
pub mod title_block;
pub mod update;
pub mod view;
pub mod view_kinds;
pub mod flat_view;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use graphics::{Graphics, sheet_graphics};
pub use note::{Note, NoteId};
pub use table::{Table, TableId};
pub use standard::{Orientation, Projection, Scale, SheetFormat, SheetSize, Standard};
pub use style::DrawingStyle;
pub use template::{DrawingUnits, Template, TemplateSource, builtin_templates};
pub use title_block::{ReferenceProps, TitleProps};
pub use view::{Frame3, NamedView, View, ViewId, ViewKind};

/// Identifies a sheet within a drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SheetId(pub Uuid);

impl SheetId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for SheetId {
    fn default() -> Self {
        Self::new()
    }
}

/// A part or assembly a sheet or view references: a document element (by its id's UUID, so
/// this crate need not know `cadrs_core`), optionally one part of a Part Studio (the part's
/// feature UUID and body index, as `cadrs_core::PartId`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectRef {
    pub element: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<(Uuid, u32)>,
}

/// A dependency hash of one part (or, with `part: None`, of the whole Part Studio) of a
/// [`ModelSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartHash {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<(Uuid, u32)>,
    pub hash: u64,
}

/// The state of a referenced Part Studio the drawing's views show (P3C.6): a snapshot written
/// by `cadrs_core` (its features, part settings and appearances, as RON) and the dependency hash
/// of each of its parts at that state. Views keep showing it until the drawing is updated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSource {
    /// The Part Studio (a document element's UUID).
    pub element: Uuid,
    /// Its state, as `cadrs_core::drawing_source` writes it.
    pub snapshot: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartHash>,
    /// An assembly's occurrences and their parts' properties (P3C.5: what its callouts read);
    /// `None` for a Part Studio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly: Option<assembly::AssemblyInfo>,
    /// The drawing's reference to this source is pinned (P3G.2, ER5.6): Update all skips it.
    /// Only a reference to a version (a linked copy) is pinned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl ModelSource {
    /// The dependency hash of `part` (the whole studio for `None`) in this state.
    pub fn hash_of(&self, part: Option<(Uuid, u32)>) -> Option<u64> {
        self.parts.iter().find(|p| p.part == part).map(|p| p.hash)
    }
}

/// One view's part of an update (see [`DrawingOp::Update`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewUpdate {
    pub id: ViewId,
    pub source_hash: Option<u64>,
    /// Its annotations with their references refreshed (see [`update::refreshed`]).
    pub annotations: Vec<annotation::Annotation>,
}

/// One sheet of a drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: SheetId,
    pub name: String,
    pub format: SheetFormat,
    #[serde(default)]
    pub scale: Scale,
    #[serde(default = "yes")]
    pub border: bool,
    #[serde(default = "yes")]
    pub zones: bool,
    #[serde(default = "yes")]
    pub title_block: bool,
    /// What the title block and parametric notes read properties from (D2.8, D9.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<ObjectRef>,
    /// The views on the sheet (P3C.2), in the order they were placed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<View>,
    /// Notes (P3C.4), in the order they were placed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// Tables (P3C.4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<Table>,
    /// Sheet sketch lines and splines, inserted DXF blocks and images (P3C.7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch: Vec<sheet_sketch::SketchItem>,
}

fn yes() -> bool {
    true
}

/// A sheet's editable settings (the Sheet properties dialog, D2.8).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetProps {
    pub format: SheetFormat,
    pub scale: Scale,
    pub border: bool,
    pub zones: bool,
    pub title_block: bool,
    pub reference: Option<ObjectRef>,
}

impl Sheet {
    pub fn new(name: impl Into<String>, format: SheetFormat) -> Self {
        Self {
            id: SheetId::new(),
            name: name.into(),
            format,
            scale: Scale::default(),
            border: true,
            zones: true,
            title_block: true,
            reference: None,
            views: Vec::new(),
            notes: Vec::new(),
            tables: Vec::new(),
            sketch: Vec::new(),
        }
    }

    pub fn view(&self, id: ViewId) -> Option<&View> {
        self.views.iter().find(|v| v.id == id)
    }

    pub fn props(&self) -> SheetProps {
        SheetProps {
            format: self.format,
            scale: self.scale,
            border: self.border,
            zones: self.zones,
            title_block: self.title_block,
            reference: self.reference,
        }
    }

    /// Width and height in mm.
    pub fn size_mm(&self) -> (f64, f64) {
        self.format.size_mm()
    }
}

/// The template a drawing was made from, as recorded in the drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateInfo {
    pub name: String,
    pub source: TemplateSource,
}

/// A drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub template: TemplateInfo,
    pub projection: Projection,
    pub units: DrawingUnits,
    /// The drawing properties (defaults for every sheet).
    pub style: DrawingStyle,
    /// "Lock drawing properties": the panel is read-only while set.
    #[serde(default)]
    pub locked: bool,
    /// Drawn / checked / approved names and dates, company.
    #[serde(default)]
    pub title: TitleProps,
    pub sheets: Vec<Sheet>,
    /// The state of each referenced Part Studio the views show (P3C.6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<ModelSource>,
}

impl Drawing {
    /// A new drawing from a template: one sheet, "Sheet1", referencing `reference`.
    pub fn from_template(t: &Template, reference: Option<ObjectRef>) -> Self {
        let mut sheet = Sheet::new("Sheet1", t.format);
        sheet.reference = reference;
        Self {
            template: TemplateInfo {
                name: t.name.clone(),
                source: t.source,
            },
            projection: t.projection,
            units: t.units,
            style: t.style(),
            locked: false,
            title: TitleProps::default(),
            sheets: vec![sheet],
            sources: Vec::new(),
        }
    }

    /// The state the views of Part Studio `element` show, if the drawing has one.
    pub fn source(&self, element: Uuid) -> Option<&ModelSource> {
        self.sources.iter().find(|s| s.element == element)
    }

    /// The dependency hash of the model state view `v` shows: its own, else its part's in the
    /// drawing's source.
    pub fn shown_hash(&self, v: &View) -> Option<u64> {
        v.source_hash.or_else(|| self.source(v.reference.element)?.hash_of(v.reference.part))
    }

    /// The views that are out of date (D13.2): `live` gives the workspace's dependency hash of a
    /// reference (`None` while it isn't known yet; those views count as current). A view without
    /// a source for its studio shows the workspace itself and is never out of date.
    pub fn out_of_date(&self, live: &dyn Fn(&ObjectRef) -> Option<u64>) -> Vec<ViewId> {
        self.sheets
            .iter()
            .flat_map(|s| s.views.iter())
            .filter(|v| {
                let Some(shown) = self.shown_hash(v) else {
                    return false;
                };
                live(&v.reference).is_some_and(|h| h != shown)
            })
            .map(|v| v.id)
            .collect()
    }

    /// The BOM tables that are out of date (P3C.5, D14.8): their assembly's dependency hash in
    /// the workspace (`live`, `None` while unknown) differs from the one their rows show.
    pub fn stale_boms(&self, live: &dyn Fn(&ObjectRef) -> Option<u64>) -> Vec<(SheetId, TableId)> {
        self.sheets
            .iter()
            .flat_map(|s| s.tables.iter().map(move |t| (s.id, t)))
            .filter(|(_, t)| {
                t.bom.as_ref().is_some_and(|b| live(&ObjectRef { element: b.assembly, part: None }).is_some_and(|h| h != b.source_hash))
            })
            .map(|(s, t)| (s, t.id))
            .collect()
    }

    pub fn sheet(&self, id: SheetId) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.id == id)
    }

    pub fn sheet_index(&self, id: SheetId) -> Option<usize> {
        self.sheets.iter().position(|s| s.id == id)
    }

    /// The view with this id and the index of its sheet.
    pub fn view(&self, id: ViewId) -> Option<(usize, &View)> {
        self.sheets
            .iter()
            .enumerate()
            .find_map(|(i, s)| s.view(id).map(|v| (i, v)))
    }

    fn view_mut(&mut self, id: ViewId) -> Option<&mut View> {
        self.sheets.iter_mut().flat_map(|s| s.views.iter_mut()).find(|v| v.id == id)
    }

    /// Every view whose parent is `id`.
    pub fn children(&self, id: ViewId) -> Vec<ViewId> {
        self.sheets
            .iter()
            .flat_map(|s| s.views.iter())
            .filter(|v| v.parent == Some(id))
            .map(|v| v.id)
            .collect()
    }

    /// The scale a view shows at: its own, or its parent's when inherited (D4.7).
    pub fn effective_scale(&self, id: ViewId) -> Option<Scale> {
        let mut seen = 0;
        let mut v = self.view(id)?.1;
        while v.scale_inherited && seen < 64 {
            match v.parent.and_then(|p| self.view(p)) {
                Some((_, p)) => v = p,
                None => break,
            }
            seen += 1;
        }
        Some(v.scale)
    }

    /// Where views go when `id` is dragged by `delta` (sheet mm), keeping alignment (D7.1,
    /// D7.2): an aligned view only slides along its fold line, and each aligned child follows
    /// its parent across its own fold line (recursively). Returns every moved view's new anchor.
    pub fn drag_view(&self, id: ViewId, delta: [f64; 2]) -> Vec<(ViewId, [f64; 2])> {
        let Some((sheet, v)) = self.view(id) else {
            return Vec::new();
        };
        let d = match self.aligned_parent(v, sheet) {
            Some(p) => {
                let n = v.fold_on_sheet(p.rotation).unwrap_or([0.0, 0.0]);
                let t = delta[0] * n[0] + delta[1] * n[1];
                [n[0] * t, n[1] * t]
            }
            None => delta,
        };
        let mut out = vec![(id, [v.anchor[0] + d[0], v.anchor[1] + d[1]])];
        self.carry_children(id, v.rotation, d, &mut out, 0);
        out
    }

    fn carry_children(&self, id: ViewId, rotation: f64, d: [f64; 2], out: &mut Vec<(ViewId, [f64; 2])>, depth: usize) {
        if depth > 32 {
            return;
        }
        let Some((sheet, _)) = self.view(id) else {
            return;
        };
        for c in &self.sheets[sheet].views {
            if c.parent != Some(id) || !c.aligned() || out.iter().any(|(o, _)| *o == c.id) {
                continue;
            }
            let n = c.fold_on_sheet(rotation).unwrap_or([0.0, 0.0]);
            let t = d[0] * n[0] + d[1] * n[1];
            let dc = [d[0] - n[0] * t, d[1] - n[1] * t];
            out.push((c.id, [c.anchor[0] + dc[0], c.anchor[1] + dc[1]]));
            self.carry_children(c.id, c.rotation, dc, out, depth + 1);
        }
    }

    /// The parent `v` is kept aligned with: on the same sheet, alignment not suppressed.
    pub fn aligned_parent(&self, v: &View, sheet: usize) -> Option<&View> {
        if !v.aligned() {
            return None;
        }
        self.sheets[sheet].view(v.parent?)
    }

    /// The next free default sheet name ("Sheet2"): one more than the highest number in use.
    pub fn next_sheet_name(&self) -> String {
        let max = self
            .sheets
            .iter()
            .filter_map(|s| s.name.strip_prefix("Sheet")?.trim().parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("Sheet{}", max + 1)
    }

    /// Applies an edit. On error the drawing may be half-edited; the command layer restores it.
    pub fn apply(&mut self, op: &DrawingOp) -> Result<(), String> {
        match op {
            DrawingOp::InsertSheet { id, after } => {
                if self.sheet(*id).is_some() {
                    return Err("sheet id already in use".into());
                }
                // A new sheet copies the format, scale and reference of the one it follows
                // (or the last), like Onshape's Insert sheet.
                let at = after
                    .and_then(|a| self.sheet_index(a))
                    .unwrap_or(self.sheets.len().saturating_sub(1));
                let mut sheet = match self.sheets.get(at) {
                    Some(s) => {
                        let mut n = Sheet::new(self.next_sheet_name(), s.format);
                        n.scale = s.scale;
                        n.border = s.border;
                        n.zones = s.zones;
                        n.title_block = s.title_block;
                        n.reference = s.reference;
                        n
                    }
                    None => Sheet::new(self.next_sheet_name(), SheetFormat::new(SheetSize::AnsiA, Orientation::Landscape)),
                };
                sheet.id = *id;
                let i = if self.sheets.is_empty() { 0 } else { at + 1 };
                self.sheets.insert(i, sheet);
            }
            DrawingOp::DeleteSheet { id } => {
                let i = self.sheet_index(*id).ok_or("no such sheet")?;
                if self.sheets.len() <= 1 {
                    return Err("a drawing needs at least one sheet".into());
                }
                self.sheets.remove(i);
            }
            DrawingOp::RenameSheet { id, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err("name must not be empty".into());
                }
                let i = self.sheet_index(*id).ok_or("no such sheet")?;
                self.sheets[i].name = name.to_string();
            }
            DrawingOp::SetSheetProps { id, props } => {
                let i = self.sheet_index(*id).ok_or("no such sheet")?;
                let s = &mut self.sheets[i];
                s.format = props.format;
                s.scale = props.scale;
                s.border = props.border;
                s.zones = props.zones;
                s.title_block = props.title_block;
                s.reference = props.reference;
            }
            DrawingOp::InsertView { sheet, view } => {
                if self.view(view.id).is_some() {
                    return Err("view id already in use".into());
                }
                let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                let s = &mut self.sheets[i];
                // The sheet scale and reference come from the first view on it (D4.2, D4.7).
                if s.views.is_empty() {
                    s.scale = view.scale;
                    s.reference = Some(view.reference);
                }
                s.views.push(view.clone());
            }
            DrawingOp::DeleteViews { ids } => {
                for id in ids {
                    if self.view(*id).is_none() {
                        return Err("no such view".into());
                    }
                }
                for s in &mut self.sheets {
                    s.views.retain(|v| !ids.contains(&v.id));
                }
                // Children of a deleted view keep their place but lose their parent.
                for v in self.sheets.iter_mut().flat_map(|s| s.views.iter_mut()) {
                    if v.parent.is_some_and(|p| ids.contains(&p)) {
                        v.parent = None;
                        v.fold = None;
                        v.scale_inherited = false;
                    }
                }
            }
            DrawingOp::MoveViews { moves } => {
                for (id, anchor) in moves {
                    let v = self.view_mut(*id).ok_or("no such view")?;
                    v.anchor = *anchor;
                }
            }
            DrawingOp::SetView { view, .. } => {
                let old_scale = self.effective_scale(view.id);
                let v = self.view_mut(view.id).ok_or("no such view")?;
                *v = view.clone();
                // A view whose scale changed carries its inheriting children along.
                if old_scale != self.effective_scale(view.id) {
                    let mut todo = self.children(view.id);
                    let mut seen = 0;
                    while let Some(c) = todo.pop() {
                        seen += 1;
                        if seen > 256 {
                            break;
                        }
                        let parent_scale = self
                            .view(c)
                            .and_then(|(_, v)| v.parent)
                            .and_then(|p| self.effective_scale(p));
                        if let (Some(ps), Some(v)) = (parent_scale, self.view_mut(c))
                            && v.scale_inherited
                        {
                            v.scale = ps;
                            todo.extend(self.children(c));
                        }
                    }
                }
                // The sheet scale follows its first view (D4.7).
                if let Some((si, _)) = self.view(view.id) {
                    let s = &mut self.sheets[si];
                    if s.views.first().is_some_and(|f| f.id == view.id) {
                        s.scale = view.scale;
                    }
                }
            }
            DrawingOp::MoveViewToSheet { id, sheet } => {
                let to = self.sheet_index(*sheet).ok_or("no such sheet")?;
                let (from, _) = self.view(*id).ok_or("no such view")?;
                if from == to {
                    return Ok(());
                }
                let k = self.sheets[from].views.iter().position(|v| v.id == *id).ok_or("no such view")?;
                // The view goes (with its annotations, P3C.3); its children stay.
                let v = self.sheets[from].views.remove(k);
                let dest = &mut self.sheets[to];
                if dest.views.is_empty() {
                    dest.scale = v.scale;
                    dest.reference = Some(v.reference);
                }
                dest.views.push(v);
            }
            DrawingOp::AddAnnotation { view, annotation } => {
                let v = self.view_mut(*view).ok_or("no such view")?;
                if v.annotations.iter().any(|a| a.id == annotation.id) {
                    return Err("annotation id already in use".into());
                }
                v.annotations.push(annotation.clone());
            }
            DrawingOp::SetAnnotation { view, annotation, .. } => {
                let v = self.view_mut(*view).ok_or("no such view")?;
                let a = v
                    .annotations
                    .iter_mut()
                    .find(|a| a.id == annotation.id)
                    .ok_or("no such annotation")?;
                *a = annotation.clone();
            }
            DrawingOp::DeleteAnnotations { ids } => {
                for (view, id) in ids {
                    let v = self.view_mut(*view).ok_or("no such view")?;
                    let n = v.annotations.len();
                    v.annotations.retain(|a| a.id != *id);
                    if v.annotations.len() == n {
                        return Err("no such annotation".into());
                    }
                }
            }
            DrawingOp::AddNote { sheet, note } => {
                let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                if self.sheets[i].notes.iter().any(|n| n.id == note.id) {
                    return Err("note id already in use".into());
                }
                self.sheets[i].notes.push(note.clone());
            }
            DrawingOp::SetNote { sheet, note, .. } => {
                let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                let n = self.sheets[i].notes.iter_mut().find(|n| n.id == note.id).ok_or("no such note")?;
                *n = note.clone();
            }
            DrawingOp::AddTable { sheet, table } => {
                let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                if self.sheets[i].tables.iter().any(|t| t.id == table.id) {
                    return Err("table id already in use".into());
                }
                self.sheets[i].tables.push(table.clone());
            }
            DrawingOp::SetTable { sheet, table, .. } => {
                let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                let t = self.sheets[i].tables.iter_mut().find(|t| t.id == table.id).ok_or("no such table")?;
                *t = table.clone();
            }
            DrawingOp::DeleteSheetItems { sheet, notes, tables } => {
                let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                let s = &mut self.sheets[i];
                let (n0, t0) = (s.notes.len(), s.tables.len());
                s.notes.retain(|n| !notes.contains(&n.id));
                s.tables.retain(|t| !tables.contains(&t.id));
                if n0 - s.notes.len() != notes.len() || t0 - s.tables.len() != tables.len() {
                    return Err("no such note or table".into());
                }
            }
            DrawingOp::AddSketchItems { sheet, items } => {
                let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                let s = &mut self.sheets[i];
                for it in items {
                    if s.sketch.iter().any(|x| x.id == it.id) {
                        return Err("sketch item id already in use".into());
                    }
                    if let sheet_sketch::ItemKind::Spline { points } = &it.kind
                        && points.len() < 2
                    {
                        return Err("a spline needs two points".into());
                    }
                    s.sketch.push(it.clone());
                }
            }
            DrawingOp::SetSketchItems { sheet, items, .. } => {
                let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                for it in items {
                    let x = self.sheets[i].sketch.iter_mut().find(|x| x.id == it.id).ok_or("no such sketch item")?;
                    *x = it.clone();
                }
            }
            DrawingOp::DeleteSketchItems { sheet, ids } => {
                let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                let s = &mut self.sheets[i];
                let n = s.sketch.len();
                s.sketch.retain(|x| !ids.contains(&x.id));
                if n - s.sketch.len() != ids.len() {
                    return Err("no such sketch item".into());
                }
            }
            DrawingOp::SetStyle(style) => {
                if self.locked {
                    return Err("drawing properties are locked".into());
                }
                self.style = style.clone();
            }
            DrawingOp::SetLocked(l) => self.locked = *l,
            DrawingOp::SetSource(src) => {
                match self.sources.iter_mut().find(|s| s.element == src.element) {
                    // A new state of the same source keeps its pin (P3G.2).
                    Some(s) => {
                        let pinned = s.pinned;
                        *s = src.clone();
                        s.pinned |= pinned;
                    }
                    None => self.sources.push(src.clone()),
                }
            }
            DrawingOp::Batch { ops, .. } => {
                for op in ops {
                    self.apply(op)?;
                }
            }
            DrawingOp::Update { sources, views, notes } => {
                for src in sources {
                    self.apply(&DrawingOp::SetSource(src.clone()))?;
                }
                for u in views {
                    let v = self.view_mut(u.id).ok_or("no such view")?;
                    v.source_hash = u.source_hash;
                    v.annotations = u.annotations.clone();
                }
                for (sheet, note) in notes {
                    let i = self.sheet_index(*sheet).ok_or("no such sheet")?;
                    let n = self.sheets[i].notes.iter_mut().find(|n| n.id == note.id).ok_or("no such note")?;
                    *n = note.clone();
                }
            }
            DrawingOp::UpdateFromTemplate(t) => {
                if self.locked {
                    return Err("drawing properties are locked".into());
                }
                self.style = t.style();
                self.units = t.units;
                self.projection = t.projection;
                self.template = TemplateInfo {
                    name: t.name.clone(),
                    source: t.source,
                };
            }
        }
        Ok(())
    }
}

/// An edit of a drawing.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawingOp {
    /// Adds a sheet after `after` (or at the end), named "SheetN", with its format.
    InsertSheet { id: SheetId, after: Option<SheetId> },
    DeleteSheet { id: SheetId },
    RenameSheet { id: SheetId, name: String },
    SetSheetProps { id: SheetId, props: SheetProps },
    SetStyle(DrawingStyle),
    SetLocked(bool),
    /// "Update properties from a template…": the template's drawing properties, units and
    /// projection.
    UpdateFromTemplate(Template),
    /// Places a view on a sheet (the first view sets the sheet's scale and reference).
    InsertView { sheet: SheetId, view: View },
    /// Deletes views; their children stay, unaligned.
    DeleteViews { ids: Vec<ViewId> },
    /// Moves views (their new anchors).
    MoveViews { moves: Vec<(ViewId, [f64; 2])> },
    /// Replaces a view's settings (display, scale, alignment, sketches, rotation…); `label` is
    /// the undo label ("Show hidden lines").
    SetView { view: View, label: String },
    /// Move to sheet… (D7.3): the view goes to `sheet`, its children stay.
    MoveViewToSheet { id: ViewId, sheet: SheetId },
    /// Adds an annotation to a view (P3C.3).
    AddAnnotation { view: ViewId, annotation: annotation::Annotation },
    /// Replaces an annotation (moved text, new attachment, palette, prefix); `label` is the
    /// undo label.
    SetAnnotation { view: ViewId, annotation: annotation::Annotation, label: String },
    DeleteAnnotations { ids: Vec<(ViewId, annotation::AnnotationId)> },
    /// Places a note on a sheet (P3C.4).
    AddNote { sheet: SheetId, note: Note },
    /// Replaces a note (edited text, moved, turned, resized, a leader added); `label` is the undo
    /// label.
    SetNote { sheet: SheetId, note: Note, label: String },
    /// Places a table on a sheet (P3C.4).
    AddTable { sheet: SheetId, table: Table },
    /// Replaces a table (a cell's text, rows and columns, merges, size, place, fixed corner).
    SetTable { sheet: SheetId, table: Table, label: String },
    /// Deletes notes and tables of a sheet.
    DeleteSheetItems { sheet: SheetId, notes: Vec<NoteId>, tables: Vec<TableId> },
    /// Adds sheet sketch items (P3C.7: lines, splines, an inserted DXF, an image).
    AddSketchItems { sheet: SheetId, items: Vec<sheet_sketch::SketchItem> },
    /// Replaces a sheet sketch item (moved, a grip dragged); `label` is the undo label.
    SetSketchItems { sheet: SheetId, items: Vec<sheet_sketch::SketchItem>, label: String },
    DeleteSketchItems { sheet: SheetId, ids: Vec<sheet_sketch::ItemId> },
    /// Records (or replaces) the state of a referenced Part Studio (P3C.6): the first view of a
    /// studio records the workspace as it is.
    SetSource(ModelSource),
    /// Several edits as one (a view inserted together with its studio's state).
    Batch { ops: Vec<DrawingOp>, label: String },
    /// "Update from this workspace" (D13.2): the studios' new states, the out-of-date views'
    /// new hashes and refreshed annotations, and the notes whose leaders were refreshed.
    Update { sources: Vec<ModelSource>, views: Vec<ViewUpdate>, notes: Vec<(SheetId, Note)> },
}

impl DrawingOp {
    /// The undo label.
    pub fn label(&self) -> String {
        match self {
            DrawingOp::InsertSheet { .. } => "Insert sheet".into(),
            DrawingOp::DeleteSheet { .. } => "Delete sheet".into(),
            DrawingOp::RenameSheet { name, .. } => format!("Rename sheet to {}", name.trim()),
            DrawingOp::SetSheetProps { .. } => "Edit sheet properties".into(),
            DrawingOp::SetStyle(_) => "Edit drawing properties".into(),
            DrawingOp::SetLocked(true) => "Lock drawing properties".into(),
            DrawingOp::SetLocked(false) => "Unlock drawing properties".into(),
            DrawingOp::UpdateFromTemplate(_) => "Update properties from template".into(),
            DrawingOp::InsertView { view, .. } => format!("Insert {} view", view.name.trim_end_matches(" view")),
            DrawingOp::DeleteViews { ids } if ids.len() == 1 => "Delete view".into(),
            DrawingOp::DeleteViews { .. } => "Delete views".into(),
            DrawingOp::MoveViews { .. } => "Move view".into(),
            DrawingOp::SetView { label, .. } => label.clone(),
            DrawingOp::MoveViewToSheet { .. } => "Move view to sheet".into(),
            DrawingOp::AddAnnotation { annotation, .. } => format!("Insert {}", annotation.noun()),
            DrawingOp::SetAnnotation { label, .. } => label.clone(),
            DrawingOp::DeleteAnnotations { ids } if ids.len() == 1 => "Delete annotation".into(),
            DrawingOp::DeleteAnnotations { .. } => "Delete annotations".into(),
            DrawingOp::AddNote { .. } => "Insert note".into(),
            DrawingOp::SetNote { label, .. } | DrawingOp::SetTable { label, .. } => label.clone(),
            DrawingOp::AddTable { .. } => "Insert table".into(),
            DrawingOp::SetSource(_) => "Record the workspace".into(),
            DrawingOp::AddSketchItems { items, .. } => match items.as_slice() {
                [one] => format!("Insert {}", one.noun()),
                _ => "Insert sketch entities".into(),
            },
            DrawingOp::SetSketchItems { label, .. } => label.clone(),
            DrawingOp::DeleteSketchItems { ids, .. } if ids.len() == 1 => "Delete sketch entity".into(),
            DrawingOp::DeleteSketchItems { .. } => "Delete sketch entities".into(),
            DrawingOp::Batch { label, .. } => label.clone(),
            DrawingOp::Update { .. } => "Update from this workspace".into(),
            DrawingOp::DeleteSheetItems { notes, tables, .. } => match (notes.len(), tables.len()) {
                (1, 0) => "Delete note".into(),
                (0, 1) => "Delete table".into(),
                _ => "Delete notes and tables".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ansi_a() -> Drawing {
        Drawing::from_template(&template::builtin("ANSI_A_INCH.dwt").unwrap(), None)
    }

    #[test]
    fn new_drawing_has_one_sheet() {
        let d = ansi_a();
        assert_eq!(d.sheets.len(), 1);
        assert_eq!(d.sheets[0].name, "Sheet1");
        assert_eq!(d.units, DrawingUnits::Inch);
        assert_eq!(d.projection, Projection::Third);
        let (w, h) = d.sheets[0].size_mm();
        assert!((w - 279.4).abs() < 1e-9 && (h - 215.9).abs() < 1e-9);
    }

    #[test]
    fn sheets_insert_rename_delete() {
        let mut d = ansi_a();
        let first = d.sheets[0].id;
        let a = SheetId::new();
        d.apply(&DrawingOp::InsertSheet { id: a, after: Some(first) }).unwrap();
        assert_eq!(d.sheets[1].name, "Sheet2");
        assert_eq!(d.sheets[1].format, d.sheets[0].format);
        d.apply(&DrawingOp::RenameSheet { id: a, name: " Handle ".into() }).unwrap();
        assert_eq!(d.sheets[1].name, "Handle");
        assert!(d.apply(&DrawingOp::RenameSheet { id: a, name: " ".into() }).is_err());
        let b = SheetId::new();
        d.apply(&DrawingOp::InsertSheet { id: b, after: None }).unwrap();
        assert_eq!(d.sheets[2].name, "Sheet2", "Sheet2 is free again after the rename");
        d.apply(&DrawingOp::DeleteSheet { id: first }).unwrap();
        d.apply(&DrawingOp::DeleteSheet { id: a }).unwrap();
        assert!(d.apply(&DrawingOp::DeleteSheet { id: b }).is_err(), "keeps the last sheet");
    }

    #[test]
    fn locked_properties_refuse_edits() {
        let mut d = ansi_a();
        d.apply(&DrawingOp::SetLocked(true)).unwrap();
        let mut s = d.style.clone();
        s.precision = 1;
        assert!(d.apply(&DrawingOp::SetStyle(s.clone())).is_err());
        d.apply(&DrawingOp::SetLocked(false)).unwrap();
        d.apply(&DrawingOp::SetStyle(s)).unwrap();
        assert_eq!(d.style.precision, 1);
        let iso = template::builtin("ISO_A3_MM.dwt").unwrap();
        d.apply(&DrawingOp::UpdateFromTemplate(iso)).unwrap();
        assert_eq!(d.style.precision, 2);
        assert_eq!(d.units, DrawingUnits::Millimeter);
        assert_eq!(d.projection, Projection::First);
    }

    #[test]
    fn sheet_graphics_show_zones_and_title_block() {
        let d = ansi_a();
        let g = sheet_graphics(&d, 0, &ReferenceProps::default());
        let s = g.strings();
        for label in ["1", "2", "A", "B", "1:1", "1 OF 1", "THIRD ANGLE", "DO NOT SCALE DRAWING"] {
            assert!(s.contains(&label), "{label} missing from {s:?}");
        }
        assert!(s.contains(&title_block::DASHES));
        assert_eq!(g.circles.len(), 2, "the projection symbol's end view");
        // Zones and border off: only the title block remains.
        let mut d2 = d.clone();
        let mut p = d2.sheets[0].props();
        p.border = false;
        d2.apply(&DrawingOp::SetSheetProps { id: d2.sheets[0].id, props: p }).unwrap();
        let g2 = sheet_graphics(&d2, 0, &ReferenceProps::default());
        assert!(!g2.strings().contains(&"B"));
    }

    #[test]
    fn views_align_move_and_change_sheet() {
        use crate::view::{Placement, projected_view};
        let mut d = ansi_a();
        let sheet = d.sheets[0].id;
        let part = ObjectRef {
            element: Uuid::from_u128(7),
            part: Some((Uuid::from_u128(9), 0)),
        };
        // The first view sets the sheet's scale and reference (D4.2, D4.7).
        let front = View::base(part, NamedView::Front, Scale::new(1, 2), [80.0, 60.0]);
        d.apply(&DrawingOp::InsertView { sheet, view: front.clone() }).unwrap();
        assert_eq!(d.sheets[0].scale, Scale::new(1, 2));
        assert_eq!(d.sheets[0].reference, Some(part));
        let top = projected_view(&front, Placement::Ortho([0.0, 1.0]), d.projection, [85.0, 140.0], None);
        let right = projected_view(&front, Placement::Ortho([1.0, 0.0]), d.projection, [170.0, 64.0], None);
        let iso = projected_view(&front, Placement::Iso(1.0, 1.0), d.projection, [200.0, 150.0], None);
        for v in [&top, &right, &iso] {
            d.apply(&DrawingOp::InsertView { sheet, view: v.clone() }).unwrap();
        }
        assert_eq!(top.anchor, [80.0, 140.0]);
        assert_eq!(right.anchor, [170.0, 60.0]);
        // Dragging Front carries Top sideways and Right up and down; the isometric view stays.
        let moves = d.drag_view(front.id, [-10.0, 5.0]);
        let at = |id: ViewId| moves.iter().find(|(v, _)| *v == id).map(|(_, a)| *a);
        assert_eq!(at(front.id), Some([70.0, 65.0]));
        assert_eq!(at(top.id), Some([70.0, 140.0]));
        assert_eq!(at(right.id), Some([170.0, 65.0]));
        assert_eq!(at(iso.id), None);
        // An aligned child slides only along its fold line.
        assert_eq!(d.drag_view(top.id, [7.0, 3.0]), vec![(top.id, [80.0, 143.0])]);
        // Suppressed, it moves freely.
        let mut free = top.clone();
        free.align_suppressed = true;
        d.apply(&DrawingOp::SetView { view: free, label: "Suppress".into() }).unwrap();
        assert_eq!(d.drag_view(top.id, [7.0, 3.0]), vec![(top.id, [87.0, 143.0])]);
        // A new scale on the parent reaches the children that inherit it (not the ones set).
        let mut iso_set = d.view(iso.id).unwrap().1.clone();
        iso_set.scale = Scale::new(1, 4);
        iso_set.scale_inherited = false;
        d.apply(&DrawingOp::SetView { view: iso_set, label: "Scale".into() }).unwrap();
        let mut f = d.view(front.id).unwrap().1.clone();
        f.scale = Scale::new(1, 1);
        d.apply(&DrawingOp::SetView { view: f, label: "Scale".into() }).unwrap();
        assert_eq!(d.view(right.id).unwrap().1.scale, Scale::new(1, 1));
        assert_eq!(d.view(iso.id).unwrap().1.scale, Scale::new(1, 4));
        assert_eq!(d.sheets[0].scale, Scale::new(1, 1), "the sheet scale follows its first view");
        // Move to sheet: the view goes, its children stay (D7.3).
        let second = SheetId::new();
        d.apply(&DrawingOp::InsertSheet { id: second, after: None }).unwrap();
        d.apply(&DrawingOp::MoveViewToSheet { id: front.id, sheet: second }).unwrap();
        assert_eq!(d.view(front.id).unwrap().0, 1);
        assert_eq!(d.view(right.id).unwrap().0, 0);
        // Children on another sheet aren't carried by drags any more.
        assert_eq!(d.drag_view(front.id, [1.0, 1.0]).len(), 1);
        // Delete: the children lose their parent.
        d.apply(&DrawingOp::DeleteViews { ids: vec![front.id] }).unwrap();
        assert!(d.view(right.id).unwrap().1.parent.is_none());
        let text = ron::to_string(&d).unwrap();
        let back: Drawing = ron::from_str(&text).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn annotations_are_edited_and_go_with_their_view() {
        use crate::annotation::{Annotation, AnnotationKind, EdgeRef, Shape};
        let mut d = ansi_a();
        let sheet = d.sheets[0].id;
        let part = ObjectRef { element: Uuid::from_u128(7), part: None };
        let front = View::base(part, NamedView::Front, Scale::new(1, 2), [80.0, 60.0]);
        d.apply(&DrawingOp::InsertView { sheet, view: front.clone() }).unwrap();
        let edge = EdgeRef { edge: None, face: None, shape: Shape::Circle { center: [0.0, 0.0], radius: 5.0, arc: None } };
        let mut a = Annotation::new(AnnotationKind::Centermark(edge));
        let op = DrawingOp::AddAnnotation { view: front.id, annotation: a.clone() };
        assert_eq!(op.label(), "Insert centermark");
        d.apply(&op).unwrap();
        assert!(d.apply(&op).is_err(), "the id is taken");
        a.kind = AnnotationKind::VirtualSharp { a: edge, b: edge };
        d.apply(&DrawingOp::SetAnnotation { view: front.id, annotation: a.clone(), label: "Edit".into() }).unwrap();
        assert_eq!(d.view(front.id).unwrap().1.annotations, vec![a.clone()]);
        // A projected view starts without its parent's annotations.
        let top = crate::view::projected_view(
            d.view(front.id).unwrap().1,
            crate::view::Placement::Ortho([0.0, 1.0]),
            d.projection,
            [80.0, 140.0],
            None,
        );
        assert!(top.annotations.is_empty());
        // Move to sheet: the annotations go with the view (D7.3).
        let second = SheetId::new();
        d.apply(&DrawingOp::InsertSheet { id: second, after: None }).unwrap();
        d.apply(&DrawingOp::MoveViewToSheet { id: front.id, sheet: second }).unwrap();
        assert_eq!(d.sheets[1].views[0].annotations, vec![a.clone()]);
        d.apply(&DrawingOp::DeleteAnnotations { ids: vec![(front.id, a.id)] }).unwrap();
        assert!(d.view(front.id).unwrap().1.annotations.is_empty());
        assert!(d.apply(&DrawingOp::DeleteAnnotations { ids: vec![(front.id, a.id)] }).is_err());
        let back: Drawing = ron::from_str(&ron::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn out_of_date_views_follow_their_parts_hash() {
        let mut d = ansi_a();
        let sheet = d.sheets[0].id;
        let el = Uuid::from_u128(7);
        let (pa, pb) = ((Uuid::from_u128(9), 0), (Uuid::from_u128(10), 0));
        let src = ModelSource {
            element: el,
            snapshot: "old".into(),
            parts: vec![PartHash { part: Some(pa), hash: 1 }, PartHash { part: Some(pb), hash: 2 }],
            assembly: None,
            pinned: false,
        };
        // The first view of a studio records its state with it, as one step.
        let mut va = View::base(ObjectRef { element: el, part: Some(pa) }, NamedView::Front, Scale::new(1, 1), [50.0, 50.0]);
        va.source_hash = src.hash_of(Some(pa));
        let insert = DrawingOp::InsertView { sheet, view: va.clone() };
        let label = insert.label();
        d.apply(&DrawingOp::Batch { ops: vec![DrawingOp::SetSource(src.clone()), insert], label }).unwrap();
        let vb = View::base(ObjectRef { element: el, part: Some(pb) }, NamedView::Top, Scale::new(1, 1), [150.0, 50.0]);
        d.apply(&DrawingOp::InsertView { sheet, view: vb.clone() }).unwrap();
        assert_eq!(d.shown_hash(&vb), Some(2), "a view without its own hash takes its part's in the source");
        // The workspace: part b changed, part a didn't; another studio is irrelevant.
        let live = |r: &ObjectRef| match r.part {
            Some(p) if p == pa => Some(1),
            Some(p) if p == pb => Some(3),
            _ => None,
        };
        assert_eq!(d.out_of_date(&live), vec![vb.id]);
        // Not known yet: nothing is out of date.
        assert!(d.out_of_date(&|_| None).is_empty());
        // Update: the new state, b's new hash.
        let new = ModelSource { snapshot: "new".into(), parts: vec![PartHash { part: Some(pa), hash: 1 }, PartHash { part: Some(pb), hash: 3 }], ..src };
        let op = update::update_op(&d, vec![new], &[]);
        d.apply(&op).unwrap();
        assert_eq!(d.source(el).unwrap().snapshot, "new");
        assert!(d.out_of_date(&live).is_empty());
        assert_eq!(op.label(), "Update from this workspace");
        let back: Drawing = ron::from_str(&ron::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn drawings_round_trip_through_ron() {
        let mut d = ansi_a();
        d.sheets[0].reference = Some(ObjectRef {
            element: Uuid::from_u128(7),
            part: Some((Uuid::from_u128(9), 0)),
        });
        let text = ron::to_string(&d).unwrap();
        let back: Drawing = ron::from_str(&text).unwrap();
        assert_eq!(back, d);
    }
}
