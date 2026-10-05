//! Tables (P3C.4, D10, X9): a grid of rich-text cells on the sheet.
//!
//! A [`Table`] is placed by its **fixed corner** (`at`): the corner that stays put while the
//! table grows or shrinks (D10.1, D10.4). Columns have widths and rows heights (sheet mm; a row
//! grows to fit its text). An optional **title row** (one cell across the table) and **header
//! row** come first and draw bold and centred. Cells can be merged into rectangles
//! ([`Merge`]); a merged block shows its top-left cell's text.
//!
//! Every edit is a pure function returning the new table, applied as one
//! [`crate::DrawingOp::SetTable`] so it is undoable: inserting and removing rows and columns
//! (merges grow, shrink and move with them), merging and unmerging, resizing from the fixed
//! corner by a midpoint grip ([`Table::resize`]), moving, and changing the fixed corner
//! ([`Table::set_fixed`], which keeps the table where it is). [`Table::next_cell`] is the Tab /
//! Shift+Tab order (D10.2): row by row, a merged block once.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::note::NoteText;
use crate::rich::{self, FieldContext, HAlign, P2, RichText};
use crate::style::DrawingStyle;

/// Identifies a table on its sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TableId(pub Uuid);

impl TableId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TableId {
    fn default() -> Self {
        Self::new()
    }
}

/// A corner of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];

    pub fn label(self) -> &'static str {
        match self {
            Corner::TopLeft => "Top left",
            Corner::TopRight => "Top right",
            Corner::BottomLeft => "Bottom left",
            Corner::BottomRight => "Bottom right",
        }
    }

    pub fn right(self) -> bool {
        matches!(self, Corner::TopRight | Corner::BottomRight)
    }

    fn bottom(self) -> bool {
        matches!(self, Corner::BottomLeft | Corner::BottomRight)
    }
}

/// A side of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// Merged cells: `rows` × `cols` from (`row`, `col`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Merge {
    pub row: usize,
    pub col: usize,
    pub rows: usize,
    pub cols: usize,
}

impl Merge {
    pub fn contains(&self, r: usize, c: usize) -> bool {
        r >= self.row && r < self.row + self.rows && c >= self.col && c < self.col + self.cols
    }
}

/// A table on a sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub id: TableId,
    /// The fixed corner's place on the sheet (mm).
    pub at: P2,
    pub fixed: Corner,
    /// Column widths, left to right (mm).
    pub cols: Vec<f64>,
    /// Row heights, top to bottom (mm), as drawn (grown to fit their text).
    pub rows: Vec<f64>,
    /// How much each row was grown to fit its text (P3C.7): a row whose text gets shorter
    /// shrinks back, down to its own height (`rows[r] - grown[r]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grown: Vec<f64>,
    /// `cells[row][col]`.
    pub cells: Vec<Vec<RichText>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merges: Vec<Merge>,
    #[serde(default)]
    pub title_row: bool,
    #[serde(default)]
    pub header_row: bool,
    /// The header row is the last row, not the first (a BOM table ordered bottom to top,
    /// P3C.5).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub header_last: bool,
    /// Cap height of cell text (mm).
    pub text_height: f64,
    /// A BOM table's source (P3C.5, D11): the assembly, BOM type, order and rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bom: Option<crate::assembly::BomData>,
}

/// A cell (row, column).
pub type Cell = (usize, usize);

impl Table {
    /// A table of `rows` × `cols` body cells, with a title row and a header row on top when
    /// asked, its fixed corner at `at` (the Table dialog, D10.1).
    pub fn new(rows: usize, cols: usize, title: bool, header: bool, fixed: Corner, at: P2, style: &DrawingStyle) -> Self {
        let n = rows.max(1) + title as usize + header as usize;
        let cols = cols.max(1);
        let rh = style.table_row_height;
        let mut t = Self {
            id: TableId::new(),
            at,
            fixed,
            cols: vec![rh * 5.0; cols],
            rows: vec![rh; n],
            grown: Vec::new(),
            cells: vec![vec![RichText::default(); cols]; n],
            merges: Vec::new(),
            title_row: title,
            header_row: header,
            header_last: false,
            text_height: style.table_text_height,
            bom: None,
        };
        if title && cols > 1 {
            t.merges.push(Merge { row: 0, col: 0, rows: 1, cols });
        }
        t
    }

    /// A revision table (P3C.8, X14): a "REVISIONS" title, the header REV | DESCRIPTION | DATE |
    /// APPROVED and `rows` editable rows, the first one "A", "Initial release". The release
    /// workflow that fills it in Onshape is out of scope: its cells are typed.
    pub fn revision(rows: usize, fixed: Corner, at: P2, style: &DrawingStyle) -> Self {
        let mut t = Self::new(rows.max(1), 4, true, true, fixed, at, style);
        let rh = style.table_row_height;
        // Smaller text than a plain table's, so the four columns fit beside a title block.
        t.text_height = style.table_text_height * 0.75;
        t.cols = vec![rh * 1.6, rh * 5.0, rh * 3.0, rh * 3.4];
        t.cells[0][0] = RichText::plain("REVISIONS");
        for (c, h) in ["REV", "DESCRIPTION", "DATE", "APPROVED"].iter().enumerate() {
            t.cells[1][c] = RichText::plain(h);
        }
        t.cells[2][0] = RichText::plain("A");
        t.cells[2][1] = RichText::plain("Initial release");
        t
    }

    /// Row `r`'s own height: as drawn, less what it grew for its text.
    pub fn natural_row(&self, r: usize) -> f64 {
        let h = self.rows.get(r).copied().unwrap_or(0.0);
        (h - self.grown.get(r).copied().unwrap_or(0.0).max(0.0)).max(0.0)
    }

    pub fn n_rows(&self) -> usize {
        self.rows.len()
    }

    pub fn n_cols(&self) -> usize {
        self.cols.len()
    }

    pub fn width(&self) -> f64 {
        self.cols.iter().sum()
    }

    pub fn height(&self) -> f64 {
        self.rows.iter().sum()
    }

    /// The top-left corner on the sheet, for heights `rows`.
    fn top_left_with(&self, rows: &[f64]) -> P2 {
        let (w, h) = (self.width(), rows.iter().sum::<f64>());
        [
            if self.fixed.right() { self.at[0] - w } else { self.at[0] },
            if self.fixed.bottom() { self.at[1] + h } else { self.at[1] },
        ]
    }

    /// The table's box (min, max) with its stored row heights.
    pub fn rect(&self) -> (P2, P2) {
        let tl = self.top_left_with(&self.rows);
        (([tl[0], tl[1] - self.height()]), [tl[0] + self.width(), tl[1]])
    }

    /// Where a corner is now.
    pub fn corner(&self, c: Corner) -> P2 {
        let (lo, hi) = self.rect();
        [if c.right() { hi[0] } else { lo[0] }, if c.bottom() { lo[1] } else { hi[1] }]
    }

    /// The merge a cell is in.
    pub fn merge_at(&self, r: usize, c: usize) -> Option<&Merge> {
        self.merges.iter().find(|m| m.contains(r, c))
    }

    /// The cell a cell shows as: its merge's top-left, or itself.
    pub fn origin(&self, r: usize, c: usize) -> Cell {
        self.merge_at(r, c).map_or((r, c), |m| (m.row, m.col))
    }

    /// Whether a cell is hidden under another one's merge.
    pub fn covered(&self, r: usize, c: usize) -> bool {
        self.origin(r, c) != (r, c)
    }

    /// A cell's extent in rows and columns.
    pub fn span(&self, r: usize, c: usize) -> (usize, usize) {
        self.merges.iter().find(|m| m.row == r && m.col == c).map_or((1, 1), |m| (m.rows, m.cols))
    }

    /// Tab (or Shift+Tab when `back`): the next cell a merged block counts once, wrapping.
    pub fn next_cell(&self, from: Cell, back: bool) -> Cell {
        let order: Vec<Cell> = (0..self.n_rows())
            .flat_map(|r| (0..self.n_cols()).map(move |c| (r, c)))
            .filter(|(r, c)| !self.covered(*r, *c))
            .collect();
        let at = self.origin(from.0, from.1);
        let i = order.iter().position(|x| *x == at).unwrap_or(0);
        let n = order.len();
        order[if back { (i + n - 1) % n } else { (i + 1) % n }]
    }

    fn check(&self) -> Result<(), String> {
        if self.rows.is_empty() || self.cols.is_empty() {
            return Err("a table needs a row and a column".into());
        }
        Ok(())
    }

    /// A new row before row `at` (`at` = rows for the end).
    pub fn insert_row(&self, at: usize) -> Result<Table, String> {
        let mut t = self.clone();
        let at = at.min(t.n_rows());
        let h = t.natural_row(at.min(t.n_rows().saturating_sub(1)));
        let h = if h > 0.0 { h } else { 7.0 };
        t.rows.insert(at, h);
        if t.grown.len() + 1 == t.rows.len() {
            t.grown.insert(at, 0.0);
        }
        t.cells.insert(at, vec![RichText::default(); t.n_cols()]);
        for m in &mut t.merges {
            if m.row >= at {
                m.row += 1;
            } else if m.row + m.rows > at {
                m.rows += 1;
            }
        }
        Ok(t)
    }

    pub fn remove_row(&self, at: usize) -> Result<Table, String> {
        if self.n_rows() <= 1 || at >= self.n_rows() {
            return Err("can't remove the last row".into());
        }
        let mut t = self.clone();
        t.rows.remove(at);
        if t.grown.len() > at {
            t.grown.remove(at);
        }
        t.cells.remove(at);
        if t.title_row && at == 0 {
            t.title_row = false;
        } else if t.header_row && at == t.title_row as usize {
            t.header_row = false;
        }
        t.merges = t
            .merges
            .iter()
            .filter_map(|m| {
                let mut m = *m;
                if m.row > at {
                    m.row -= 1;
                } else if m.row + m.rows > at {
                    m.rows -= 1;
                }
                (m.rows > 0 && m.rows * m.cols > 1).then_some(m)
            })
            .collect();
        t.check()?;
        Ok(t)
    }

    pub fn insert_col(&self, at: usize) -> Result<Table, String> {
        let mut t = self.clone();
        let at = at.min(t.n_cols());
        let old = t.n_cols();
        let w = t.cols.get(at).or(t.cols.last()).copied().unwrap_or(25.0);
        t.cols.insert(at, w);
        for row in &mut t.cells {
            row.insert(at, RichText::default());
        }
        for m in &mut t.merges {
            // A merge across the whole table (the title) takes in a new last column too.
            if m.col == 0 && m.cols == old {
                m.cols += 1;
            } else if m.col >= at {
                m.col += 1;
            } else if m.col + m.cols > at {
                m.cols += 1;
            }
        }
        Ok(t)
    }

    pub fn remove_col(&self, at: usize) -> Result<Table, String> {
        if self.n_cols() <= 1 || at >= self.n_cols() {
            return Err("can't remove the last column".into());
        }
        let mut t = self.clone();
        t.cols.remove(at);
        for row in &mut t.cells {
            row.remove(at);
        }
        t.merges = t
            .merges
            .iter()
            .filter_map(|m| {
                let mut m = *m;
                if m.col > at {
                    m.col -= 1;
                } else if m.col + m.cols > at {
                    m.cols -= 1;
                }
                (m.cols > 0 && m.rows * m.cols > 1).then_some(m)
            })
            .collect();
        t.check()?;
        Ok(t)
    }

    /// Merges the rectangle from `a` to `b` (any two corners), grown to take in any merge it
    /// cuts. The top-left cell keeps its text, followed by the others' non-empty texts.
    pub fn merge(&self, a: Cell, b: Cell) -> Result<Table, String> {
        let (mut r0, mut r1) = (a.0.min(b.0), a.0.max(b.0));
        let (mut c0, mut c1) = (a.1.min(b.1), a.1.max(b.1));
        if r1 >= self.n_rows() || c1 >= self.n_cols() {
            return Err("no such cell".into());
        }
        // Grow to whole merges.
        loop {
            let mut grown = false;
            for m in &self.merges {
                let overlaps = m.row <= r1 && m.row + m.rows > r0 && m.col <= c1 && m.col + m.cols > c0;
                if overlaps {
                    let (nr0, nr1) = (r0.min(m.row), r1.max(m.row + m.rows - 1));
                    let (nc0, nc1) = (c0.min(m.col), c1.max(m.col + m.cols - 1));
                    if (nr0, nr1, nc0, nc1) != (r0, r1, c0, c1) {
                        (r0, r1, c0, c1) = (nr0, nr1, nc0, nc1);
                        grown = true;
                    }
                }
            }
            if !grown {
                break;
            }
        }
        if r0 == r1 && c0 == c1 {
            return Err("select more than one cell to merge".into());
        }
        let mut t = self.clone();
        t.merges.retain(|m| !(m.row >= r0 && m.row <= r1 && m.col >= c0 && m.col <= c1));
        // Join the texts into the top-left cell.
        let mut joined = t.cells[r0][c0].clone();
        for r in r0..=r1 {
            for c in c0..=c1 {
                if (r, c) == (r0, c0) {
                    continue;
                }
                let other = std::mem::take(&mut t.cells[r][c]);
                if !other.is_empty() {
                    if joined.is_empty() {
                        joined = other;
                    } else {
                        joined.paragraphs.extend(other.paragraphs);
                    }
                }
                t.cells[r][c] = RichText::default();
            }
        }
        t.cells[r0][c0] = joined;
        t.merges.push(Merge { row: r0, col: c0, rows: r1 - r0 + 1, cols: c1 - c0 + 1 });
        Ok(t)
    }

    /// Splits the merge a cell is in back into single cells.
    pub fn unmerge(&self, cell: Cell) -> Result<Table, String> {
        let m = *self.merge_at(cell.0, cell.1).ok_or("the cell isn't merged")?;
        let mut t = self.clone();
        t.merges.retain(|x| *x != m);
        Ok(t)
    }

    /// Sets a cell's text (its merge's, for a covered cell).
    pub fn set_cell(&self, cell: Cell, text: RichText) -> Result<Table, String> {
        let (r, c) = self.origin(cell.0, cell.1);
        if r >= self.n_rows() || c >= self.n_cols() {
            return Err("no such cell".into());
        }
        let mut t = self.clone();
        t.cells[r][c] = text;
        Ok(t)
    }

    /// The sides a midpoint grip resizes: the two away from the fixed corner.
    pub fn free_sides(&self) -> [Side; 2] {
        [
            if self.fixed.right() { Side::Left } else { Side::Right },
            if self.fixed.bottom() { Side::Top } else { Side::Bottom },
        ]
    }

    /// Resizes by dragging the midpoint grip of side `side` to `to` (D10.4): the columns (or
    /// rows) scale so that side follows the pointer, from the fixed corner, which stays put.
    pub fn resize(&self, side: Side, to: P2) -> Result<Table, String> {
        if !self.free_sides().contains(&side) {
            return Err("that side holds the fixed corner".into());
        }
        let mut t = self.clone();
        let min = self.text_height * 1.5;
        match side {
            Side::Left | Side::Right => {
                // No column narrower than its longest word: the others give up the width.
                let mins = self.column_mins();
                let w = (to[0] - self.at[0]).abs().max(mins.iter().sum());
                t.cols = scale_columns(&self.cols, &mins, w);
            }
            Side::Top | Side::Bottom => {
                let h = (to[1] - self.at[1]).abs().max(min * self.n_rows() as f64);
                let k = h / self.height();
                t.rows.iter_mut().for_each(|r| *r *= k);
                // The resized heights are the rows' own.
                t.grown.clear();
            }
        }
        Ok(t)
    }

    /// Moves the line between column `i - 1` and column `i` to the sheet x `x` (D12.9): the two
    /// columns share their width between them, each at least 1.5 text heights; the table keeps
    /// its width and place.
    pub fn set_column_edge(&self, i: usize, x: f64) -> Result<Table, String> {
        if i == 0 || i >= self.n_cols() {
            return Err("no such column line".into());
        }
        let (lo, _) = self.rect();
        let left = lo[0] + self.cols[..i - 1].iter().sum::<f64>();
        let pair = self.cols[i - 1] + self.cols[i];
        let mins = self.column_mins();
        let (min_a, min_b) = (mins[i - 1], mins[i]);
        if min_a + min_b > pair + 1e-9 {
            return Err("the columns are at their narrowest".into());
        }
        let a = (x - left).clamp(min_a, pair - min_b);
        let mut t = self.clone();
        t.cols[i - 1] = a;
        t.cols[i] = pair - a;
        Ok(t)
    }

    /// Moves the table by `d`.
    pub fn moved(&self, d: P2) -> Table {
        let mut t = self.clone();
        t.at = [t.at[0] + d[0], t.at[1] + d[1]];
        t
    }

    /// Another fixed corner, the table staying where it is (Table properties…, D10.4).
    pub fn set_fixed(&self, c: Corner) -> Table {
        let mut t = self.clone();
        t.at = self.corner(c);
        t.fixed = c;
        t
    }

    /// The narrowest each column can be (mm): its longest word (cells break only between
    /// words, P3C wrap-up), padded, and at least 1.5 text heights. Merged cells don't count.
    pub fn column_mins(&self) -> Vec<f64> {
        let floor = self.text_height * 1.5;
        (0..self.n_cols())
            .map(|c| {
                (0..self.n_rows())
                    .filter(|&r| !self.covered(r, c) && self.span(r, c).1 == 1)
                    .map(|r| {
                        let text = self.shown_text(r, c);
                        let style = rich::CharStyle { bold: self.heading(r), ..Default::default() };
                        let h = if self.title_row && r == 0 { 1.25 * self.text_height } else { self.text_height };
                        let word = text.plain_text().split_whitespace().map(|w| rich::width(w, &style)).fold(0.0, f64::max);
                        word * h + 2.0 * PAD * self.text_height + 1e-6
                    })
                    .fold(floor, f64::max)
            })
            .collect()
    }

    /// Whether a row is the title or header row (bold, centred).
    pub fn heading(&self, r: usize) -> bool {
        let header = if self.header_last { self.n_rows().saturating_sub(1) } else { self.title_row as usize };
        (self.title_row && r == 0) || (self.header_row && r == header)
    }

    /// The text of a cell as drawn: a heading row's bold and centred.
    pub fn shown_text(&self, r: usize, c: usize) -> RichText {
        let mut t = self.cells[r][c].clone();
        if self.heading(r) {
            t.restyle(
                |s| s.bold = true,
                |p| {
                    if p.align == HAlign::Left {
                        p.align = HAlign::Center;
                    }
                },
            );
        }
        t
    }
}

/// Widths `cols` scaled to add up to `total`, none below its `mins` (the columns at their
/// minimum drop out and the rest share what is left, in proportion).
pub fn scale_columns(cols: &[f64], mins: &[f64], total: f64) -> Vec<f64> {
    let mut fixed = vec![false; cols.len()];
    loop {
        let free: f64 = cols.iter().zip(&fixed).filter(|(_, f)| !**f).map(|(c, _)| *c).sum();
        let left = total - mins.iter().zip(&fixed).filter(|(_, f)| **f).map(|(m, _)| *m).sum::<f64>();
        let k = if free > 0.0 { left / free } else { 1.0 };
        let mut changed = false;
        for i in 0..cols.len() {
            if !fixed[i] && cols[i] * k < mins[i] {
                fixed[i] = true;
                changed = true;
            }
        }
        if !changed || fixed.iter().all(|f| *f) {
            return (0..cols.len()).map(|i| if fixed[i] { mins[i] } else { cols[i] * k }).collect();
        }
    }
}

/// The table with its rows grown to fit their text (so the fixed corner and the grips work on
/// the table as drawn), and each column at least as wide as its longest word. Every edit
/// stores this.
pub fn fit_rows(t: &Table, ctx: &FieldContext) -> Table {
    let mut t = t.clone();
    for (c, m) in t.column_mins().into_iter().enumerate() {
        if t.cols[c] < m {
            t.cols[c] = m;
        }
    }
    let t = &t;
    let g = table_graphics(t, ctx, false, None);
    let mut out = t.clone();
    out.grown = (0..t.n_rows()).map(|r| (g.rows[r] - t.natural_row(r)).max(0.0)).collect();
    if out.grown.iter().all(|x| *x == 0.0) {
        out.grown.clear();
    }
    out.rows = g.rows;
    out
}

/// A grip of a table (D10.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableGrip {
    /// Moves the table; the fixed corner is drawn black.
    Corner(Corner),
    /// Resizes from the fixed corner.
    Side(Side),
    /// The line between column `i - 1` and column `i` (P3C.5, D12.9: column widths), on the top
    /// edge.
    Column(usize),
}

/// A table on the sheet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableGraphics {
    /// Grid lines (thin) and the outline (medium).
    pub thin: Vec<(P2, P2)>,
    pub outline: Vec<(P2, P2)>,
    pub texts: Vec<NoteText>,
    pub strokes: Vec<Vec<P2>>,
    /// Each shown cell's box (min, max), by its (row, col).
    pub cells: Vec<(Cell, P2, P2)>,
    /// The laid-out text of each shown cell, and the box point its layout starts at.
    pub layouts: Vec<(Cell, P2, rich::RichLayout)>,
    pub grips: Vec<(P2, TableGrip)>,
    pub min: P2,
    pub max: P2,
    /// The row heights drawn (grown to fit their text).
    pub rows: Vec<f64>,
}

impl TableGraphics {
    pub fn cell_at(&self, p: P2) -> Option<Cell> {
        self.cells
            .iter()
            .find(|(_, lo, hi)| p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1])
            .map(|(c, _, _)| *c)
    }

    pub fn contains(&self, p: P2, tol: f64) -> bool {
        p[0] >= self.min[0] - tol && p[0] <= self.max[0] + tol && p[1] >= self.min[1] - tol && p[1] <= self.max[1] + tol
    }

    pub fn cell_box(&self, c: Cell) -> Option<(P2, P2)> {
        self.cells.iter().find(|(x, _, _)| *x == c).map(|(_, lo, hi)| (*lo, *hi))
    }
}

/// A table's cell padding, in text heights: left and right, and above and below.
const PAD: f64 = 0.45;
const VPAD: f64 = 0.25;

#[allow(clippy::needless_range_loop)]
/// A table on the sheet: its grid, cell texts (rows grown to fit them) and, when `grips`, its
/// grips. `edited` replaces one cell's text (the cell being typed in).
pub fn table_graphics(t: &Table, ctx: &FieldContext, grips: bool, edited: Option<(Cell, &RichText)>) -> TableGraphics {
    let mut g = TableGraphics::default();
    let h = t.text_height;
    let pad = PAD * h;
    let text_of = |r: usize, c: usize| -> RichText {
        match edited {
            Some((cell, text)) if cell == (r, c) => {
                let mut tt = t.clone();
                tt.cells[r][c] = text.clone();
                tt.shown_text(r, c)
            }
            _ => t.shown_text(r, c),
        }
    };
    let cell_h = |r: usize| if t.title_row && r == 0 { h * 1.25 } else { h };
    // Column x positions.
    let widths = |c0: usize, n: usize| t.cols[c0..c0 + n].iter().sum::<f64>();
    // Rows grow to their text (and shrink back to their own height when it gets shorter).
    let mut rows: Vec<f64> = (0..t.n_rows()).map(|r| t.natural_row(r)).collect();
    let mut layouts: Vec<(Cell, rich::RichLayout)> = Vec::new();
    for r in 0..t.n_rows() {
        for c in 0..t.n_cols() {
            if t.covered(r, c) {
                continue;
            }
            let (sr, sc) = t.span(r, c);
            let wrap = (widths(c, sc) - 2.0 * pad).max(h);
            let l = rich::layout_with(&text_of(r, c), ctx, cell_h(r), Some(wrap), false);
            if sr == 1 {
                rows[r] = rows[r].max(l.height + 2.0 * VPAD * h + 0.3 * h);
            }
            layouts.push(((r, c), l));
        }
    }
    let tl = t.top_left_with(&rows);
    g.rows = rows.clone();
    let total_h: f64 = rows.iter().sum();
    let total_w = t.width();
    let xs: Vec<f64> = std::iter::once(0.0).chain(t.cols.iter().scan(0.0, |a, w| {
        *a += w;
        Some(*a)
    })).collect();
    let ys: Vec<f64> = std::iter::once(0.0).chain(rows.iter().scan(0.0, |a, w| {
        *a += w;
        Some(*a)
    })).collect();
    let px = |x: f64| tl[0] + x;
    let py = |y: f64| tl[1] - y;
    g.min = [tl[0], tl[1] - total_h];
    g.max = [tl[0] + total_w, tl[1]];
    // Outline.
    let (x0, x1, y0, y1) = (px(0.0), px(total_w), py(0.0), py(total_h));
    g.outline = vec![([x0, y0], [x1, y0]), ([x1, y0], [x1, y1]), ([x1, y1], [x0, y1]), ([x0, y1], [x0, y0])];
    // Inner lines, broken where a merge spans them.
    for r in 1..t.n_rows() {
        let mut start: Option<f64> = None;
        for c in 0..t.n_cols() {
            let crosses = t.merge_at(r, c).is_some_and(|m| m.row < r);
            match (crosses, start) {
                (false, None) => start = Some(xs[c]),
                (true, Some(s)) => {
                    g.thin.push(([px(s), py(ys[r])], [px(xs[c]), py(ys[r])]));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            g.thin.push(([px(s), py(ys[r])], [px(total_w), py(ys[r])]));
        }
    }
    for c in 1..t.n_cols() {
        let mut start: Option<f64> = None;
        for r in 0..t.n_rows() {
            let crosses = t.merge_at(r, c).is_some_and(|m| m.col < c);
            match (crosses, start) {
                (false, None) => start = Some(ys[r]),
                (true, Some(s)) => {
                    g.thin.push(([px(xs[c]), py(s)], [px(xs[c]), py(ys[r])]));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            g.thin.push(([px(xs[c]), py(s)], [px(xs[c]), py(total_h)]));
        }
    }
    // Cells and their text, centred vertically.
    for ((r, c), l) in layouts {
        let (sr, sc) = t.span(r, c);
        let lo = [px(xs[c]), py(ys[r + sr])];
        let hi = [px(xs[c + sc]), py(ys[r])];
        g.cells.push(((r, c), lo, hi));
        let box_h = hi[1] - lo[1];
        let text_h = l.height + 0.3 * cell_h(r);
        let origin = [lo[0] + pad, hi[1] - (box_h - text_h).max(0.0) / 2.0];
        for p in &l.pieces {
            g.texts.push(NoteText {
                piece: p.clone(),
                pos: [origin[0] + p.pos[0], origin[1] + p.pos[1]],
                rotation: 0.0,
            });
        }
        for s in &l.strokes {
            g.strokes.push(s.iter().map(|q| [origin[0] + q[0], origin[1] + q[1]]).collect());
        }
        g.layouts.push(((r, c), origin, l));
    }
    if grips {
        let (lo, hi) = (g.min, g.max);
        let corner = |c: Corner| [if c.right() { hi[0] } else { lo[0] }, if c.bottom() { lo[1] } else { hi[1] }];
        for c in Corner::ALL {
            g.grips.push((corner(c), TableGrip::Corner(c)));
        }
        // The column lines' grips, on the top edge.
        let mut x = lo[0];
        for (i, w) in t.cols.iter().enumerate().take(t.n_cols().saturating_sub(1)) {
            x += w;
            g.grips.push(([x, hi[1]], TableGrip::Column(i + 1)));
        }
        for s in t.free_sides() {
            let p = match s {
                Side::Left => [lo[0], (lo[1] + hi[1]) / 2.0],
                Side::Right => [hi[0], (lo[1] + hi[1]) / 2.0],
                Side::Top => [(lo[0] + hi[0]) / 2.0, hi[1]],
                Side::Bottom => [(lo[0] + hi[0]) / 2.0, lo[1]],
            };
            g.grips.push((p, TableGrip::Side(s)));
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        let style = crate::template::builtin("ANSI_A_INCH.dwt").unwrap().style();
        Table::new(3, 4, true, true, Corner::TopLeft, [20.0, 180.0], &style)
    }

    #[test]
    fn new_table_has_title_and_header_rows() {
        let t = table();
        assert_eq!((t.n_rows(), t.n_cols()), (5, 4));
        assert_eq!(t.merges, vec![Merge { row: 0, col: 0, rows: 1, cols: 4 }]);
        assert!(t.heading(0) && t.heading(1) && !t.heading(2));
        assert_eq!(t.rect().1, [20.0 + t.width(), 180.0]);
    }

    #[test]
    fn tab_order_skips_covered_cells_and_wraps() {
        let t = table();
        // The title is one cell; Tab goes on to the header row.
        assert_eq!(t.next_cell((0, 0), false), (1, 0));
        assert_eq!(t.next_cell((1, 3), false), (2, 0));
        assert_eq!(t.next_cell((2, 0), true), (1, 3));
        assert_eq!(t.next_cell((1, 0), true), (0, 0));
        // Wraps both ways.
        assert_eq!(t.next_cell((4, 3), false), (0, 0));
        assert_eq!(t.next_cell((0, 0), true), (4, 3));
        // A covered cell counts as its merge.
        assert_eq!(t.next_cell((0, 2), false), (1, 0));
        let m = t.merge((2, 1), (3, 2)).unwrap();
        assert_eq!(m.next_cell((2, 0), false), (2, 1));
        assert_eq!(m.next_cell((2, 1), false), (2, 3));
        assert_eq!(m.next_cell((3, 0), false), (3, 3));
        assert_eq!(m.next_cell((3, 3), true), (3, 0));
    }

    #[test]
    fn merge_unmerge_and_rows() {
        let t = table();
        let t1 = t.set_cell((2, 0), RichText::plain("A")).unwrap().set_cell((2, 1), RichText::plain("B")).unwrap();
        let m = t1.merge((2, 0), (3, 1)).unwrap();
        assert_eq!(m.merge_at(3, 1), Some(&Merge { row: 2, col: 0, rows: 2, cols: 2 }));
        assert_eq!(m.cells[2][0].paragraphs.len(), 2, "texts joined");
        assert!(m.covered(3, 1) && !m.covered(2, 0));
        // Merging across part of a merge takes in the whole of it.
        let grown = m.merge((3, 1), (3, 2)).unwrap();
        assert_eq!(grown.merge_at(2, 2), Some(&Merge { row: 2, col: 0, rows: 2, cols: 3 }));
        let un = m.unmerge((3, 1)).unwrap();
        assert!(un.merge_at(2, 0).is_none() && !un.covered(3, 1));
        assert!(t.merge((2, 2), (2, 2)).is_err());
        // Rows through a merge stretch it; removing them shrinks it.
        let r = m.insert_row(3).unwrap();
        assert_eq!(r.merge_at(2, 0).unwrap().rows, 3);
        assert_eq!(r.n_rows(), 6);
        let r2 = r.remove_row(3).unwrap().remove_row(3).unwrap();
        assert!(r2.merge_at(2, 0).is_none() || r2.merge_at(2, 0).unwrap().rows * r2.merge_at(2, 0).unwrap().cols > 1);
        // Columns.
        let c = t.insert_col(4).unwrap();
        assert_eq!(c.n_cols(), 5);
        assert_eq!(c.merges[0].cols, 5, "the title spans the new column");
        let c2 = c.remove_col(0).unwrap();
        assert_eq!(c2.merges[0].cols, 4);
    }

    #[test]
    fn rows_grow_for_wrapped_text_and_shrink_back() {
        let r = crate::title_block::ReferenceProps::default();
        let d = crate::rich::DrawingContext::default();
        let ctx = FieldContext { reference: &r, drawing: &d };
        let t = fit_rows(&table(), &ctx);
        let base = t.rows[2];
        let long = "A long cell text that wraps onto several lines in its column";
        let tall = fit_rows(&t.set_cell((2, 0), RichText::plain(long)).unwrap(), &ctx);
        assert!(tall.rows[2] > base * 1.5, "{} vs {base}", tall.rows[2]);
        assert!((tall.natural_row(2) - base).abs() < 1e-9);
        // The text unwraps: the row shrinks back to its own height (P3C.4's delta).
        let short = fit_rows(&tall.set_cell((2, 0), RichText::plain("A")).unwrap(), &ctx);
        assert!((short.rows[2] - base).abs() < 1e-9, "{} vs {base}", short.rows[2]);
        assert!(short.grown.is_empty());
        // Inserting a row next to a grown one gives it the row's own height.
        let ins = tall.insert_row(3).unwrap();
        assert!((ins.rows[3] - base).abs() < 1e-9 && ins.grown.len() == ins.rows.len());
    }

    /// P3C wrap-up: cells break between words only ("Quantit/y" and "MSB-0001x/C" were split),
    /// columns never get narrower than their longest word, and a typed long word widens its
    /// column.
    #[test]
    fn cells_wrap_between_words_and_columns_keep_their_longest_word() {
        let r = crate::title_block::ReferenceProps::default();
        let d = crate::rich::DrawingContext::default();
        let ctx = FieldContext { reference: &r, drawing: &d };
        let mut t = table();
        t.cells[1][1] = RichText::plain("Quantity");
        t.cells[2][2] = RichText::plain("MSB-0001xC");
        t.cells[3][0] = RichText::plain("Graphite Phosphor Bronze Bushes");
        let mins = t.column_mins();
        let h = t.text_height;
        let bold = rich::CharStyle { bold: true, ..Default::default() };
        assert!(mins[1] >= rich::width("Quantity", &bold) * h);
        assert!(mins[2] >= rich::width("MSB-0001xC", &rich::CharStyle::default()) * h);
        // Narrowed as far as it goes: every column at least its minimum, the words whole.
        let (lo, hi) = t.rect();
        let narrow = t.set_fixed(Corner::TopRight).resize(Side::Left, [hi[0] - 1.0, lo[1]]).unwrap();
        for (c, m) in narrow.cols.iter().zip(&mins) {
            assert!(*c >= m - 1e-9, "{c} < {m}");
        }
        assert!((narrow.width() - mins.iter().sum::<f64>()).abs() < 1e-6);
        let g = table_graphics(&narrow, &ctx, false, None);
        let words: Vec<&str> = g.texts.iter().map(|x| x.piece.text.trim()).collect();
        for w in ["Quantity", "MSB-0001xC", "Graphite", "Phosphor", "Bronze", "Bushes"] {
            assert!(words.iter().any(|x| x.split_whitespace().any(|y| y == w)), "{w} split: {words:?}");
        }
        // A column line stops at the neighbours' minimums.
        let x = narrow.rect().0[0] + narrow.cols[0] + narrow.cols[1] + narrow.cols[2] - 50.0;
        assert!(narrow.set_column_edge(3, x).is_err() || narrow.set_column_edge(3, x).unwrap().cols[2] >= mins[2] - 1e-9);
        // A long word typed in a narrow column widens it.
        let typed = fit_rows(&table().set_cell((2, 0), RichText::plain("Supercalifragilistic")).unwrap(), &ctx);
        assert!(typed.cols[0] >= rich::width("Supercalifragilistic", &rich::CharStyle::default()) * h);
    }

    #[test]
    fn scaling_columns_keeps_minimums_and_the_total() {
        let cols = scale_columns(&[20.0, 80.0, 20.0, 30.0, 95.0], &[13.0, 20.0, 21.0, 29.0, 20.0], 150.0);
        assert!((cols.iter().sum::<f64>() - 150.0).abs() < 1e-9);
        assert_eq!((cols[0], cols[2], cols[3]), (13.0, 21.0, 29.0));
        assert!((cols[1] / cols[4] - 80.0 / 95.0).abs() < 1e-9);
    }

    #[test]
    fn resizing_keeps_the_fixed_corner() {
        for fixed in Corner::ALL {
            let t = table().set_fixed(fixed);
            let before = t.corner(fixed);
            let (lo, hi) = t.rect();
            for side in t.free_sides() {
                let to = match side {
                    Side::Left => [lo[0] - 30.0, 0.0],
                    Side::Right => [hi[0] + 30.0, 0.0],
                    Side::Top => [0.0, hi[1] + 12.0],
                    Side::Bottom => [0.0, lo[1] - 12.0],
                };
                let r = t.resize(side, to).unwrap();
                let after = r.corner(fixed);
                assert!((after[0] - before[0]).abs() < 1e-9 && (after[1] - before[1]).abs() < 1e-9, "{fixed:?} {side:?}");
                // The dragged side followed the pointer.
                let (rlo, rhi) = r.rect();
                match side {
                    Side::Left => assert!((rlo[0] - to[0]).abs() < 1e-9),
                    Side::Right => assert!((rhi[0] - to[0]).abs() < 1e-9),
                    Side::Top => assert!((rhi[1] - to[1]).abs() < 1e-9),
                    Side::Bottom => assert!((rlo[1] - to[1]).abs() < 1e-9),
                }
            }
            // The fixed corner's sides don't resize.
            let fixed_side = if t.free_sides()[0] == Side::Right { Side::Left } else { Side::Right };
            assert!(t.resize(fixed_side, [0.0, 0.0]).is_err());
        }
        // Changing the fixed corner keeps the table in place.
        let t = table();
        let moved = t.set_fixed(Corner::BottomRight);
        assert_eq!(moved.rect(), t.rect());
        assert_eq!(moved.at, t.corner(Corner::BottomRight));
    }
}
