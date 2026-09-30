//! Rich text for notes and table cells (P3C.4, D9.3, D9.4): paragraphs of styled spans, with
//! property fields that resolve when the text is drawn, an editing model and a layout.
//!
//! # Model
//! A [`RichText`] is a list of [`Paragraph`]s (alignment, bulleted or numbered list), each a list
//! of [`Span`]s: plain text or a [`Field`], with a [`CharStyle`] (bold, italic, underline,
//! strikethrough, text height). Hole symbols (⌴ ⌵ ↧), Ø, ±, ° and the other symbols of the
//! symbol menu are ordinary characters; the three Inter lacks are drawn as vector strokes like
//! the dimensions' ([`crate::annotation::symbol_strokes`]).
//!
//! # Parametric text (D9.4, X3)
//! A [`Field`] reads a property of the sheet's referenced part or assembly ([`RefProp`], through
//! the [`ReferenceProperties`] trait, so the property model of P3B.6 can plug in) or of the
//! drawing itself ([`DrawingProp`]: sheet scale, name, number, date…), in a text case and, for
//! dates, a date format. An undefined property shows [`crate::title_block::DASHES`]. Fields are
//! resolved each time the text is laid out, so a note follows the property and the sheet scale.
//!
//! # Editing
//! [`Editor`] edits a flat copy of the text (one item per character, field or paragraph break,
//! each with its style) with a caret and a selection: typing, deleting, moving, and toggling
//! styles on the selection or, with none, on what is typed next.
//!
//! # Layout
//! [`layout`] measures every character in the Inter face the app draws it with ([`FACE_REGULAR`],
//! [`FACE_BOLD`], Inter Italic) and breaks paragraphs at spaces to a wrap width. Coordinates are
//! the text box's: x right from its left edge, y up from its top edge (so lines have negative y).
//! It returns the pieces to draw (runs of one style on one line), the strokes of symbols,
//! underlines and strikethroughs, the caret stops (one per flat position) and the box size.

use std::cell::RefCell;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::annotation::{Symbol, symbol_strokes};
use crate::standard::{Projection, Scale, SheetSize};
use crate::style::DateFormat;
use crate::template::DrawingUnits;
use crate::title_block::{DASHES, ReferenceProps, TitleProps};

pub type P2 = [f64; 2];

fn is_false(b: &bool) -> bool {
    !*b
}

// ---------------------------------------------------------------------------------------------
// Model

/// How characters look.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct CharStyle {
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Cap height (sheet mm); `None`: the note's or cell's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
}

/// A style toggle of the note toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Attr {
    Bold,
    Italic,
    Underline,
    Strike,
}

impl CharStyle {
    pub fn get(&self, a: Attr) -> bool {
        match a {
            Attr::Bold => self.bold,
            Attr::Italic => self.italic,
            Attr::Underline => self.underline,
            Attr::Strike => self.strike,
        }
    }

    pub fn set(&mut self, a: Attr, on: bool) {
        match a {
            Attr::Bold => self.bold = on,
            Attr::Italic => self.italic = on,
            Attr::Underline => self.underline = on,
            Attr::Strike => self.strike = on,
        }
    }
}

/// Paragraph alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A paragraph's list style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ListKind {
    #[default]
    None,
    Bullet,
    Numbered,
}

/// A property of the sheet's referenced part or assembly (Insert sheet reference property).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RefProp {
    Name,
    Description,
    PartNumber,
    Revision,
    Material,
    Vendor,
    Project,
}

impl RefProp {
    pub const ALL: [RefProp; 7] = [
        RefProp::Name,
        RefProp::Description,
        RefProp::PartNumber,
        RefProp::Revision,
        RefProp::Material,
        RefProp::Vendor,
        RefProp::Project,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RefProp::Name => "Name",
            RefProp::Description => "Description",
            RefProp::PartNumber => "Part number",
            RefProp::Revision => "Revision",
            RefProp::Material => "Material",
            RefProp::Vendor => "Vendor",
            RefProp::Project => "Project",
        }
    }
}

/// A property of the drawing (Insert drawing property).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DrawingProp {
    SheetScale,
    SheetName,
    SheetNumber,
    SheetCount,
    /// "2 of 3".
    SheetOf,
    SheetSize,
    DrawingName,
    Date,
    Projection,
    Units,
    DrawnBy,
    CheckedBy,
    ApprovedBy,
    Company,
}

impl DrawingProp {
    pub const ALL: [DrawingProp; 14] = [
        DrawingProp::SheetScale,
        DrawingProp::SheetName,
        DrawingProp::SheetNumber,
        DrawingProp::SheetCount,
        DrawingProp::SheetOf,
        DrawingProp::SheetSize,
        DrawingProp::DrawingName,
        DrawingProp::Date,
        DrawingProp::Projection,
        DrawingProp::Units,
        DrawingProp::DrawnBy,
        DrawingProp::CheckedBy,
        DrawingProp::ApprovedBy,
        DrawingProp::Company,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DrawingProp::SheetScale => "Sheet scale",
            DrawingProp::SheetName => "Sheet name",
            DrawingProp::SheetNumber => "Sheet number",
            DrawingProp::SheetCount => "Number of sheets",
            DrawingProp::SheetOf => "Sheet n of m",
            DrawingProp::SheetSize => "Sheet size",
            DrawingProp::DrawingName => "Drawing name",
            DrawingProp::Date => "Date",
            DrawingProp::Projection => "Projection",
            DrawingProp::Units => "Units",
            DrawingProp::DrawnBy => "Drawn by",
            DrawingProp::CheckedBy => "Checked by",
            DrawingProp::ApprovedBy => "Approved by",
            DrawingProp::Company => "Company",
        }
    }
}

/// Which property a field reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Prop {
    Reference(RefProp),
    Drawing(DrawingProp),
}

/// A field's text case (D9.4 "text format").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextCase {
    #[default]
    AsIs,
    Upper,
    Lower,
    Title,
}

impl TextCase {
    pub const ALL: [TextCase; 4] = [TextCase::AsIs, TextCase::Upper, TextCase::Lower, TextCase::Title];

    pub fn label(self) -> &'static str {
        match self {
            TextCase::AsIs => "As is",
            TextCase::Upper => "UPPERCASE",
            TextCase::Lower => "lowercase",
            TextCase::Title => "Title Case",
        }
    }

    pub fn apply(self, s: &str) -> String {
        match self {
            TextCase::AsIs => s.to_string(),
            TextCase::Upper => s.to_uppercase(),
            TextCase::Lower => s.to_lowercase(),
            TextCase::Title => s
                .split(' ')
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        Some(f) => f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

/// A property field in a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Field {
    pub prop: Prop,
    #[serde(default)]
    pub case: TextCase,
    /// For dates: the format (`None`: the drawing properties').
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<DateFormat>,
}

impl Field {
    pub fn new(prop: Prop) -> Self {
        Self { prop, case: TextCase::AsIs, date: None }
    }

    /// The field's chip label in the editor ("Part: Name", "Sheet: Scale").
    pub fn label(&self) -> String {
        match self.prop {
            Prop::Reference(p) => format!("Part: {}", p.label()),
            Prop::Drawing(p) => format!("Drawing: {}", p.label()),
        }
    }
}

/// A piece of a paragraph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Inline {
    Text(String),
    Field(Field),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub content: Inline,
    #[serde(default)]
    pub style: CharStyle,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Paragraph {
    #[serde(default)]
    pub spans: Vec<Span>,
    #[serde(default)]
    pub align: HAlign,
    #[serde(default)]
    pub list: ListKind,
}

/// Styled text: at least one paragraph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RichText {
    pub paragraphs: Vec<Paragraph>,
}

impl Default for RichText {
    fn default() -> Self {
        Self { paragraphs: vec![Paragraph::default()] }
    }
}

impl RichText {
    /// Unstyled text; each line a paragraph.
    pub fn plain(s: &str) -> Self {
        Self {
            paragraphs: s
                .split('\n')
                .map(|l| Paragraph {
                    spans: if l.is_empty() {
                        Vec::new()
                    } else {
                        vec![Span { content: Inline::Text(l.to_string()), style: CharStyle::default() }]
                    },
                    ..Default::default()
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.paragraphs.iter().all(|p| {
            p.spans.iter().all(|s| match &s.content {
                Inline::Text(t) => t.is_empty(),
                Inline::Field(_) => false,
            })
        })
    }

    /// The literal text, fields left out, paragraphs joined by newlines (BOM cells, tests).
    pub fn plain_text(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| {
                p.spans
                    .iter()
                    .filter_map(|s| match &s.content {
                        Inline::Text(t) => Some(t.as_str()),
                        Inline::Field(_) => None,
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The text with fields resolved, paragraphs joined by newlines (for tests and search).
    pub fn resolved(&self, ctx: &FieldContext) -> String {
        self.paragraphs
            .iter()
            .map(|p| {
                p.spans
                    .iter()
                    .map(|s| match &s.content {
                        Inline::Text(t) => t.clone(),
                        Inline::Field(f) => resolve_field(f, ctx),
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every character's (and field's) style set to `f(style)`, and every paragraph's with `p`.
    pub fn restyle(&mut self, f: impl Fn(&mut CharStyle), p: impl Fn(&mut Paragraph)) {
        for para in &mut self.paragraphs {
            for s in &mut para.spans {
                f(&mut s.style);
            }
            p(para);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Properties

/// The referenced part's or assembly's properties, as fields read them. [`ReferenceProps`]
/// implements it; since P3C.5 `cadrs_core::drawing_export::reference_props` fills it from the
/// property model of P3B.6 (`cadrs_core::properties`: Name, Part number, Description, Revision,
/// Material, Vendor), and an undefined property reads as dashes.
pub trait ReferenceProperties {
    /// The property's value, or `None` when it isn't defined.
    fn reference_property(&self, p: RefProp) -> Option<String>;
}

impl ReferenceProperties for ReferenceProps {
    fn reference_property(&self, p: RefProp) -> Option<String> {
        match p {
            RefProp::Name => self.name.clone(),
            RefProp::Description => self.description.clone(),
            RefProp::PartNumber => self.part_number.clone(),
            RefProp::Revision => self.revision.clone(),
            RefProp::Material => self.material.clone(),
            RefProp::Vendor => self.vendor.clone(),
            RefProp::Project => None,
        }
    }
}

/// What drawing properties read.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingContext {
    pub drawing_name: String,
    pub sheet_name: String,
    /// 0-based.
    pub sheet_index: usize,
    pub sheet_count: usize,
    pub scale: Scale,
    pub size: SheetSize,
    pub projection: Projection,
    pub units: DrawingUnits,
    /// Today (year, month, day).
    pub date: Option<(i32, u32, u32)>,
    pub date_format: DateFormat,
    pub title: TitleProps,
}

impl Default for DrawingContext {
    fn default() -> Self {
        Self {
            drawing_name: String::new(),
            sheet_name: "Sheet1".into(),
            sheet_index: 0,
            sheet_count: 1,
            scale: Scale::default(),
            size: SheetSize::AnsiA,
            projection: Projection::Third,
            units: DrawingUnits::Inch,
            date: None,
            date_format: DateFormat::Iso,
            title: TitleProps::default(),
        }
    }
}

/// Everything fields read.
#[derive(Clone, Copy)]
pub struct FieldContext<'a> {
    pub reference: &'a dyn ReferenceProperties,
    pub drawing: &'a DrawingContext,
}

/// A date in a format.
pub fn format_date((y, m, d): (i32, u32, u32), f: DateFormat) -> String {
    match f {
        DateFormat::Iso => format!("{y:04}-{m:02}-{d:02}"),
        DateFormat::Us => format!("{m:02}/{d:02}/{y:04}"),
        DateFormat::European => format!("{d:02}.{m:02}.{y:04}"),
    }
}

/// A field's text now: the property in the field's case, or [`DASHES`] when it isn't defined.
pub fn resolve_field(f: &Field, ctx: &FieldContext) -> String {
    let d = ctx.drawing;
    let value: Option<String> = match f.prop {
        Prop::Reference(p) => ctx.reference.reference_property(p),
        Prop::Drawing(p) => match p {
            DrawingProp::SheetScale => Some(d.scale.label()),
            DrawingProp::SheetName => Some(d.sheet_name.clone()),
            DrawingProp::SheetNumber => Some((d.sheet_index + 1).to_string()),
            DrawingProp::SheetCount => Some(d.sheet_count.max(1).to_string()),
            DrawingProp::SheetOf => Some(format!("{} of {}", d.sheet_index + 1, d.sheet_count.max(1))),
            DrawingProp::SheetSize => Some(d.size.letter().to_string()),
            DrawingProp::DrawingName => Some(d.drawing_name.clone()),
            DrawingProp::Date => d.date.map(|dt| format_date(dt, f.date.unwrap_or(d.date_format))),
            DrawingProp::Projection => Some(d.projection.label().to_string()),
            DrawingProp::Units => Some(d.units.label().to_string()),
            DrawingProp::DrawnBy => d.title.drawn_by.clone(),
            DrawingProp::CheckedBy => d.title.checked_by.clone(),
            DrawingProp::ApprovedBy => d.title.approved_by.clone(),
            DrawingProp::Company => d.title.company.clone(),
        },
    };
    match value.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(v) => f.case.apply(v),
        None => DASHES.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Editing

/// One position of the flat text.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Char(char),
    Field(Field),
    /// The end of a paragraph (the next one starts after it).
    Break,
}

/// A paragraph's own settings.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParaStyle {
    pub align: HAlign,
    pub list: ListKind,
}

/// The text as a flat list of items, with each paragraph's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Flat {
    pub items: Vec<(Item, CharStyle)>,
    /// One per paragraph (breaks + 1).
    pub paras: Vec<ParaStyle>,
}

impl Flat {
    pub fn of(t: &RichText) -> Self {
        let mut items = Vec::new();
        let mut paras = Vec::new();
        for (i, p) in t.paragraphs.iter().enumerate() {
            if i > 0 {
                items.push((Item::Break, CharStyle::default()));
            }
            paras.push(ParaStyle { align: p.align, list: p.list });
            for s in &p.spans {
                match &s.content {
                    Inline::Text(txt) => items.extend(txt.chars().map(|c| (Item::Char(c), s.style))),
                    Inline::Field(f) => items.push((Item::Field(*f), s.style)),
                }
            }
        }
        if paras.is_empty() {
            paras.push(ParaStyle::default());
        }
        Self { items, paras }
    }

    pub fn to_rich(&self) -> RichText {
        let mut paragraphs = Vec::new();
        let mut cur = Paragraph::default();
        let mut k = 0;
        let apply = |p: &mut Paragraph, s: Option<&ParaStyle>| {
            if let Some(s) = s {
                p.align = s.align;
                p.list = s.list;
            }
        };
        apply(&mut cur, self.paras.first());
        for (item, style) in &self.items {
            match item {
                Item::Break => {
                    paragraphs.push(std::mem::take(&mut cur));
                    k += 1;
                    apply(&mut cur, self.paras.get(k));
                }
                Item::Char(c) => match cur.spans.last_mut() {
                    Some(Span { content: Inline::Text(t), style: s }) if s == style => t.push(*c),
                    _ => cur.spans.push(Span { content: Inline::Text(c.to_string()), style: *style }),
                },
                Item::Field(f) => cur.spans.push(Span { content: Inline::Field(*f), style: *style }),
            }
        }
        paragraphs.push(cur);
        RichText { paragraphs }
    }

    /// The paragraph of flat position `pos`.
    pub fn para_of(&self, pos: usize) -> usize {
        self.items[..pos.min(self.items.len())].iter().filter(|(i, _)| *i == Item::Break).count()
    }

    /// The flat range of paragraph `p` (without its break).
    pub fn para_range(&self, p: usize) -> std::ops::Range<usize> {
        let mut start = 0;
        let mut k = 0;
        for (i, (item, _)) in self.items.iter().enumerate() {
            if *item == Item::Break {
                if k == p {
                    return start..i;
                }
                k += 1;
                start = i + 1;
            }
        }
        start..self.items.len()
    }
}

/// Edits a rich text: a caret and a selection (from `anchor` to `caret`) in its flat form.
#[derive(Debug, Clone, PartialEq)]
pub struct Editor {
    pub flat: Flat,
    pub caret: usize,
    pub anchor: usize,
    /// What typing inserts (the style before the caret, changed by the toggles).
    pub style: CharStyle,
}

impl Editor {
    /// Edits `t` with the caret at its end.
    pub fn new(t: &RichText) -> Self {
        let flat = Flat::of(t);
        let n = flat.items.len();
        let mut e = Self { flat, caret: n, anchor: n, style: CharStyle::default() };
        e.style = e.style_at(n);
        e
    }

    pub fn text(&self) -> RichText {
        self.flat.to_rich()
    }

    pub fn len(&self) -> usize {
        self.flat.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.flat.items.is_empty()
    }

    /// The selection, in order.
    pub fn selection(&self) -> std::ops::Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    fn style_at(&self, pos: usize) -> CharStyle {
        let before = self.flat.items[..pos].iter().rev().find(|(i, _)| *i != Item::Break);
        let after = self.flat.items[pos..].iter().find(|(i, _)| *i != Item::Break);
        before.or(after).map(|(_, s)| *s).unwrap_or_default()
    }

    /// Moves the caret to `pos` (keeping the anchor when `extend`).
    pub fn set_caret(&mut self, pos: usize, extend: bool) {
        self.caret = pos.min(self.len());
        if !extend {
            self.anchor = self.caret;
        }
        self.style = self.style_at(self.caret);
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.len();
    }

    fn delete_selection(&mut self) -> bool {
        let r = self.selection();
        if r.is_empty() {
            return false;
        }
        let (p0, p1) = (self.flat.para_of(r.start), self.flat.para_of(r.end));
        self.flat.items.drain(r.clone());
        // Merged paragraphs keep the first one's settings.
        if p1 > p0 {
            self.flat.paras.drain(p0 + 1..=p1);
        }
        self.caret = r.start;
        self.anchor = r.start;
        true
    }

    fn insert_item(&mut self, item: Item) {
        self.delete_selection();
        let st = self.style;
        if item == Item::Break {
            let p = self.flat.para_of(self.caret);
            let ps = self.flat.paras[p];
            self.flat.paras.insert(p + 1, ps);
        }
        self.flat.items.insert(self.caret, (item, st));
        self.caret += 1;
        self.anchor = self.caret;
    }

    /// Types text (newlines start paragraphs).
    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '\n' => self.insert_item(Item::Break),
                '\r' => {}
                c if c.is_control() => {}
                c => self.insert_item(Item::Char(c)),
            }
        }
    }

    pub fn insert_field(&mut self, f: Field) {
        self.insert_item(Item::Field(f));
    }

    /// Enter: a new paragraph with this one's settings.
    pub fn newline(&mut self) {
        self.insert_item(Item::Break);
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() || self.caret == 0 {
            return;
        }
        self.anchor = self.caret - 1;
        self.delete_selection();
        self.style = self.style_at(self.caret);
    }

    pub fn delete(&mut self) {
        if self.delete_selection() || self.caret >= self.len() {
            return;
        }
        self.anchor = self.caret + 1;
        self.delete_selection();
    }

    pub fn left(&mut self, extend: bool) {
        let r = self.selection();
        let to = if !extend && !r.is_empty() { r.start } else { self.caret.saturating_sub(1) };
        self.set_caret(to, extend);
    }

    pub fn right(&mut self, extend: bool) {
        let r = self.selection();
        let to = if !extend && !r.is_empty() { r.end } else { self.caret + 1 };
        self.set_caret(to, extend);
    }

    /// Home and End: the start or end of the caret's paragraph.
    pub fn home(&mut self, extend: bool) {
        let r = self.flat.para_range(self.flat.para_of(self.caret));
        self.set_caret(r.start, extend);
    }

    pub fn end(&mut self, extend: bool) {
        let r = self.flat.para_range(self.flat.para_of(self.caret));
        self.set_caret(r.end, extend);
    }

    /// Whether the selection (or, with none, the typing style) has `a`.
    pub fn has(&self, a: Attr) -> bool {
        let r = self.selection();
        let mut chars = self.flat.items[r].iter().filter(|(i, _)| *i != Item::Break).peekable();
        if chars.peek().is_none() {
            return self.style.get(a);
        }
        chars.all(|(_, s)| s.get(a))
    }

    /// Bold, italic, underline or strikethrough on or off: on the selection when there is one,
    /// else for what is typed next.
    pub fn toggle(&mut self, a: Attr) {
        let on = !self.has(a);
        let r = self.selection();
        for (_, s) in &mut self.flat.items[r] {
            s.set(a, on);
        }
        self.style.set(a, on);
    }

    /// The text height of the selection (or of what is typed next); `None` for the default.
    pub fn height(&self) -> Option<f64> {
        let r = self.selection();
        self.flat.items[r]
            .iter()
            .find(|(i, _)| *i != Item::Break)
            .map_or(self.style.height, |(_, s)| s.height)
    }

    pub fn set_height(&mut self, h: Option<f64>) {
        let r = self.selection();
        for (_, s) in &mut self.flat.items[r] {
            s.height = h;
        }
        self.style.height = h;
    }

    /// The paragraphs the selection touches.
    fn paras(&self) -> std::ops::RangeInclusive<usize> {
        let r = self.selection();
        self.flat.para_of(r.start)..=self.flat.para_of(r.end)
    }

    pub fn para_style(&self) -> ParaStyle {
        self.flat.paras[self.flat.para_of(self.caret).min(self.flat.paras.len() - 1)]
    }

    pub fn set_align(&mut self, a: HAlign) {
        for p in self.paras() {
            self.flat.paras[p].align = a;
        }
    }

    /// A list style on the selected paragraphs, or off when they already have it.
    pub fn toggle_list(&mut self, l: ListKind) {
        let all = self.paras().all(|p| self.flat.paras[p].list == l);
        for p in self.paras() {
            self.flat.paras[p].list = if all { ListKind::None } else { l };
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Measuring

/// The weight regular text is drawn in: Inter Medium (Bevy blends glyph coverage in linear
/// space, so Regular draws grey at sheet sizes).
pub const FACE_REGULAR: u16 = 500;
/// The weight bold text is drawn in: Inter ExtraBold (clearly heavier than the Medium body).
pub const FACE_BOLD: u16 = 800;

/// The face a style is drawn with: (weight, italic). Bold italic uses the bundled ExtraBold
/// Italic; plain italic Inter Italic (regular weight, there is no Medium Italic).
pub fn face(style: &CharStyle) -> (u16, bool) {
    (if style.bold { FACE_BOLD } else { FACE_REGULAR }, style.italic)
}

thread_local! {
    static ADVANCES: RefCell<HashMap<(char, u16, bool), f64>> = RefCell::new(HashMap::new());
}

/// A character's advance in cap heights, in the face `style` draws with.
pub fn advance(c: char, style: &CharStyle) -> f64 {
    if let Some(s) = Symbol::of(c) {
        return s.width();
    }
    let (w, it) = face(style);
    let key = (c, w, it && !style.bold);
    if let Some(a) = ADVANCES.with(|m| m.borrow().get(&key).copied()) {
        return a;
    }
    // Bold italic measures with the upright ExtraBold (its advances are within a few per cent).
    let data = cadrs_sketch::text::inter_data(w, key.2);
    let a = ttf_parser::Face::parse(data, 0)
        .ok()
        .map(|f| {
            let cap = f.capital_height().filter(|c| *c > 0).map_or(f.units_per_em() as f64 * 0.727, |c| c as f64);
            let space = f.glyph_index(' ').and_then(|g| f.glyph_hor_advance(g)).unwrap_or(f.units_per_em() / 4) as f64;
            let adv = f.glyph_index(c).and_then(|g| f.glyph_hor_advance(g)).map_or(space, |a| a as f64);
            adv / cap
        })
        .unwrap_or(0.6);
    ADVANCES.with(|m| m.borrow_mut().insert(key, a));
    a
}

/// The width of `s` (cap heights) in `style`'s face.
pub fn width(s: &str, style: &CharStyle) -> f64 {
    s.chars().map(|c| advance(c, style)).sum()
}

// ---------------------------------------------------------------------------------------------
// Layout

/// A run of one style on one line: its left end at the middle of its capitals (as the
/// annotations' [`crate::annotation::PlacedText`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub pos: P2,
    /// Cap height (mm).
    pub height: f64,
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// A field's value (shaded while editing).
    pub field: bool,
}

/// A caret position: x, the baseline's y and the line's cap height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stop {
    pub x: f64,
    pub baseline: f64,
    pub height: f64,
}

/// A laid-out rich text (the text box's coordinates, see the module docs).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RichLayout {
    pub pieces: Vec<Piece>,
    /// Symbols, underlines and strikethroughs.
    pub strokes: Vec<Vec<P2>>,
    /// Field boxes (min, max), shaded while editing.
    pub fields: Vec<(P2, P2)>,
    /// One per flat position (0..=len).
    pub stops: Vec<Stop>,
    /// The box: width and height.
    pub width: f64,
    pub height: f64,
}

/// Line spacing, in cap heights.
pub const LINE: f64 = 1.6;
/// Space between a bullet or number and the text, and the list indent, in cap heights.
const INDENT: f64 = 1.6;

/// A laid-out line: its atoms, height, alignment, indent and list marker.
type OutLine = (Vec<Atom>, f64, HAlign, f64, Option<(String, CharStyle)>);

struct Atom {
    /// Flat index (`None` for a list marker).
    index: usize,
    text: String,
    style: CharStyle,
    h: f64,
    w: f64,
    field: bool,
    space: bool,
}

/// Lays out `text` with default cap height `h`, wrapped at `wrap` (mm) when given.
pub fn layout(text: &RichText, ctx: &FieldContext, h: f64, wrap: Option<f64>) -> RichLayout {
    layout_with(text, ctx, h, wrap, true)
}

/// As [`layout`]; with `break_words` false a word longer than the line stays whole (it runs
/// past the wrap width) instead of breaking anywhere: table cells, which size their columns to
/// their longest word ([`crate::table::Table::column_mins`]).
pub fn layout_with(text: &RichText, ctx: &FieldContext, h: f64, wrap: Option<f64>, break_words: bool) -> RichLayout {
    let flat = Flat::of(text);
    let mut out = RichLayout::default();
    let mut stops: Vec<Option<Stop>> = vec![None; flat.items.len() + 1];
    let mut y_top = 0.0; // top of the current line
    let mut widest: f64 = 0.0;
    let mut number = 0;
    let mut lines_out: Vec<OutLine> = Vec::new();
    for (pi, para) in flat.paras.iter().enumerate() {
        let range = flat.para_range(pi);
        number = if para.list == ListKind::Numbered { number + 1 } else { 0 };
        let atoms: Vec<Atom> = flat.items[range.clone()]
            .iter()
            .enumerate()
            .map(|(k, (item, st))| {
                let ah = st.height.unwrap_or(h);
                let (text, field) = match item {
                    Item::Char(c) => (c.to_string(), false),
                    Item::Field(f) => (resolve_field(f, ctx), true),
                    Item::Break => (String::new(), false),
                };
                let w = width(&text, st) * ah;
                Atom { index: range.start + k, space: text == " ", text, style: *st, h: ah, w, field }
            })
            .collect();
        let marker = match para.list {
            ListKind::None => None,
            ListKind::Bullet => Some("•".to_string()),
            ListKind::Numbered => Some(format!("{number}.")),
        };
        let first_style = atoms.first().map(|a| a.style).unwrap_or_default();
        let indent = if marker.is_some() { INDENT * first_style.height.unwrap_or(h) } else { 0.0 };
        let room = wrap.map(|w| (w - indent).max(h));
        // Greedy breaking at spaces (a word longer than the line breaks anywhere, unless
        // `break_words` is off: then it stays whole).
        let mut lines: Vec<Vec<Atom>> = vec![Vec::new()];
        let mut line_w = 0.0;
        let mut last_space: Option<usize> = None;
        for a in atoms {
            let cur = lines.last_mut().unwrap();
            if let Some(room) = room
                && !a.space
                && line_w + a.w > room + 1e-9
                && !cur.is_empty()
                && (break_words || last_space.is_some())
            {
                let carry: Vec<Atom> = match last_space {
                    Some(k) if k + 1 < cur.len() => cur.drain(k + 1..).collect(),
                    Some(_) => Vec::new(),
                    None => Vec::new(),
                };
                line_w = carry.iter().map(|a| a.w).sum();
                lines.push(carry);
                last_space = None;
            }
            let cur = lines.last_mut().unwrap();
            if a.space {
                last_space = Some(cur.len());
            }
            line_w += a.w;
            cur.push(a);
        }
        for (li, line) in lines.into_iter().enumerate() {
            let lh = line.iter().map(|a| a.h).fold(first_style.height.unwrap_or(h), f64::max);
            let m = if li == 0 { marker.clone().map(|m| (m, first_style)) } else { None };
            lines_out.push((line, lh, para.align, indent, m));
        }
        // The break's own stop is placed at the end of the paragraph's last line (below).
    }
    // Widths, for alignment without a wrap width.
    let line_width = |l: &[Atom]| {
        // Trailing spaces don't count.
        let n = l.iter().rposition(|a| !a.space).map_or(0, |k| k + 1);
        l[..n].iter().map(|a| a.w).sum::<f64>()
    };
    for (l, _, _, indent, _) in &lines_out {
        widest = widest.max(indent + line_width(l));
    }
    let box_w = wrap.unwrap_or(widest).max(widest.min(wrap.unwrap_or(f64::MAX)));
    let mut para_end_stop: Vec<(usize, Stop)> = Vec::new();
    for (line, lh, align, indent, marker) in &lines_out {
        let baseline = y_top - lh;
        let lw = line_width(line);
        let room = box_w - indent;
        let x0 = indent
            + match align {
                HAlign::Left => 0.0,
                HAlign::Center => (room - lw).max(0.0) / 2.0,
                HAlign::Right => (room - lw).max(0.0),
            };
        if let Some((m, st)) = marker {
            let mh = st.height.unwrap_or(*lh);
            out.pieces.push(Piece {
                pos: [0.0, baseline + mh / 2.0],
                height: mh,
                text: m.clone(),
                bold: st.bold,
                italic: false,
                field: false,
            });
        }
        let mut x = x0;
        let mut i = 0;
        while i < line.len() {
            let a = &line[i];
            // A run of atoms with one style (a field alone).
            let mut j = i + 1;
            while j < line.len() && !a.field && !line[j].field && line[j].style == a.style {
                j += 1;
            }
            let run = &line[i..j];
            let rw: f64 = run.iter().map(|r| r.w).sum();
            let ah = a.h;
            // Text runs without the symbols Inter lacks; the symbols as strokes.
            let mut px = x;
            let mut buf = String::new();
            let mut buf_x = x;
            for r in run {
                stops[r.index] = Some(Stop { x: px, baseline, height: ah });
                for c in r.text.chars() {
                    let cw = advance(c, &r.style) * ah;
                    if let Some(sym) = Symbol::of(c) {
                        if !buf.is_empty() {
                            out.pieces.push(Piece {
                                pos: [buf_x, baseline + ah / 2.0],
                                height: ah,
                                text: std::mem::take(&mut buf),
                                bold: a.style.bold,
                                italic: a.style.italic,
                                field: a.field,
                            });
                        }
                        out.strokes.extend(symbol_strokes(sym, [px, baseline], ah));
                        buf_x = px + cw;
                    } else {
                        if buf.is_empty() {
                            buf_x = px;
                        }
                        buf.push(c);
                    }
                    px += cw;
                }
            }
            if !buf.trim().is_empty() {
                out.pieces.push(Piece {
                    pos: [buf_x, baseline + ah / 2.0],
                    height: ah,
                    text: buf,
                    bold: a.style.bold,
                    italic: a.style.italic,
                    field: a.field,
                });
            }
            // Decorations, under the run's visible width (not its trailing spaces).
            let vis = if j == line.len() {
                let n = run.iter().rposition(|a| !a.space).map_or(0, |k| k + 1);
                run[..n].iter().map(|r| r.w).sum::<f64>()
            } else {
                rw
            };
            if a.style.underline && vis > 0.0 {
                let y = baseline - 0.18 * ah;
                out.strokes.push(vec![[x, y], [x + vis, y]]);
            }
            if a.style.strike && vis > 0.0 {
                let y = baseline + 0.45 * ah;
                out.strokes.push(vec![[x, y], [x + vis, y]]);
            }
            if a.field {
                out.fields.push(([x - 0.1 * ah, baseline - 0.3 * ah], [x + rw + 0.1 * ah, baseline + 1.25 * ah]));
            }
            x += rw;
            i = j;
        }
        // The stop after the line's last atom.
        if let Some(last) = line.last() {
            let end = last.index + 1;
            para_end_stop.push((end, Stop { x, baseline, height: *lh }));
        } else {
            // An empty paragraph: its stop is where its text would start.
            para_end_stop.push((usize::MAX, Stop { x: x0, baseline, height: *lh }));
        }
        y_top -= lh * LINE;
    }
    // Stops at line ends and paragraph breaks: a position without a stop takes the end of the
    // line before it; an empty paragraph's the start of its line.
    let mut empty = para_end_stop.iter().filter(|(k, _)| *k == usize::MAX).map(|(_, s)| *s);
    for (k, s) in &para_end_stop {
        if *k != usize::MAX && *k < stops.len() && stops[*k].is_none() {
            stops[*k] = Some(*s);
        }
    }
    // Walk the paragraphs to give empty ones their stop.
    for pi in 0..flat.paras.len() {
        let r = flat.para_range(pi);
        if r.is_empty()
            && let Some(s) = empty.next()
        {
            stops[r.start] = Some(s);
        }
    }
    let first = Stop { x: 0.0, baseline: -h, height: h };
    let mut last = first;
    out.stops = stops
        .into_iter()
        .map(|s| {
            if let Some(s) = s {
                last = s;
            }
            s.unwrap_or(last)
        })
        .collect();
    out.width = box_w.max(0.0);
    let last_h = lines_out.last().map_or(h, |l| l.1);
    out.height = (-y_top) - last_h * (LINE - 1.0);
    out
}

/// The caret stop nearest a point of the text box: on the nearest line, the nearest in x.
pub fn stop_at(l: &RichLayout, p: P2) -> usize {
    let line_d = |s: &Stop| {
        let mid = s.baseline + s.height / 2.0;
        ((p[1] - mid).abs() - s.height * 0.8).max(0.0)
    };
    let best_line = l.stops.iter().map(line_d).fold(f64::MAX, f64::min);
    l.stops
        .iter()
        .enumerate()
        .filter(|(_, s)| line_d(s) <= best_line + 1e-9)
        .min_by(|a, b| (a.1.x - p[0]).abs().total_cmp(&(b.1.x - p[0]).abs()))
        .map_or(0, |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with<'a>(r: &'a ReferenceProps, d: &'a DrawingContext) -> FieldContext<'a> {
        FieldContext { reference: r, drawing: d }
    }

    #[test]
    fn fields_resolve_and_undefined_show_dashes() {
        let r = ReferenceProps {
            name: Some("Universal Joint Flange".into()),
            material: Some("Steel".into()),
            description: Some("   ".into()),
            ..Default::default()
        };
        let d = DrawingContext {
            drawing_name: "Flange drawing".into(),
            sheet_name: "Sheet2".into(),
            sheet_index: 1,
            sheet_count: 3,
            scale: Scale::new(1, 2),
            date: Some((2026, 9, 28)),
            ..Default::default()
        };
        let c = ctx_with(&r, &d);
        let f = |p: Prop| resolve_field(&Field::new(p), &c);
        assert_eq!(f(Prop::Reference(RefProp::Name)), "Universal Joint Flange");
        assert_eq!(f(Prop::Reference(RefProp::Material)), "Steel");
        // Blank and undefined properties show dashes (D9.4).
        assert_eq!(f(Prop::Reference(RefProp::Description)), DASHES);
        assert_eq!(f(Prop::Reference(RefProp::PartNumber)), DASHES);
        assert_eq!(f(Prop::Reference(RefProp::Vendor)), DASHES);
        assert_eq!(f(Prop::Drawing(DrawingProp::DrawnBy)), DASHES);
        assert_eq!(f(Prop::Drawing(DrawingProp::SheetScale)), "1:2");
        assert_eq!(f(Prop::Drawing(DrawingProp::SheetOf)), "2 of 3");
        assert_eq!(f(Prop::Drawing(DrawingProp::SheetName)), "Sheet2");
        assert_eq!(f(Prop::Drawing(DrawingProp::Date)), "2026-09-28");
        // Formats.
        let mut upper = Field::new(Prop::Reference(RefProp::Name));
        upper.case = TextCase::Upper;
        assert_eq!(resolve_field(&upper, &c), "UNIVERSAL JOINT FLANGE");
        let mut date = Field::new(Prop::Drawing(DrawingProp::Date));
        date.date = Some(DateFormat::Us);
        assert_eq!(resolve_field(&date, &c), "09/28/2026");
        date.date = Some(DateFormat::European);
        assert_eq!(resolve_field(&date, &c), "28.09.2026");
        // No date known: dashes.
        let d2 = DrawingContext::default();
        assert_eq!(resolve_field(&date, &ctx_with(&r, &d2)), DASHES);
        // A dashed field fills in once the property is set.
        let r2 = ReferenceProps { part_number: Some("UJ-100".into()), ..r.clone() };
        assert_eq!(resolve_field(&Field::new(Prop::Reference(RefProp::PartNumber)), &ctx_with(&r2, &d)), "UJ-100");
    }

    #[test]
    fn editor_types_styles_and_round_trips() {
        let mut e = Editor::new(&RichText::default());
        e.insert_str("Hello world");
        // Select "world" and make it bold.
        e.set_caret(6, false);
        e.set_caret(11, true);
        e.toggle(Attr::Bold);
        assert!(e.has(Attr::Bold));
        e.set_caret(11, false);
        e.newline();
        e.toggle(Attr::Italic);
        e.insert_str("next");
        e.toggle_list(ListKind::Bullet);
        let t = e.text();
        assert_eq!(t.paragraphs.len(), 2);
        assert_eq!(t.paragraphs[0].spans.len(), 2);
        assert_eq!(t.paragraphs[0].spans[0].content, Inline::Text("Hello ".into()));
        assert!(t.paragraphs[0].spans[1].style.bold);
        assert_eq!(t.paragraphs[1].list, ListKind::Bullet);
        assert_eq!(t.paragraphs[0].list, ListKind::None);
        assert!(t.paragraphs[1].spans[0].style.italic && t.paragraphs[1].spans[0].style.bold);
        assert_eq!(Flat::of(&t).to_rich(), t);
        // Backspace across the paragraph break joins them, keeping the first's settings.
        let mut e2 = Editor::new(&t);
        e2.set_caret(12, false);
        e2.backspace();
        assert_eq!(e2.text().paragraphs.len(), 1);
        assert_eq!(e2.text().paragraphs[0].list, ListKind::None);
        // A field is one position.
        let mut e3 = Editor::new(&RichText::plain("Part "));
        e3.insert_field(Field::new(Prop::Reference(RefProp::Name)));
        assert_eq!(e3.len(), 6);
        e3.backspace();
        assert_eq!(e3.text(), RichText::plain("Part "));
    }

    #[test]
    fn a_long_word_stays_whole_without_word_breaking() {
        let r = ReferenceProps::default();
        let d = DrawingContext::default();
        let c = ctx_with(&r, &d);
        let t = RichText::plain("Quantity MSB-0001xC");
        let w = width("Qua", &CharStyle::default()) * 3.0;
        let broken = layout(&t, &c, 3.0, Some(w));
        assert!(broken.pieces.iter().all(|p| p.text.trim() != "Quantity"));
        let whole = layout_with(&t, &c, 3.0, Some(w), false);
        let texts: Vec<&str> = whole.pieces.iter().map(|p| p.text.trim()).collect();
        assert_eq!(texts, ["Quantity", "MSB-0001xC"]);
    }

    #[test]
    fn layout_wraps_aligns_and_places_stops() {
        let r = ReferenceProps::default();
        let d = DrawingContext::default();
        let c = ctx_with(&r, &d);
        let t = RichText::plain("ALL FILLETS R.06 UNLESS OTHERWISE SPECIFIED");
        let one = layout(&t, &c, 3.0, None);
        assert_eq!(one.stops.len(), t.resolved(&c).chars().count() + 1);
        let w = one.width;
        // Wrapped at a third of its width: more lines, each within the width.
        let wrapped = layout(&t, &c, 3.0, Some(w / 3.0));
        assert!(wrapped.height > 2.0 * one.height);
        for p in &wrapped.pieces {
            assert!(p.pos[0] + width(p.text.trim_end(), &CharStyle::default()) * 3.0 <= w / 3.0 + 1e-6, "{p:?}");
        }
        // Centred text sits in the middle of the wrap width.
        let mut centred = RichText::plain("AB");
        centred.paragraphs[0].align = HAlign::Center;
        let l = layout(&centred, &c, 3.0, Some(40.0));
        let tw = width("AB", &CharStyle::default()) * 3.0;
        assert!((l.pieces[0].pos[0] - (40.0 - tw) / 2.0).abs() < 1e-9);
        // Stops advance along the line.
        assert!(one.stops.windows(2).all(|s| s[1].x >= s[0].x - 1e-9));
        // Symbols are strokes, not text.
        let sym = layout(&RichText::plain("⌴Ø.438"), &c, 3.0, None);
        assert!(!sym.strokes.is_empty());
        assert!(sym.pieces.iter().all(|p| !p.text.contains('⌴')));
    }
}
