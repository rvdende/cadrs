//! Assembly drawings (P3C.5, D11, D12, D14.8, X10): what a drawing keeps of an assembly, its
//! **BOM tables** and its **callouts** (balloons).
//!
//! - **The assembly's state.** A view of an assembly shows every part occurrence at its
//!   placement. The drawing keeps the assembly as it was at the last update in a
//!   [`crate::ModelSource`] (like a Part Studio's, P3C.6): `cadrs_core` writes the snapshot it
//!   projects from, and [`AssemblyInfo`] beside it: each occurrence's id, name, origin and its
//!   part's properties, which callouts read.
//! - **BOM tables** (D11.1–D11.3) are ordinary [`Table`]s with a [`BomData`]: the assembly, the
//!   BOM type (Flattened, Structured top level or multi level), the order (top to bottom: the
//!   header on top, counting down; bottom to top: the header at the bottom, counting up) and
//!   the rows as the assembly's BOM computed them (its columns, the occurrences each row
//!   covers). [`bom_table`] lays them out; [`refreshed_bom_table`] puts new rows in a table
//!   keeping its place, fixed corner, text height and (when the columns stay) its column
//!   widths. Resizing and formatting work as for any table.
//! - **Callouts** ([`Callout`], D11.4–D11.6) are annotations of an assembly view attached to an
//!   occurrence: a leader from a point on the part to a border (circle, underline, box,
//!   triangle or none) with five text fields (upper, lower, left, right, centre). A field is
//!   literal text with property tokens: `{Part: Name}` (any part property the drawing keeps)
//!   and `{Table: Qty.}`, `{Table: Item No.}` (the row of a BOM table on the sheet that holds
//!   the occurrence, [`resolve_field`]). Callouts are laid out on the sheet in the view's
//!   frame, so they follow the view; when their occurrence is gone after an update they dangle
//!   red with their last text ([`refresh_callout`]). [`inference`] lines a callout being
//!   dragged up with the others.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::annotation::{AnnGraphics, GripKind, PlacedText, ViewModel, text_width};
use crate::rich::{P2, RichText};
use crate::style::DrawingStyle;
use crate::table::{Corner, Table, TableId};
use crate::view::View;

// ---------------------------------------------------------------------------------------------
// The assembly's state

/// One part occurrence of an assembly, as its drawing keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OccurrenceInfo {
    /// The occurrence's id in the assembly (an instance's, or derived for a part inside a
    /// subassembly).
    pub id: Uuid,
    /// Its instance name, "Universal Joint Flange <1>".
    pub name: String,
    /// Where its origin is (assembly mm): what a callout's leader follows on update.
    pub origin: [f64; 3],
    /// Its part's properties by label ("Name", "Part number", "Description", "Material", …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<(String, String)>,
}

impl OccurrenceInfo {
    /// A property's value, if the part has it.
    pub fn prop(&self, label: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k.eq_ignore_ascii_case(label)).map(|(_, v)| v.as_str())
    }
}

/// An assembly as its drawing keeps it: its occurrences.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssemblyInfo {
    pub occurrences: Vec<OccurrenceInfo>,
}

impl AssemblyInfo {
    pub fn occurrence(&self, id: &Uuid) -> Option<&OccurrenceInfo> {
        self.occurrences.iter().find(|o| o.id == *id)
    }
}

/// The part properties a callout's `Part:` menu offers (the ones the drawing keeps).
pub const PART_PROPERTIES: [&str; 6] = ["Name", "Part number", "Description", "Material", "Revision", "Vendor"];

/// The BOM table properties a callout's `Table:` menu offers.
pub const TABLE_PROPERTIES: [&str; 2] = ["Item No.", "Qty."];

// ---------------------------------------------------------------------------------------------
// BOM tables

/// The Insert BOM dialog's **BOM type** (D11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BomType {
    /// Parts only, at any depth.
    #[default]
    Flattened,
    /// The top level's items, subassemblies as rows.
    TopLevel,
    /// Subassemblies and their parts, numbered 2.1, 2.2, …
    MultiLevel,
}

impl BomType {
    pub const ALL: [BomType; 3] = [BomType::Flattened, BomType::TopLevel, BomType::MultiLevel];

    pub fn label(self) -> &'static str {
        match self {
            BomType::Flattened => "Flattened",
            BomType::TopLevel => "Structured - Top level",
            BomType::MultiLevel => "Structured - Multi level",
        }
    }
}

/// The Insert BOM dialog's **Order** (D11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BomOrder {
    /// The header on top, items counting down.
    #[default]
    TopToBottom,
    /// The header at the bottom, items counting up (the usual style above a title block).
    BottomToTop,
}

impl BomOrder {
    pub const ALL: [BomOrder; 2] = [BomOrder::TopToBottom, BomOrder::BottomToTop];

    pub fn label(self) -> &'static str {
        match self {
            BomOrder::TopToBottom => "Top to bottom",
            BomOrder::BottomToTop => "Bottom to top",
        }
    }
}

/// One row of a BOM table as the assembly's BOM computed it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BomRowData {
    /// Its item number ("2", "2.1").
    pub item: String,
    #[serde(default)]
    pub depth: usize,
    pub quantity: u32,
    /// The occurrences it covers (what callouts look their rows up by).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occurrences: Vec<Uuid>,
    /// Which part or assembly it is (as `cadrs_core` names it; tests read it).
    #[serde(default)]
    pub owner: String,
    /// One cell per column.
    pub cells: Vec<String>,
}

/// A BOM table's source (D11.1, D11.2): the assembly, the type and order, and the rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BomData {
    /// The assembly (a document element's UUID).
    pub assembly: Uuid,
    #[serde(default)]
    pub kind: BomType,
    #[serde(default)]
    pub order: BomOrder,
    /// The column headers, as the assembly's BOM has them ("Item No.", "Name", "Quantity", …).
    pub columns: Vec<String>,
    pub rows: Vec<BomRowData>,
    /// The assembly's dependency hash the rows were computed at (P3C.6): the table is out of
    /// date when the workspace's differs.
    #[serde(default)]
    pub source_hash: u64,
}

impl BomData {
    /// The row that covers occurrence `id`.
    pub fn row_of(&self, id: &Uuid) -> Option<&BomRowData> {
        self.rows.iter().find(|r| r.occurrences.contains(id))
    }

    /// A column's index by its header (Qty. and Quantity, Item and Item No. are the same).
    pub fn column(&self, label: &str) -> Option<usize> {
        let norm = |s: &str| {
            let s = s.trim().trim_end_matches('.').to_lowercase();
            match s.as_str() {
                "qty" => "quantity".to_string(),
                "item" | "item no" => "item no".to_string(),
                _ => s,
            }
        };
        let want = norm(label);
        self.columns.iter().position(|c| norm(c) == want)
    }

    /// Table property `label` of the row covering `id` ("Item No.", "Qty.", or a column's
    /// header).
    pub fn value(&self, id: &Uuid, label: &str) -> Option<String> {
        let row = self.row_of(id)?;
        let l = label.trim().trim_end_matches('.').to_lowercase();
        match l.as_str() {
            "qty" | "quantity" => Some(row.quantity.to_string()),
            "item" | "item no" => Some(row.item.clone()),
            _ => self.column(label).and_then(|c| row.cells.get(c).cloned()),
        }
    }
}

/// A column's width (mm) for its texts at cap height `h`: sized to its content on one line
/// (the header too, bold), padded; a text longer than 30 text heights wraps, between words
/// only, so the column is never narrower than its longest word.
fn column_width(texts: &[&str], h: f64, header: &str) -> f64 {
    let bold = crate::rich::CharStyle { bold: true, ..Default::default() };
    let w = texts.iter().map(|t| text_width(t.trim())).fold(0.0, f64::max) * h;
    let hw = crate::rich::width(header.trim(), &bold) * h;
    let word = texts
        .iter()
        .flat_map(|t| t.split_whitespace())
        .map(text_width)
        .chain(header.split_whitespace().map(|w| crate::rich::width(w, &bold)))
        .fold(0.0, f64::max)
        * h;
    let pad = 1.2 * h;
    (w.max(hw).min(30.0 * h).max(word) + pad).max(4.0 * h)
}

/// Lays out a BOM table's cells from `data` (header row, then the rows; reversed with the
/// header at the bottom for bottom-to-top).
fn fill(t: &mut Table, data: &BomData, row_h: f64) {
    let n = data.rows.len() + 1;
    let cols = data.columns.len().max(1);
    let mut cells: Vec<Vec<RichText>> = Vec::with_capacity(n);
    let header: Vec<RichText> = (0..cols).map(|c| RichText::plain(data.columns.get(c).map(String::as_str).unwrap_or(""))).collect();
    let body: Vec<Vec<RichText>> = data
        .rows
        .iter()
        .map(|r| (0..cols).map(|c| RichText::plain(r.cells.get(c).map(String::as_str).unwrap_or(""))).collect())
        .collect();
    match data.order {
        BomOrder::TopToBottom => {
            cells.push(header);
            cells.extend(body);
        }
        BomOrder::BottomToTop => {
            cells.extend(body.into_iter().rev());
            cells.push(header);
        }
    }
    t.cells = cells;
    t.rows = vec![row_h; n];
    t.grown.clear();
    t.merges.clear();
    t.title_row = false;
    t.header_row = true;
    t.header_last = data.order == BomOrder::BottomToTop;
}

/// The widths the columns of `data` want at cap height `h`.
pub fn column_widths(data: &BomData, h: f64) -> Vec<f64> {
    (0..data.columns.len().max(1))
        .map(|c| {
            let texts: Vec<&str> = data.rows.iter().map(|r| r.cells.get(c).map(String::as_str).unwrap_or("")).collect();
            column_width(&texts, h, data.columns.get(c).map(String::as_str).unwrap_or(""))
        })
        .collect()
}

/// A new BOM table of `data`, its fixed corner `fixed` at `at` (D11.2).
pub fn bom_table(data: BomData, fixed: Corner, at: P2, style: &DrawingStyle) -> Table {
    let h = style.table_text_height;
    let mut t = Table::new(1, data.columns.len().max(1), false, true, fixed, at, style);
    t.id = TableId::new();
    t.cols = column_widths(&data, h);
    fill(&mut t, &data, style.table_row_height);
    t.bom = Some(data);
    t
}

/// Table `t` with the rows of `data` (an update, or BOM Table properties): its place, fixed
/// corner and text height stay, and its column widths when the columns are the same.
pub fn refreshed_bom_table(t: &Table, data: BomData, style: &DrawingStyle) -> Table {
    let mut out = t.clone();
    let same_columns = t.bom.as_ref().is_some_and(|b| b.columns == data.columns) && t.cols.len() == data.columns.len();
    if !same_columns {
        out.cols = column_widths(&data, t.text_height);
    }
    // The body rows keep the height the table's rows have now (a resized table stays resized).
    let row_h = (0..t.n_rows()).map(|r| t.natural_row(r)).fold(f64::MAX, f64::min);
    let row_h = if row_h.is_finite() && row_h > 0.0 { row_h } else { style.table_row_height };
    fill(&mut out, &data, row_h);
    out.bom = Some(data);
    out
}

/// Snaps a table's fixed corner to the sheet border's same corner when it is within `tol`
/// (D11.2: "Click the sheet to place the table; it snaps to the border corner").
pub fn snap_to_border(at: P2, corner: Corner, border: (P2, P2), tol: f64) -> Option<P2> {
    let (lo, hi) = border;
    let c = match corner {
        Corner::TopLeft => [lo[0], hi[1]],
        Corner::TopRight => [hi[0], hi[1]],
        Corner::BottomLeft => [lo[0], lo[1]],
        Corner::BottomRight => [hi[0], lo[1]],
    };
    ((at[0] - c[0]).hypot(at[1] - c[1]) <= tol).then_some(c)
}

/// Snaps a table's fixed corner to the title block `block` (P3E.5, TD10.5: the test drive's
/// Structured BOM "snapped at the title block"): a right-hand corner (the table to the left of
/// the block) to the block's left edge, at its bottom-left or top-left corner when within
/// `tol`, else onto the edge (x only) when level with it; a bottom corner to the block's top
/// edge's corner on its side (the table standing on the block).
pub fn snap_to_title_block(at: P2, corner: Corner, block: (P2, P2), tol: f64) -> Option<P2> {
    let (lo, hi) = block;
    let right = matches!(corner, Corner::BottomRight | Corner::TopRight);
    let mut points = Vec::new();
    if right {
        points.extend([[lo[0], lo[1]], [lo[0], hi[1]]]);
    }
    match corner {
        Corner::BottomRight => points.push([hi[0], hi[1]]),
        Corner::BottomLeft => points.push([lo[0], hi[1]]),
        _ => {}
    }
    let d = |p: &P2| (at[0] - p[0]).hypot(at[1] - p[1]);
    if let Some(p) = points.into_iter().filter(|p| d(p) <= tol).min_by(|a, b| d(a).total_cmp(&d(b))) {
        return Some(p);
    }
    (right && (at[0] - lo[0]).abs() <= tol && at[1] >= lo[1] && at[1] <= hi[1]).then_some([lo[0], at[1]])
}

/// Table `t` narrowed so it stays within the frame `border` across (P3E.5 judge: a six-column
/// BOM snapped at the title block ran past the frame's left edge): its free side moves in to
/// the frame, the columns sharing the width (none narrower than its longest word, so cells wrap
/// between words). A table that fits is returned as it is.
pub fn fit_within(t: &Table, border: (P2, P2)) -> Table {
    use crate::table::Side;
    let (lo, hi) = t.rect();
    let (side, edge, over) = if t.fixed.right() { (Side::Left, border.0[0], lo[0] < border.0[0] - 1e-9) } else { (Side::Right, border.1[0], hi[0] > border.1[0] + 1e-9) };
    if !over {
        return t.clone();
    }
    t.resize(side, [edge, t.at[1]]).unwrap_or_else(|_| t.clone())
}

/// Where a table's fixed corner snaps: the frame's corner ([`snap_to_border`]) or the title
/// block ([`snap_to_title_block`]), whichever is nearer.
pub fn snap_table_corner(at: P2, corner: Corner, border: (P2, P2), block: Option<(P2, P2)>, tol: f64) -> Option<P2> {
    let d = |p: &P2| (at[0] - p[0]).hypot(at[1] - p[1]);
    [snap_to_border(at, corner, border, tol), block.and_then(|b| snap_to_title_block(at, corner, b, tol))]
        .into_iter()
        .flatten()
        .min_by(|a, b| d(a).total_cmp(&d(b)))
}

// ---------------------------------------------------------------------------------------------
// Callouts

/// A callout's border (D11.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Border {
    #[default]
    Circle,
    Underline,
    Box,
    Triangle,
    None,
}

impl Border {
    pub const ALL: [Border; 5] = [Border::Circle, Border::Underline, Border::Box, Border::Triangle, Border::None];

    pub fn label(self) -> &'static str {
        match self {
            Border::Circle => "Circle",
            Border::Underline => "Underline",
            Border::Box => "Box",
            Border::Triangle => "Triangle",
            Border::None => "None",
        }
    }
}

/// A callout border's size: tight around its text, or wide enough for `n` characters.
pub const SIZES: [(u8, &str); 5] = [(0, "Tight Fit"), (1, "1 Character"), (2, "2 Characters"), (3, "3 Characters"), (4, "4 Characters")];

/// The five text fields (D11.4): literal text with `{Part: …}` and `{Table: …}` tokens.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CalloutFields {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub upper: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lower: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub left: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub right: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub center: String,
}

impl CalloutFields {
    /// Upper, lower, left, right, centre.
    pub fn all(&self) -> [&str; 5] {
        [&self.upper, &self.lower, &self.left, &self.right, &self.center]
    }
}

/// A callout (balloon) of an assembly view (D11.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Callout {
    /// The occurrence its leader points at.
    pub occurrence: Uuid,
    /// Where the leader ends on the part (view 2D, model mm).
    pub attach: P2,
    /// Where the callout sits (view 2D): a circle's or box's centre; an underline's end nearest
    /// the leader.
    pub text: P2,
    #[serde(default)]
    pub border: Border,
    /// 0: tight fit, else the characters the border fits.
    #[serde(default)]
    pub size: u8,
    /// Cap height (sheet mm).
    pub text_height: f64,
    pub fields: CalloutFields,
    /// The texts it showed when its occurrence went away (upper, lower, left, right, centre).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<[String; 5]>,
}

/// A token of a field.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldPart {
    Text(String),
    Part(String),
    Table(String),
}

/// A field's text split into literal text and `{Part: …}` / `{Table: …}` tokens.
pub fn parse_field(s: &str) -> Vec<FieldPart> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let Some(j) = rest[i..].find('}') else { break };
        let inner = &rest[i + 1..i + j];
        let tok = if let Some(p) = inner.strip_prefix("Part:") {
            Some(FieldPart::Part(p.trim().to_string()))
        } else {
            inner.strip_prefix("Table:").map(|t| FieldPart::Table(t.trim().to_string()))
        };
        match tok {
            Some(t) => {
                if i > 0 {
                    out.push(FieldPart::Text(rest[..i].to_string()));
                }
                out.push(t);
            }
            None => out.push(FieldPart::Text(rest[..i + j + 1].to_string())),
        }
        rest = &rest[i + j + 1..];
    }
    if !rest.is_empty() {
        out.push(FieldPart::Text(rest.to_string()));
    }
    out
}

/// The token a menu inserts: `{Part: Name}`, `{Table: Qty.}`.
pub fn token(part: bool, label: &str) -> String {
    format!("{{{}: {label}}}", if part { "Part" } else { "Table" })
}

/// A field's text for occurrence `occ` with the sheet's BOM tables `boms` (D11.4): a part
/// property the part doesn't have, or a table property without a BOM table that lists the
/// occurrence, reads empty (Onshape: "only the component properties are available then").
pub fn resolve_field(field: &str, occ: Option<&OccurrenceInfo>, boms: &[&BomData]) -> String {
    let mut out = String::new();
    for p in parse_field(field) {
        match p {
            FieldPart::Text(t) => out.push_str(&t),
            FieldPart::Part(k) => {
                if let Some(v) = occ.and_then(|o| o.prop(&k)) {
                    out.push_str(v);
                }
            }
            FieldPart::Table(k) => {
                if let Some(v) = occ.and_then(|o| boms.iter().find_map(|b| b.value(&o.id, &k))) {
                    out.push_str(&v);
                }
            }
        }
    }
    out.trim().to_string()
}

/// A callout's five texts (upper, lower, left, right, centre) in model `m`: its last ones when
/// its occurrence is gone (it dangles).
pub fn callout_texts(m: &dyn ViewModel, c: &Callout) -> [String; 5] {
    if callout_dangles(m, c) {
        return c.last.clone().unwrap_or_default();
    }
    let occ = m.occurrence(&c.occurrence);
    let boms = m.boms();
    c.fields.all().map(|f| resolve_field(f, occ, &boms))
}

/// Whether a callout dangles: its view shows an assembly that has no longer its occurrence.
pub fn callout_dangles(m: &dyn ViewModel, c: &Callout) -> bool {
    m.is_assembly() && m.occurrence(&c.occurrence).is_none()
}

/// The callout's border extent around its centre text of width `w` at cap height `h`: half
/// width and half height.
fn border_half(c: &Callout, w: f64, h: f64) -> (f64, f64) {
    let fit = if c.size == 0 { w } else { (c.size as f64 * 0.72 * h).max(w) };
    match c.border {
        Border::Circle => {
            let r = (fit / 2.0 + 0.55 * h).max(1.05 * h);
            (r, r)
        }
        Border::Box => (fit / 2.0 + 0.5 * h, 0.95 * h),
        Border::Triangle => (fit / 2.0 + 1.3 * h, 1.3 * h),
        Border::Underline | Border::None => (fit / 2.0, 0.6 * h),
    }
}

fn circle_pts(c: P2, r: f64) -> Vec<P2> {
    (0..=48)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / 48.0;
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        })
        .collect()
}

fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The nearest point to `p` on segment `a`–`b`.
fn nearest_on(a: P2, b: P2, p: P2) -> P2 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    if l2 < 1e-18 {
        return a;
    }
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
    [a[0] + d[0] * t, a[1] + d[1] * t]
}

/// Where the callout's centre text is centred (sheet mm), and the underline's ends, from its
/// sheet anchor `t`: an underline callout's anchor is the end of its line nearest the leader.
fn layout_center(c: &Callout, t: P2, attach: P2, w: f64, h: f64) -> (P2, Option<(P2, P2)>) {
    match c.border {
        Border::Underline | Border::None => {
            let (hw, _) = border_half(c, w, h);
            // The line runs away from the leader: right of the anchor when the part is to the
            // left.
            let right = attach[0] <= t[0];
            let (a, b) = if right { (t, [t[0] + 2.0 * hw + 0.4 * h, t[1]]) } else { ([t[0] - 2.0 * hw - 0.4 * h, t[1]], t) };
            let mid = [(a[0] + b[0]) / 2.0, t[1] + 0.85 * h];
            (mid, (c.border == Border::Underline).then_some((a, b)))
        }
        _ => (t, None),
    }
}

/// A callout's graphics on the sheet: its border, leader (a small arrow on the part), texts,
/// its boxes for picking and its text grip.
pub fn callout_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, c: &Callout) -> AnnGraphics {
    let mut g = AnnGraphics { dangling: callout_dangles(m, c), ..AnnGraphics::default() };
    let [upper, lower, left, right, center] = callout_texts(m, c);
    let h = c.text_height;
    let t = view.to_sheet(c.text);
    let attach = view.to_sheet(c.attach);
    let w = text_width(&center) * h;
    let (mid, underline) = layout_center(c, t, attach, w, h);
    let (hw, hh) = border_half(c, w, h);
    // The border and the point the leader meets it at.
    let meet = match c.border {
        Border::Circle => {
            g.strokes.push(circle_pts(mid, hw));
            let d = dist(attach, mid).max(1e-9);
            [mid[0] + (attach[0] - mid[0]) / d * hw, mid[1] + (attach[1] - mid[1]) / d * hw]
        }
        Border::Box => {
            let (a, b, cc, dd) = ([mid[0] - hw, mid[1] - hh], [mid[0] + hw, mid[1] - hh], [mid[0] + hw, mid[1] + hh], [mid[0] - hw, mid[1] + hh]);
            g.strokes.push(vec![a, b, cc, dd, a]);
            [(a, b), (b, cc), (cc, dd), (dd, a)]
                .iter()
                .map(|(p, q)| nearest_on(*p, *q, attach))
                .min_by(|p, q| dist(*p, attach).total_cmp(&dist(*q, attach)))
                .unwrap_or(mid)
        }
        Border::Triangle => {
            let top = [mid[0], mid[1] + 1.35 * hh];
            let (l, r) = ([mid[0] - hw, mid[1] - 0.65 * hh], [mid[0] + hw, mid[1] - 0.65 * hh]);
            g.strokes.push(vec![l, r, top, l]);
            [(l, r), (r, top), (top, l)]
                .iter()
                .map(|(p, q)| nearest_on(*p, *q, attach))
                .min_by(|p, q| dist(*p, attach).total_cmp(&dist(*q, attach)))
                .unwrap_or(mid)
        }
        Border::Underline => {
            let (a, b) = underline.unwrap_or((t, t));
            g.strokes.push(vec![a, b]);
            if dist(a, attach) <= dist(b, attach) { a } else { b }
        }
        Border::None => t,
    };
    // The leader, with an arrowhead on the part scaled to the text (visible at sheet zoom).
    if dist(meet, attach) > 1e-6 {
        g.strokes.push(vec![meet, attach]);
        let len = callout_arrow_length(style, h).min(0.5 * dist(meet, attach));
        let d = [attach[0] - meet[0], attach[1] - meet[1]];
        let l = d[0].hypot(d[1]).max(1e-12);
        let u = [d[0] / l, d[1] / l];
        let base = [attach[0] - u[0] * len, attach[1] - u[1] * len];
        let n = [-u[1] * len * 0.3, u[0] * len * 0.3];
        g.fills.push([attach, [base[0] + n[0], base[1] + n[1]], [base[0] - n[0], base[1] - n[1]]]);
    }
    // Texts: the centre inside (on) the border, the others around it.
    let place = |s: &str, x: f64, y: f64, g: &mut AnnGraphics| {
        if s.is_empty() {
            return;
        }
        let tw = text_width(s) * h;
        g.texts.push(PlacedText { pos: [x, y], height: h, text: s.to_string() });
        g.boxes.push(([x - 0.2 * h, y - 0.7 * h], [x + tw + 0.2 * h, y + 0.7 * h]));
    };
    place(&center, mid[0] - w / 2.0, mid[1], &mut g);
    let side = if c.border == Border::Underline || c.border == Border::None { w / 2.0 + 0.5 * h } else { hw + 0.5 * h };
    place(&right, mid[0] + side, mid[1], &mut g);
    let lw = text_width(&left) * h;
    place(&left, mid[0] - side - lw, mid[1], &mut g);
    let vert = if c.border == Border::Underline || c.border == Border::None { 1.6 * h } else { hh + 1.0 * h };
    let uw = text_width(&upper) * h;
    place(&upper, mid[0] - uw / 2.0, mid[1] + vert, &mut g);
    let bw = text_width(&lower) * h;
    let below = if c.border == Border::Underline { 1.75 * h } else { vert };
    place(&lower, mid[0] - bw / 2.0, mid[1] - below, &mut g);
    // The border's box picks the callout too.
    g.boxes.push(([mid[0] - hw, mid[1] - hh], [mid[0] + hw, mid[1] + hh]));
    g.grips.push((t, GripKind::Text));
    g.grips.push((attach, GripKind::Attach(0)));
    g.attached.push(vec![attach]);
    g.attached_dead.push(g.dangling);
    g
}

/// A callout leader's arrowhead length (mm) for text height `h`: 1.1 text heights, at least
/// the drawing's dimension arrow.
pub fn callout_arrow_length(style: &DrawingStyle, h: f64) -> f64 {
    style.dim_arrow_length.max(1.1 * h)
}

/// **Inference lines** (D11.6): a callout at sheet point `p` snaps to the vertical or horizontal
/// through another callout's anchor within `tol`; returns the snapped point and the dashed
/// guides to draw (from the other callout to the snapped point).
pub fn inference(p: P2, others: &[P2], tol: f64) -> (P2, Vec<(P2, P2)>) {
    let mut out = p;
    let mut lines = Vec::new();
    let best_x = others.iter().filter(|o| (o[0] - p[0]).abs() <= tol).min_by(|a, b| (a[0] - p[0]).abs().total_cmp(&(b[0] - p[0]).abs()));
    let best_y = others.iter().filter(|o| (o[1] - p[1]).abs() <= tol).min_by(|a, b| (a[1] - p[1]).abs().total_cmp(&(b[1] - p[1]).abs()));
    if let Some(o) = best_x {
        out[0] = o[0];
    }
    if let Some(o) = best_y {
        out[1] = o[1];
    }
    if let Some(o) = best_x {
        lines.push((*o, out));
    }
    if let Some(o) = best_y {
        lines.push((*o, out));
    }
    (out, lines)
}

/// Callout `c` of view `view` after an update from the assembly `old` to `new` (P3C.6): its
/// leader follows its occurrence (the origin's move, projected); when the occurrence is gone it
/// keeps its place and its last texts (it dangles red).
pub fn refresh_callout(view: &View, old: Option<&AssemblyInfo>, new: Option<&AssemblyInfo>, boms: &[&BomData], c: &Callout) -> Callout {
    let mut out = c.clone();
    let frame = view.frame.view_frame();
    let proj = |p: [f64; 3]| {
        let q = frame.to_2d(&nalgebra::Point3::new(p[0], p[1], p[2]));
        [q.x, q.y]
    };
    let was = old.and_then(|a| a.occurrence(&c.occurrence));
    match new.and_then(|a| a.occurrence(&c.occurrence)) {
        Some(now) => {
            if let Some(was) = was {
                let (a, b) = (proj(was.origin), proj(now.origin));
                out.attach = [c.attach[0] + b[0] - a[0], c.attach[1] + b[1] - a[1]];
            }
            out.last = None;
        }
        None => {
            if c.last.is_none() {
                out.last = Some(c.fields.all().map(|f| resolve_field(f, was, boms)));
            }
        }
    }
    out
}

/// A view model with what callouts read besides the geometry: the assembly the drawing keeps
/// for the view (its occurrences) and the sheet's BOM tables.
pub struct SheetModel<'a> {
    pub inner: &'a dyn ViewModel,
    pub assembly: Option<&'a AssemblyInfo>,
    pub tables: &'a [Table],
}

impl<'a> SheetModel<'a> {
    /// The model of view `v` on sheet `sheet` of `d`.
    pub fn new(d: &'a crate::Drawing, sheet: &'a crate::Sheet, v: &View, inner: &'a dyn ViewModel) -> Self {
        Self { inner, assembly: d.source(v.reference.element).and_then(|s| s.assembly.as_ref()), tables: &sheet.tables }
    }
}

impl ViewModel for SheetModel<'_> {
    fn projection(&self) -> &cadrs_kernel::Projection {
        self.inner.projection()
    }
    fn model_edge(&self, name: &cadrs_kernel::naming::EdgeName) -> Option<&crate::annotation::ModelEdge> {
        self.inner.model_edge(name)
    }
    fn hole(&self, feature: &Uuid) -> Option<&crate::annotation::HoleInfo> {
        self.inner.hole(feature)
    }
    fn hatch(&self) -> &[Vec<[f64; 2]>] {
        self.inner.hatch()
    }
    fn threads(&self) -> &[crate::annotation::ThreadInfo] {
        self.inner.threads()
    }
    fn chamfer(&self, feature: &Uuid) -> Option<&crate::annotation::ChamferInfo> {
        self.inner.chamfer(feature)
    }
    fn occurrence(&self, id: &Uuid) -> Option<&OccurrenceInfo> {
        self.assembly.and_then(|a| a.occurrence(id)).or_else(|| self.inner.occurrence(id))
    }
    fn is_assembly(&self) -> bool {
        self.assembly.is_some() || self.inner.is_assembly()
    }
    fn boms(&self) -> Vec<&BomData> {
        self.tables.iter().filter_map(|t| t.bom.as_ref()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::ModelData;
    use crate::standard::Scale;
    use crate::view::NamedView;
    use crate::{ObjectRef, style::DrawingStyle};

    /// P3C wrap-up: BOM columns are sized to their content on one line (the Ex3 Name column
    /// holds "Hex socket head cap screw ISO 4762" unwrapped), up to 30 text heights.
    #[test]
    fn bom_columns_fit_their_content() {
        let mut b = bom();
        b.rows[0].cells[1] = "Hex socket head cap screw ISO 4762".into();
        let h = 2.5;
        let w = column_widths(&b, h);
        assert!(w[1] >= text_width("Hex socket head cap screw ISO 4762") * h);
        let bold = crate::rich::CharStyle { bold: true, ..Default::default() };
        assert!(w[0] >= crate::rich::width("Item No.", &bold) * h, "the header on one line");
        b.rows[0].cells[1] = "word ".repeat(40);
        assert!(column_widths(&b, h)[1] <= 31.3 * h);
    }

    #[test]
    fn callout_arrowheads_scale_with_the_text() {
        let style = DrawingStyle::default();
        assert!(callout_arrow_length(&style, 3.048) >= 1.1 * 3.048);
        assert!(callout_arrow_length(&style, 0.5) >= style.dim_arrow_length);
    }

    fn info() -> AssemblyInfo {
        AssemblyInfo {
            occurrences: vec![
                OccurrenceInfo { id: Uuid::from_u128(1), name: "Flange <1>".into(), origin: [0.0; 3], props: vec![("Name".into(), "Universal Joint Flange".into())] },
                OccurrenceInfo { id: Uuid::from_u128(2), name: "Flange <2>".into(), origin: [0.0, 0.0, 50.0], props: vec![("Name".into(), "Universal Joint Flange".into())] },
            ],
        }
    }

    fn bom() -> BomData {
        BomData {
            assembly: Uuid::from_u128(9),
            kind: BomType::Flattened,
            order: BomOrder::TopToBottom,
            columns: vec!["Item No.".into(), "Name".into(), "Quantity".into()],
            rows: vec![BomRowData {
                item: "1".into(),
                depth: 0,
                quantity: 2,
                occurrences: vec![Uuid::from_u128(1), Uuid::from_u128(2)],
                owner: String::new(),
                cells: vec!["1".into(), "Universal Joint Flange".into(), "2".into()],
            }],
            source_hash: 0,
        }
    }

    #[test]
    fn fields_resolve_part_and_table_properties() {
        let a = info();
        let b = bom();
        let occ = a.occurrence(&Uuid::from_u128(2));
        assert_eq!(resolve_field("{Part: Name}", occ, &[&b]), "Universal Joint Flange");
        assert_eq!(resolve_field("x {Table: Qty.}", occ, &[&b]), "x 2");
        assert_eq!(resolve_field("{Table: Item No.}", occ, &[&b]), "1");
        // Without a BOM table only the part's properties read.
        assert_eq!(resolve_field("x {Table: Qty.}", occ, &[]), "x");
        assert_eq!(resolve_field("{Part: Vendor}", occ, &[&b]), "");
        assert_eq!(parse_field("a {Part: Name} b"), vec![FieldPart::Text("a ".into()), FieldPart::Part("Name".into()), FieldPart::Text(" b".into())]);
        assert_eq!(token(false, "Qty."), "{Table: Qty.}");
    }

    #[test]
    fn bom_tables_follow_the_order() {
        let style = DrawingStyle::default();
        let t = bom_table(bom(), Corner::TopRight, [260.0, 200.0], &style);
        assert_eq!(t.cells[0][0].plain_text(), "Item No.");
        assert!(t.heading(0) && !t.heading(1));
        let mut b = bom();
        b.order = BomOrder::BottomToTop;
        let t2 = refreshed_bom_table(&t, b, &style);
        assert_eq!(t2.cells[1][0].plain_text(), "Item No.", "the header at the bottom");
        assert!(t2.heading(1) && !t2.heading(0));
        assert_eq!(t2.at, t.at);
        assert_eq!(t2.cols, t.cols, "same columns, same widths");
        assert_eq!(snap_to_border([259.0, 199.0], Corner::TopRight, ([10.0, 10.0], [260.0, 200.0]), 3.0), Some([260.0, 200.0]));
        assert_eq!(snap_to_border([250.0, 199.0], Corner::TopRight, ([10.0, 10.0], [260.0, 200.0]), 3.0), None);
    }

    /// P3E.5 (TD10.5): a BOM's bottom-right corner snaps to the title block's left edge.
    #[test]
    fn a_table_corner_snaps_to_the_title_block() {
        let (frame, block) = (([12.7, 12.7], [419.1, 266.7]), ([260.35, 12.7], [419.1, 57.15]));
        // Its bottom-left corner (on the frame's bottom), and its top-left corner.
        assert_eq!(snap_to_title_block([258.0, 14.0], Corner::BottomRight, block, 4.0), Some([260.35, 12.7]));
        assert_eq!(snap_to_title_block([262.0, 56.0], Corner::BottomRight, block, 4.0), Some([260.35, 57.15]));
        // Level with the edge, between its ends: onto the edge.
        assert_eq!(snap_to_title_block([258.5, 30.0], Corner::BottomRight, block, 4.0), Some([260.35, 30.0]));
        // Standing on the block: its top-right corner.
        assert_eq!(snap_to_title_block([417.0, 58.0], Corner::BottomRight, block, 4.0), Some([419.1, 57.15]));
        // Too far, or a left-hand corner off the block's top: no snap.
        assert_eq!(snap_to_title_block([250.0, 30.0], Corner::BottomRight, block, 4.0), None);
        assert_eq!(snap_to_title_block([258.5, 30.0], Corner::BottomLeft, block, 4.0), None);
        // With the frame: the nearer one wins (the frame's corner is the block's right end).
        assert_eq!(snap_table_corner([258.0, 14.0], Corner::BottomRight, frame, Some(block), 4.0), Some([260.35, 12.7]));
        assert_eq!(snap_table_corner([418.0, 13.0], Corner::BottomRight, frame, Some(block), 4.0), Some([419.1, 12.7]));
        assert_eq!(snap_table_corner([258.0, 14.0], Corner::BottomRight, frame, None, 4.0), None);
        // A BOM at the block's corner kept within the frame across: its left edge on the frame
        // (or left where it was, when it fits).
        let style = DrawingStyle::default();
        let w = bom_table(bom(), Corner::BottomRight, [0.0, 0.0], &style).width();
        let mins: f64 = bom_table(bom(), Corner::BottomRight, [0.0, 0.0], &style).column_mins().iter().sum();
        // Its right edge where the full width overflows but the narrowest fits.
        let x = frame.0[0] + (w + mins) / 2.0;
        let t = bom_table(bom(), Corner::BottomRight, [x, 12.7], &style);
        assert!(t.rect().0[0] < frame.0[0], "the test table overflows");
        let f = fit_within(&t, frame);
        assert!((f.rect().0[0] - frame.0[0]).abs() < 1e-6, "{:?}", f.rect());
        assert_eq!(f.at, t.at, "the fixed corner stays");
        let wide = bom_table(bom(), Corner::BottomRight, [260.35, 12.7], &style);
        assert_eq!(fit_within(&wide, frame), wide);
    }

    #[test]
    fn callouts_dangle_when_their_occurrence_goes() {
        let a = info();
        let v = View::base(ObjectRef { element: Uuid::from_u128(9), part: None }, NamedView::Front, Scale::new(1, 2), [100.0, 100.0]);
        let c = Callout {
            occurrence: Uuid::from_u128(2),
            attach: [0.0, 10.0],
            text: [40.0, 40.0],
            border: Border::Underline,
            size: 0,
            text_height: 3.0,
            fields: CalloutFields { center: "{Part: Name}".into(), right: "x {Table: Qty.}".into(), ..Default::default() },
            last: None,
        };
        let geo = ModelData::default();
        let tables = [bom_table(bom(), Corner::TopRight, [260.0, 200.0], &DrawingStyle::default())];
        let m = SheetModel { inner: &geo, assembly: Some(&a), tables: &tables };
        let texts = callout_texts(&m, &c);
        assert_eq!(texts[4], "Universal Joint Flange");
        assert_eq!(texts[3], "x 2");
        assert!(!callout_graphics(&DrawingStyle::default(), &v, &m, &c).dangling);
        // The occurrence moves 50 up in Front: the leader follows; then it goes away.
        let mut moved = a.clone();
        moved.occurrences[1].origin = [0.0, 0.0, 100.0];
        let boms: Vec<&BomData> = tables.iter().filter_map(|t| t.bom.as_ref()).collect();
        let c2 = refresh_callout(&v, Some(&a), Some(&moved), &boms, &c);
        assert!((c2.attach[1] - 60.0).abs() < 1e-9, "{:?}", c2.attach);
        let mut gone = a.clone();
        gone.occurrences.remove(1);
        let c3 = refresh_callout(&v, Some(&a), Some(&gone), &boms, &c);
        let m2 = SheetModel { inner: &geo, assembly: Some(&gone), tables: &tables };
        assert!(callout_dangles(&m2, &c3));
        let g = callout_graphics(&DrawingStyle::default(), &v, &m2, &c3);
        assert!(g.dangling);
        assert_eq!(callout_texts(&m2, &c3)[4], "Universal Joint Flange", "the last text stays");
    }

    #[test]
    fn inference_snaps_to_other_callouts() {
        let (p, lines) = inference([101.0, 57.0], &[[100.0, 80.0], [30.0, 57.5]], 2.0);
        assert_eq!(p, [100.0, 57.5]);
        assert_eq!(lines.len(), 2);
        let (q, none) = inference([150.0, 20.0], &[[100.0, 80.0]], 2.0);
        assert_eq!(q, [150.0, 20.0]);
        assert!(none.is_empty());
    }
}
