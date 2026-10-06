//! Schematic editing (GS3–GS9): placing symbols with automatic references, selecting by point
//! and box, moving (M: wires stay; G: wires attached to the moved pins follow), rotating,
//! deleting, wiring with automatic junctions, labels, page settings and fields, annotation.

use crate::graphics::{Text, TextStyle};
use crate::schematic::*;
use crate::symbol::{Field, Symbol, SymbolItem, fields};
use crate::units::{Bounds, Nm, Pt, SCHEMATIC_GRID, normalize_deg};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Something on a sheet that can be selected.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SchItem {
    Symbol(Uuid),
    /// One field of a placed symbol, selected on its own.
    Field(Uuid, String),
    Wire(Uuid),
    Bus(Uuid),
    Junction(Uuid),
    NoConnect(Uuid),
    Label(Uuid),
    Note(Uuid),
    Drawing(Uuid),
}

/// The nearest multiple of `grid`.
pub fn snap(p: Pt, grid: Nm) -> Pt {
    let s = |v: Nm| (v as f64 / grid as f64).round() as Nm * grid;
    Pt::new(s(p.x), s(p.y))
}

// ---------------------------------------------------------------------------------------------
// Placing and annotating

/// The reference prefix of a reference or template: "R?" → "R", "#PWR01" → "#PWR".
pub fn prefix(reference: &str) -> &str {
    reference.trim_end_matches(|c: char| c.is_ascii_digit() || c == '?')
}

/// Whether a reference still needs a number ("R?").
pub fn unannotated(reference: &str) -> bool {
    reference.ends_with('?') || !reference.ends_with(|c: char| c.is_ascii_digit())
}

fn number_of(reference: &str) -> Option<u32> {
    reference[prefix(reference).len()..].parse().ok()
}

/// The next free reference with `prefix` across all sheets: "R1", "R2", … ; power and flag
/// symbols ("#PWR", "#FLG") get two digits ("#PWR01").
pub fn next_reference(sch: &Schematic, prefix_: &str) -> String {
    let used: Vec<u32> = sch.sheets.iter().flat_map(|s| &s.symbols).filter(|s| prefix(s.reference()) == prefix_).filter_map(|s| number_of(s.reference())).collect();
    let n = (1..).find(|n| !used.contains(n)).unwrap();
    if prefix_.starts_with('#') { format!("{prefix_}{n:02}") } else { format!("{prefix_}{n}") }
}

/// Places a copy of library symbol `def` with its anchor at `at` (snapped to the grid), its
/// fields where the library puts them, and the next free reference. The symbol's definition
/// is copied into the schematic if it isn't there yet. Returns the new symbol's id.
pub fn place_symbol(sch: &mut Schematic, sheet: usize, def: &Symbol, at: Pt, id: Uuid) -> Uuid {
    if sch.symbol(&def.id).is_none() {
        sch.symbols.push(def.clone());
    }
    let placement = Placement { at: snap(at, SCHEMATIC_GRID), angle: 0.0, mirror: Mirror::None };
    let mut fields: Vec<Field> = def
        .fields
        .iter()
        .map(|f| Field { text: placement.text(&f.text), ..f.clone() })
        .collect();
    let pre = def.field(fields::REFERENCE).map_or("U", |f| prefix(f.value())).to_string();
    let reference = next_reference(sch, &pre);
    if let Some(f) = fields.iter_mut().find(|f| f.name == fields::REFERENCE) {
        f.text.text = reference;
    }
    sch.sheets[sheet].symbols.push(PlacedSymbol {
        id,
        symbol: def.id.clone(),
        placement,
        unit: 1,
        style: 1,
        fields,
        in_bom: def.in_bom,
        on_board: def.on_board,
        dnp: false,
        exclude_from_sim: false,
        pin_ids: def.pins.iter().map(|p| (p.number.clone(), Uuid::new_v4())).collect(),
    });
    id
}

/// Renumbers every symbol (GS8, "Fill in schematic symbol reference designators"): within
/// each prefix, by position — left to right, then top to bottom. With `keep_existing`, only
/// unannotated ones get numbers.
pub fn annotate(sch: &mut Schematic, keep_existing: bool) {
    let mut order: Vec<(String, Nm, Nm, usize, usize)> = vec![];
    for (si, sheet) in sch.sheets.iter().enumerate() {
        for (i, s) in sheet.symbols.iter().enumerate() {
            if keep_existing && !unannotated(s.reference()) {
                continue;
            }
            order.push((prefix(s.reference()).to_string(), s.placement.at.x, -s.placement.at.y, si, i));
        }
    }
    order.sort();
    for (_, _, _, si, i) in &order {
        if let Some(f) = sch.sheets[*si].symbols[*i].field_mut(fields::REFERENCE) {
            f.text.text = format!("{}?", prefix(&f.text.text));
        }
    }
    for (pre, _, _, si, i) in order {
        let r = next_reference(sch, &pre);
        if let Some(f) = sch.sheets[si].symbols[i].field_mut(fields::REFERENCE) {
            f.text.text = r;
        }
    }
}

/// Sets a placed symbol's field (adding it, hidden, if it is new). GS9: Value "red".
pub fn set_field(sch: &mut Schematic, symbol: Uuid, name: &str, value: &str) -> bool {
    for sheet in &mut sch.sheets {
        if let Some(s) = sheet.symbols.iter_mut().find(|s| s.id == symbol) {
            match s.field_mut(name) {
                Some(f) => f.text.text = value.into(),
                None => {
                    let mut t = Text::new(value, s.placement.at);
                    t.visible = false;
                    s.fields.push(Field { name: name.into(), text: t, show_name: false });
                }
            }
            return true;
        }
    }
    false
}

/// File → Page settings (GS3).
pub fn set_page(sch: &mut Schematic, sheet: usize, paper: Paper, title_block: TitleBlock) {
    let s = &mut sch.sheets[sheet];
    s.paper = paper;
    s.title_block = title_block;
}

/// The standard paper sizes (landscape).
pub fn paper(name: &str) -> Option<Paper> {
    let (w, h) = match name {
        "A5" => (210.0, 148.0),
        "A4" => (297.0, 210.0),
        "A3" => (420.0, 297.0),
        "A2" => (594.0, 420.0),
        "A1" => (841.0, 594.0),
        "A0" => (1189.0, 841.0),
        "A" | "USLetter" => (279.4, 215.9),
        "USLegal" => (355.6, 215.9),
        "B" | "USLedger" => (431.8, 279.4),
        "C" => (558.8, 431.8),
        "D" => (863.6, 558.8),
        "E" => (1117.6, 863.6),
        _ => return None,
    };
    Some(Paper { name: name.into(), size: crate::units::Size::mm(w, h) })
}

// ---------------------------------------------------------------------------------------------
// Geometry for selection

/// A placed symbol's body box (graphics and pins), without its fields.
pub fn symbol_bounds(sch: &Schematic, s: &PlacedSymbol) -> Bounds {
    let mut b = Bounds::of(s.placement.at);
    if let Some(def) = sch.symbol(&s.symbol) {
        for g in def.unit_graphics(s.unit, s.style) {
            match &g.item {
                SymbolItem::Shape(sh) => {
                    for p in sh.geom.extent() {
                        b.add(s.placement.apply(p));
                    }
                }
                SymbolItem::Text(t) => {
                    if let Some(tb) = crate::font::bounds(&s.placement.text(t)) {
                        b.add(tb.min);
                        b.add(tb.max);
                    }
                }
            }
        }
        for p in def.unit_pins(s.unit, s.style) {
            b.add(s.placement.apply(p.at));
            b.add(s.placement.apply(p.inner_end()));
        }
    }
    b
}

fn field_bounds(f: &Field) -> Option<Bounds> {
    if !f.text.visible || f.text.text.is_empty() {
        return None;
    }
    crate::font::bounds(&f.text)
}

fn label_bounds(l: &Label) -> Bounds {
    let b = crate::font::bounds(&l.text).unwrap_or(Bounds::of(l.text.at));
    Bounds::union(Some(b), Bounds::of(l.text.at))
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.x - a.x) as f64 * dx + (p.y - a.y) as f64 * dy) / l2).clamp(0.0, 1.0) };
    (p.x as f64 - (a.x as f64 + t * dx)).hypot(p.y as f64 - (a.y as f64 + t * dy))
}

/// The item under `p` (within `tol`): fields first (so a field can be picked alone), then
/// junctions, labels, wires, symbols.
pub fn hit(sch: &Schematic, sheet: usize, p: Pt, tol: Nm) -> Option<SchItem> {
    let sh = &sch.sheets[sheet];
    for s in sh.symbols.iter().rev() {
        for f in &s.fields {
            if field_bounds(f).is_some_and(|b| b.grow(tol / 2).contains(p)) {
                return Some(SchItem::Field(s.id, f.name.clone()));
            }
        }
    }
    if let Some(j) = sh.junctions.iter().find(|j| j.at.dist(p) <= tol as f64) {
        return Some(SchItem::Junction(j.id));
    }
    if let Some(n) = sh.no_connects.iter().find(|n| n.at.dist(p) <= tol as f64) {
        return Some(SchItem::NoConnect(n.id));
    }
    if let Some(l) = sh.labels.iter().find(|l| label_bounds(l).grow(tol / 2).contains(p)) {
        return Some(SchItem::Label(l.id));
    }
    if let Some(w) = sh.wires.iter().find(|w| seg_dist(p, w.a, w.b) <= tol as f64) {
        return Some(SchItem::Wire(w.id));
    }
    if let Some(w) = sh.buses.iter().find(|w| seg_dist(p, w.a, w.b) <= tol as f64) {
        return Some(SchItem::Bus(w.id));
    }
    if let Some(s) = sh.symbols.iter().rev().find(|s| symbol_bounds(sch, s).grow(tol).contains(p)) {
        return Some(SchItem::Symbol(s.id));
    }
    sh.notes.iter().find(|n| crate::font::bounds(&n.text).is_some_and(|b| b.contains(p))).map(|n| SchItem::Note(n.id))
}

fn inside(b: &Bounds, r: &Bounds) -> bool {
    r.contains(b.min) && r.contains(b.max)
}

fn touches(b: &Bounds, r: &Bounds) -> bool {
    b.min.x <= r.max.x && b.max.x >= r.min.x && b.min.y <= r.max.y && b.max.y >= r.min.y
}

fn seg_touches(a: Pt, b: Pt, r: &Bounds) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    // Sample the segment finely enough for schematic boxes.
    let n = ((a.dist(b) / SCHEMATIC_GRID as f64).ceil() as i64 * 4).clamp(1, 4096);
    (0..=n).any(|i| {
        let t = i as f64 / n as f64;
        r.contains(Pt::new(a.x + ((b.x - a.x) as f64 * t) as Nm, a.y + ((b.y - a.y) as f64 * t) as Nm))
    })
}

/// Box selection (GS5): left-to-right (`crossing == false`) takes what lies wholly inside;
/// right-to-left (`crossing`) also what the box touches.
pub fn box_select(sch: &Schematic, sheet: usize, r: Bounds, crossing: bool) -> Vec<SchItem> {
    let sh = &sch.sheets[sheet];
    let take = |b: &Bounds| if crossing { touches(b, &r) } else { inside(b, &r) };
    let mut out = vec![];
    for s in &sh.symbols {
        if take(&symbol_bounds(sch, s)) {
            out.push(SchItem::Symbol(s.id));
        }
    }
    for w in &sh.wires {
        let hit = if crossing { seg_touches(w.a, w.b, &r) } else { r.contains(w.a) && r.contains(w.b) };
        if hit {
            out.push(SchItem::Wire(w.id));
        }
    }
    out.extend(sh.junctions.iter().filter(|j| r.contains(j.at)).map(|j| SchItem::Junction(j.id)));
    out.extend(sh.no_connects.iter().filter(|j| r.contains(j.at)).map(|j| SchItem::NoConnect(j.id)));
    out.extend(sh.labels.iter().filter(|l| take(&label_bounds(l))).map(|l| SchItem::Label(l.id)));
    out
}

// ---------------------------------------------------------------------------------------------
// Moving, rotating, deleting

fn symbol_pin_points(sch: &Schematic, s: &PlacedSymbol) -> Vec<Pt> {
    sch.placed_pins(s).map(|(_, p)| p).collect()
}

/// Moves items by `d`. With `drag` (G), the ends of unselected wires on the moved symbols'
/// pins (and on moved wire ends) follow, so connections stay; without it (M) they stay put.
pub fn move_items(sch: &mut Schematic, sheet: usize, items: &[SchItem], d: Pt, drag: bool) {
    let mut anchors: Vec<Pt> = vec![];
    if drag {
        let sh = &sch.sheets[sheet];
        for it in items {
            match it {
                SchItem::Symbol(id) => {
                    if let Some(s) = sh.symbols.iter().find(|s| s.id == *id) {
                        anchors.extend(symbol_pin_points(sch, s));
                    }
                }
                SchItem::Wire(id) => {
                    if let Some(w) = sh.wires.iter().find(|w| w.id == *id) {
                        anchors.extend([w.a, w.b]);
                    }
                }
                SchItem::Label(id) => anchors.extend(sh.labels.iter().filter(|l| l.id == *id).map(|l| l.text.at)),
                _ => {}
            }
        }
    }
    let sh = &mut sch.sheets[sheet];
    for it in items {
        match it {
            SchItem::Symbol(id) => {
                if let Some(s) = sh.symbols.iter_mut().find(|s| s.id == *id) {
                    s.placement.at = s.placement.at + d;
                    s.fields.iter_mut().for_each(|f| f.text.at = f.text.at + d);
                }
            }
            SchItem::Field(id, name) => {
                if let Some(f) = sh.symbols.iter_mut().find(|s| s.id == *id).and_then(|s| s.field_mut(name)) {
                    f.text.at = f.text.at + d;
                }
            }
            SchItem::Wire(id) => sh.wires.iter_mut().filter(|w| w.id == *id).for_each(|w| {
                w.a = w.a + d;
                w.b = w.b + d;
            }),
            SchItem::Bus(id) => sh.buses.iter_mut().filter(|w| w.id == *id).for_each(|w| {
                w.a = w.a + d;
                w.b = w.b + d;
            }),
            SchItem::Junction(id) => sh.junctions.iter_mut().filter(|j| j.id == *id).for_each(|j| j.at = j.at + d),
            SchItem::NoConnect(id) => sh.no_connects.iter_mut().filter(|j| j.id == *id).for_each(|j| j.at = j.at + d),
            SchItem::Label(id) => sh.labels.iter_mut().filter(|l| l.id == *id).for_each(|l| l.text.at = l.text.at + d),
            SchItem::Note(id) => sh.notes.iter_mut().filter(|n| n.id == *id).for_each(|n| n.text.at = n.text.at + d),
            SchItem::Drawing(id) => sh.drawings.iter_mut().filter(|n| n.id == *id).for_each(|n| n.shape.geom = n.shape.geom.translated(d)),
        }
    }
    if drag {
        let moved_wires: Vec<Uuid> = items.iter().filter_map(|i| if let SchItem::Wire(id) = i { Some(*id) } else { None }).collect();
        for w in sh.wires.iter_mut().filter(|w| !moved_wires.contains(&w.id)) {
            if anchors.contains(&w.a) {
                w.a = w.a + d;
            }
            if anchors.contains(&w.b) {
                w.b = w.b + d;
            }
        }
        for j in sh.junctions.iter_mut() {
            if anchors.contains(&j.at) && !items.contains(&SchItem::Junction(j.id)) {
                j.at = j.at + d;
            }
        }
        sh.wires.retain(|w| w.a != w.b);
    }
}

/// The centre a selection rotates about: its box's centre, on the grid.
pub fn selection_center(sch: &Schematic, sheet: usize, items: &[SchItem]) -> Pt {
    let sh = &sch.sheets[sheet];
    let mut b: Option<Bounds> = None;
    for it in items {
        let ib = match it {
            SchItem::Symbol(id) => sh.symbols.iter().find(|s| s.id == *id).map(|s| Bounds::of(s.placement.at)),
            SchItem::Wire(id) => sh.wires.iter().find(|w| w.id == *id).map(|w| Bounds::union(Some(Bounds::of(w.a)), Bounds::of(w.b))),
            SchItem::Label(id) => sh.labels.iter().find(|l| l.id == *id).map(|l| Bounds::of(l.text.at)),
            SchItem::Junction(id) => sh.junctions.iter().find(|j| j.id == *id).map(|j| Bounds::of(j.at)),
            _ => None,
        };
        if let Some(ib) = ib {
            b = Some(Bounds::union(b, ib));
        }
    }
    snap(b.map_or(Pt::ZERO, |b| b.center()), SCHEMATIC_GRID)
}

fn rot_text(t: &mut Text, c: Pt) {
    t.at = c + (t.at - c).rotated(90.0);
    t.angle = normalize_deg(t.angle + 90.0);
}

/// Rotates items a quarter turn counter-clockwise about `center` (R).
pub fn rotate_items(sch: &mut Schematic, sheet: usize, items: &[SchItem], center: Pt) {
    let sh = &mut sch.sheets[sheet];
    let r = |p: Pt| center + (p - center).rotated(90.0);
    for it in items {
        match it {
            SchItem::Symbol(id) => {
                if let Some(s) = sh.symbols.iter_mut().find(|s| s.id == *id) {
                    s.placement.at = r(s.placement.at);
                    s.placement.angle = normalize_deg(s.placement.angle + 90.0);
                    s.fields.iter_mut().for_each(|f| rot_text(&mut f.text, center));
                }
            }
            SchItem::Field(id, name) => {
                if let Some(f) = sh.symbols.iter_mut().find(|s| s.id == *id).and_then(|s| s.field_mut(name)) {
                    let at = f.text.at;
                    rot_text(&mut f.text, at);
                }
            }
            SchItem::Wire(id) => sh.wires.iter_mut().filter(|w| w.id == *id).for_each(|w| {
                w.a = r(w.a);
                w.b = r(w.b);
            }),
            SchItem::Junction(id) => sh.junctions.iter_mut().filter(|j| j.id == *id).for_each(|j| j.at = r(j.at)),
            SchItem::NoConnect(id) => sh.no_connects.iter_mut().filter(|j| j.id == *id).for_each(|j| j.at = r(j.at)),
            SchItem::Label(id) => sh.labels.iter_mut().filter(|l| l.id == *id).for_each(|l| rot_text(&mut l.text, center)),
            SchItem::Note(id) => sh.notes.iter_mut().filter(|n| n.id == *id).for_each(|n| rot_text(&mut n.text, center)),
            _ => {}
        }
    }
}

/// Deletes items (Del). Deleting a field hides it instead.
pub fn delete_items(sch: &mut Schematic, sheet: usize, items: &[SchItem]) {
    let sh = &mut sch.sheets[sheet];
    for it in items {
        match it {
            SchItem::Symbol(id) => sh.symbols.retain(|s| s.id != *id),
            SchItem::Field(id, name) => {
                if let Some(f) = sh.symbols.iter_mut().find(|s| s.id == *id).and_then(|s| s.field_mut(name)) {
                    f.text.visible = false;
                }
            }
            SchItem::Wire(id) => sh.wires.retain(|w| w.id != *id),
            SchItem::Bus(id) => sh.buses.retain(|w| w.id != *id),
            SchItem::Junction(id) => sh.junctions.retain(|w| w.id != *id),
            SchItem::NoConnect(id) => sh.no_connects.retain(|w| w.id != *id),
            SchItem::Label(id) => sh.labels.retain(|w| w.id != *id),
            SchItem::Note(id) => sh.notes.retain(|w| w.id != *id),
            SchItem::Drawing(id) => sh.drawings.retain(|w| w.id != *id),
        }
    }
    let used: Vec<String> = sch.sheets.iter().flat_map(|s| &s.symbols).map(|s| s.symbol.clone()).collect();
    sch.symbols.retain(|d| used.contains(&d.id));
}

// ---------------------------------------------------------------------------------------------
// Wiring

/// Every point where things can join: wire ends, pin ends, labels.
fn connection_points(sch: &Schematic, sheet: usize) -> Vec<Pt> {
    let sh = &sch.sheets[sheet];
    let mut v: Vec<Pt> = sh.wires.iter().flat_map(|w| [w.a, w.b]).collect();
    for s in &sh.symbols {
        v.extend(symbol_pin_points(sch, s));
    }
    v
}

/// Where junction dots are needed: a point where three or more wire ends and pins meet, or a
/// wire end (or pin) on the middle of another wire.
pub fn junctions_needed(sch: &Schematic, sheet: usize) -> Vec<Pt> {
    let sh = &sch.sheets[sheet];
    let pts = connection_points(sch, sheet);
    let mut out: Vec<Pt> = vec![];
    for &p in &pts {
        if out.contains(&p) {
            continue;
        }
        let ends = pts.iter().filter(|q| **q == p).count();
        let through = sh.wires.iter().filter(|w| w.a != p && w.b != p && seg_dist(p, w.a, w.b) < 1.0).count();
        if ends + 2 * through >= 3 {
            out.push(p);
        }
    }
    out
}

/// Adds the junctions [`junctions_needed`] lists that aren't there, and removes ones that no
/// longer join anything.
pub fn fix_junctions(sch: &mut Schematic, sheet: usize) {
    let need = junctions_needed(sch, sheet);
    let sh = &mut sch.sheets[sheet];
    sh.junctions.retain(|j| need.contains(&j.at));
    for p in need {
        if !sh.junctions.iter().any(|j| j.at == p) {
            sh.junctions.push(Junction { id: Uuid::new_v4(), at: p, diameter: 0, color: None });
        }
    }
}

/// Draws a wire through `pts` (W: click, click, … double-click), on the grid; zero-length
/// segments are skipped. Junctions are added where the wire tees into others. Returns the new
/// segments' ids.
pub fn add_wire(sch: &mut Schematic, sheet: usize, pts: &[Pt]) -> Vec<Uuid> {
    let pts: Vec<Pt> = pts.iter().map(|p| snap(*p, SCHEMATIC_GRID)).collect();
    let mut ids = vec![];
    for w in pts.windows(2) {
        if w[0] == w[1] {
            continue;
        }
        let id = Uuid::new_v4();
        sch.sheets[sheet].wires.push(Wire { id, a: w[0], b: w[1], stroke: Default::default() });
        ids.push(id);
    }
    fix_junctions(sch, sheet);
    ids
}

/// A wire from `from` to `to` turning once (horizontal first), as the wire tool draws
/// between two clicks.
pub fn manhattan(from: Pt, to: Pt) -> Vec<Pt> {
    if from.x == to.x || from.y == to.y { vec![from, to] } else { vec![from, Pt::new(to.x, from.y), to] }
}

/// A net label (L) at `at`, its text running at `angle`.
pub fn add_label(sch: &mut Schematic, sheet: usize, text: &str, at: Pt, angle: f64, kind: LabelKind) -> Uuid {
    let id = Uuid::new_v4();
    let style = TextStyle { h_align: crate::graphics::HAlign::Left, v_align: crate::graphics::VAlign::Bottom, ..Default::default() };
    sch.sheets[sheet].labels.push(Label { id, kind, text: Text { text: text.into(), at: snap(at, SCHEMATIC_GRID), angle, style, visible: true }, fields: vec![] });
    id
}

/// Adds a text note (the Text tool): left-aligned at `at`, not on the grid, no electrical meaning.
pub fn add_note(sch: &mut Schematic, sheet: usize, text: &str, at: Pt) -> Uuid {
    let id = Uuid::new_v4();
    let style = TextStyle { h_align: crate::graphics::HAlign::Left, v_align: crate::graphics::VAlign::Bottom, ..Default::default() };
    sch.sheets[sheet].notes.push(Note { id, text: Text { text: text.into(), at, angle: 0.0, style, visible: true } });
    id
}

pub fn add_no_connect(sch: &mut Schematic, sheet: usize, at: Pt) -> Uuid {
    let id = Uuid::new_v4();
    sch.sheets[sheet].no_connects.push(NoConnect { id, at: snap(at, SCHEMATIC_GRID) });
    id
}

/// The pin under `p` (to start a wire from an unconnected pin): its symbol, number and end.
pub fn pin_at(sch: &Schematic, sheet: usize, p: Pt, tol: Nm) -> Option<(Uuid, String, Pt)> {
    sch.sheets[sheet].symbols.iter().find_map(|s| sch.placed_pins(s).find(|(_, at)| at.dist(p) <= tol as f64).map(|(pin, at)| (s.id, pin.number.clone(), at)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LibraryTable;

    fn sch() -> (Schematic, LibraryTable) {
        let mut s = Schematic::default();
        s.sheets.push(Sheet { id: Uuid::new_v4(), name: "Root".into(), paper: Paper::default(), ..Default::default() });
        (s, LibraryTable::builtin())
    }

    #[test]
    fn placing_annotates_and_snaps() {
        let (mut s, lib) = sch();
        let r = lib.symbol("Device:R_US").unwrap();
        let a = place_symbol(&mut s, 0, r, Pt::mm(10.1, 20.2), Uuid::new_v4());
        place_symbol(&mut s, 0, r, Pt::mm(30.0, 20.0), Uuid::new_v4());
        place_symbol(&mut s, 0, lib.symbol("power:GND").unwrap(), Pt::mm(30.0, 10.0), Uuid::new_v4());
        let refs: Vec<&str> = s.sheets[0].symbols.iter().map(|x| x.reference()).collect();
        assert_eq!(refs, ["R1", "R2", "#PWR01"]);
        assert_eq!(s.sheets[0].symbols[0].placement.at, Pt::new(8 * SCHEMATIC_GRID, 16 * SCHEMATIC_GRID));
        assert_eq!(s.symbols.len(), 2);
        // Deleting R1 frees its number.
        delete_items(&mut s, 0, &[SchItem::Symbol(a)]);
        assert_eq!(next_reference(&s, "R"), "R1");
        let id = s.sheets[0].symbols[0].id;
        assert!(set_field(&mut s, id, "Value", "1k"));
        assert_eq!(s.sheets[0].symbols[0].value(), "1k");
    }

    #[test]
    fn drag_keeps_wires_on_pins_move_leaves_them() {
        let (mut s, lib) = sch();
        let r = place_symbol(&mut s, 0, lib.symbol("Device:R").unwrap(), Pt::mm(50.8, 50.8), Uuid::new_v4());
        let top = s.placed_pins(&s.sheets[0].symbols[0]).find(|(p, _)| p.number == "1").unwrap().1;
        add_wire(&mut s, 0, &[top, top + Pt::mm(0.0, 10.16)]);
        let d = Pt::mm(5.08, 0.0);
        let mut dragged = s.clone();
        move_items(&mut dragged, 0, &[SchItem::Symbol(r)], d, true);
        assert_eq!(dragged.sheets[0].wires[0].a, top + d);
        move_items(&mut s, 0, &[SchItem::Symbol(r)], d, false);
        assert_eq!(s.sheets[0].wires[0].a, top);
        // Rotating about its own anchor turns the pins: the vertical R lies flat.
        let c = selection_center(&dragged, 0, &[SchItem::Symbol(r)]);
        rotate_items(&mut dragged, 0, &[SchItem::Symbol(r)], c);
        let pins: Vec<Pt> = dragged.placed_pins(&dragged.sheets[0].symbols[0]).map(|(_, p)| p).collect();
        assert_eq!(pins[0].y, pins[1].y);
    }

    #[test]
    fn tees_get_junctions_and_selection_works() {
        let (mut s, _) = sch();
        add_wire(&mut s, 0, &[Pt::mm(0.0, 0.0), Pt::mm(25.4, 0.0)]);
        add_wire(&mut s, 0, &[Pt::mm(12.7, 0.0), Pt::mm(12.7, 12.7)]);
        assert_eq!(s.sheets[0].junctions.len(), 1);
        assert_eq!(s.sheets[0].junctions[0].at, Pt::mm(12.7, 0.0));
        // A crossing without a shared point needs none.
        add_wire(&mut s, 0, &[Pt::mm(20.32, -5.08), Pt::mm(20.32, 5.08)]);
        assert_eq!(s.sheets[0].junctions.len(), 1);
        let hit_wire = hit(&s, 0, Pt::mm(5.0, 0.2), crate::units::mm(0.5));
        assert!(matches!(hit_wire, Some(SchItem::Wire(_))));
        let enclosed = box_select(&s, 0, Bounds { min: Pt::mm(-1.0, -1.0), max: Pt::mm(14.0, 14.0) }, false);
        assert_eq!(enclosed.iter().filter(|i| matches!(i, SchItem::Wire(_))).count(), 1);
        let crossing = box_select(&s, 0, Bounds { min: Pt::mm(-1.0, -1.0), max: Pt::mm(14.0, 14.0) }, true);
        assert_eq!(crossing.iter().filter(|i| matches!(i, SchItem::Wire(_))).count(), 2);
    }
}
