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


/// A point that moved: from, to, and (a pin there) the way the pin points out of its symbol,
/// one of (±1, 0), (0, ±1), so a wire can come into it straight.
type Shift = (Pt, Pt, Option<Pt>);

/// The way a pin at `angle` (towards its body, degrees, as placed) points out: a unit step.
fn outward(angle: f64) -> Pt {
    match ((angle.rem_euclid(360.0) + 45.0) / 90.0) as i32 % 4 {
        0 => Pt::new(-1, 0),
        1 => Pt::new(0, -1),
        2 => Pt::new(1, 0),
        _ => Pt::new(0, 1),
    }
}

/// The pins of the selected symbols (keyed by symbol and pin), where they are and which way
/// they point out.
fn pin_spots(sch: &Schematic, sheet: usize, items: &[SchItem]) -> Vec<((Uuid, usize), Pt, Pt)> {
    let sh = &sch.sheets[sheet];
    let mut out = vec![];
    for it in items {
        let SchItem::Symbol(id) = it else { continue };
        let Some(s) = sh.symbols.iter().find(|s| s.id == *id) else { continue };
        for (k, (p, at)) in sch.placed_pins(s).enumerate() {
            out.push(((s.id, k), at, outward(s.placement.apply_angle(p.angle))));
        }
    }
    out
}

/// How the pins of a selection moved between two states.
fn pin_shifts(before: &[((Uuid, usize), Pt, Pt)], after: &[((Uuid, usize), Pt, Pt)]) -> Vec<Shift> {
    before.iter().filter_map(|(k, from, _)| after.iter().find(|a| a.0 == *k).map(|(_, to, out)| (*from, *to, Some(*out)))).filter(|s| s.0 != s.1).collect()
}

/// A symbol's body as placed: its drawing and the inner ends of its pins (not the pin tips).
fn body_box(sch: &Schematic, s: &PlacedSymbol) -> Option<Bounds> {
    let def = sch.symbol(&s.symbol)?;
    let mut b: Option<Bounds> = None;
    for g in def.unit_graphics(s.unit, s.style) {
        if let SymbolItem::Shape(sh) = &g.item {
            for p in sh.geom.extent() {
                b = Some(Bounds::union(b, Bounds::of(s.placement.apply(p))));
            }
        }
    }
    for p in def.unit_pins(s.unit, s.style) {
        b = Some(Bounds::union(b, Bounds::of(s.placement.apply(p.inner_end()))));
    }
    b
}

/// Whether a level or upright segment runs through a box's inside.
fn crosses(a: Pt, b: Pt, r: &Bounds) -> bool {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    x0 < r.max.x && x1 > r.min.x && y0 < r.max.y && y1 > r.min.y
}

/// A path without repeated points or straight-through corners.
fn tidy(pts: Vec<Pt>) -> Vec<Pt> {
    let mut out: Vec<Pt> = vec![];
    for p in pts {
        if out.last() == Some(&p) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            if (a.x == b.x && b.x == p.x) || (a.y == b.y && b.y == p.y) {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// A square path from `o` to `e`, coming into `e` the way `out` points (a pin), clear of
/// `bodies` where it can be: the shortest of the L and Z shapes (fewest bends first),
/// starting along `level` (the wire's old direction) when that ties.
fn square_path(o: Pt, e: Pt, out: Option<Pt>, level: bool, bodies: &[Bounds]) -> Vec<Pt> {
    let g = SCHEMATIC_GRID;
    // A pin is left straight, at least a grid step, before any bend.
    let q = out.map_or(e, |d| e + Pt::new(d.x * g, d.y * g));
    let mid = |a: Nm, b: Nm| snap(Pt::new((a + b) / 2, 0), g).x;
    let mut cands = vec![
        vec![o, Pt::new(q.x, o.y), q, e],
        vec![o, Pt::new(o.x, q.y), q, e],
        vec![o, Pt::new(mid(o.x, q.x), o.y), Pt::new(mid(o.x, q.x), q.y), q, e],
        vec![o, Pt::new(o.x, mid(o.y, q.y)), Pt::new(q.x, mid(o.y, q.y)), q, e],
    ];
    if let Some(d) = out {
        // Out of the pin, along it, then across: for a pin facing away from `o`.
        let far = |k: Nm| e + Pt::new(d.x * k, d.y * k);
        let r = far(2 * g);
        cands.push(vec![o, Pt::new(o.x, r.y), r, e]);
        cands.push(vec![o, Pt::new(r.x, o.y), r, e]);
    }
    cands
        .into_iter()
        .map(tidy)
        .filter(|p| p.len() >= 2 && p.windows(2).all(|s| s[0].x == s[1].x || s[0].y == s[1].y))
        // Coming into a pin from its own side only.
        .filter(|p| out.is_none_or(|d| {
            let before = p[p.len() - 2];
            let (dx, dy) = ((before.x - e.x).signum(), (before.y - e.y).signum());
            (dx, dy) == (d.x, d.y)
        }))
        .min_by_key(|p| {
            let through = p.windows(2).filter(|s| bodies.iter().any(|b| crosses(s[0], s[1], b))).count();
            let first_level = p[0].y == p[1].y;
            let len: i64 = p.windows(2).map(|s| (s[1].x - s[0].x).abs() + (s[1].y - s[0].y).abs()).sum();
            (through, p.len(), first_level != level, len)
        })
        .unwrap_or_else(|| vec![o, e])
}

/// Keeps wires (other than `skip`) on points that moved, and square: the end on a moved point
/// follows it. A wire that would go slanted slides the free corner beyond it (the other wire
/// of an L, so both stay square) when it then still comes into its pin from outside, or is
/// re-run as a square path ([`square_path`]) clear of the moved symbols.
fn follow_wires(sch: &mut Schematic, sheet: usize, items: &[SchItem], shifts: &[Shift], skip: &[Uuid]) {
    if shifts.is_empty() {
        return;
    }
    // Where a corner can't slide from: pins (where they are now) and labels.
    let fixed: Vec<Pt> = sch.sheets[sheet].symbols.iter().flat_map(|s| sch.placed_pins(s).map(|(_, p)| p)).chain(sch.sheets[sheet].labels.iter().map(|l| l.text.at)).collect();
    let bodies: Vec<Bounds> = sch.sheets[sheet].symbols.iter().filter(|s| items.contains(&SchItem::Symbol(s.id))).filter_map(|s| body_box(sch, s)).collect();
    let sh = &mut sch.sheets[sheet];
    let shift_of = |p: Pt| shifts.iter().find(|s| s.0 == p).copied();
    let ids: Vec<Uuid> = sh.wires.iter().filter(|w| !skip.contains(&w.id)).map(|w| w.id).collect();
    let mut slid: Vec<Uuid> = vec![];
    for id in ids {
        if slid.contains(&id) {
            continue;
        }
        let Some(i) = sh.wires.iter().position(|w| w.id == id) else { continue };
        let w = sh.wires[i].clone();
        let (s, o) = match (shift_of(w.a), shift_of(w.b)) {
            (None, None) => continue,
            (Some(a), Some(b)) => {
                sh.wires[i].a = a.1;
                sh.wires[i].b = b.1;
                continue;
            }
            (Some(s), None) => (s, w.b),
            (None, Some(s)) => (s, w.a),
        };
        let e = s.1;
        let level = w.a.y == w.b.y && w.a.x != w.b.x;
        let upright = w.a.x == w.b.x && w.a.y != w.b.y;
        // From outside the pin (or no pin): the way in is fine.
        let enters_ok = |from: Pt| s.2.is_none_or(|d| ((from.x - e.x).signum(), (from.y - e.y).signum()) == (d.x, d.y));
        if (o.x == e.x || o.y == e.y || !(level || upright)) && enters_ok(o) {
            (sh.wires[i].a, sh.wires[i].b) = (o, e);
            continue;
        }
        // The free corner beyond slides along the other wire.
        let others: Vec<usize> = (0..sh.wires.len()).filter(|&j| j != i && (sh.wires[j].a == o || sh.wires[j].b == o)).collect();
        if (level || upright)
            && let [j] = others[..]
            && !fixed.contains(&o)
            && shift_of(o).is_none()
            && !skip.contains(&sh.wires[j].id)
        {
            let v = &sh.wires[j];
            let across = if level { v.a.x == v.b.x } else { v.a.y == v.b.y };
            let o2 = if level { Pt::new(o.x, e.y) } else { Pt::new(e.x, o.y) };
            if across && o2 != e && enters_ok(o2) {
                let v = &mut sh.wires[j];
                if v.a == o {
                    v.a = o2;
                } else {
                    v.b = o2;
                }
                slid.push(v.id);
                (sh.wires[i].a, sh.wires[i].b) = (o2, e);
                continue;
            }
        }
        let pts = square_path(o, e, s.2, level, &bodies);
        (sh.wires[i].a, sh.wires[i].b) = (pts[0], pts[1]);
        for k in 1..pts.len() - 1 {
            sh.wires.push(Wire { id: Uuid::new_v4(), a: pts[k], b: pts[k + 1], stroke: w.stroke });
        }
    }
    sh.wires.retain(|w| w.a != w.b);
}

/// Moves items by `d`. With `drag` (G, or dragging with the mouse), unselected wires on the
/// moved symbols' pins (and on moved wire ends and labels) stay on them and square
/// ([`follow_wires`]); without it (M) they stay put.
pub fn move_items(sch: &mut Schematic, sheet: usize, items: &[SchItem], d: Pt, drag: bool) {
    let mut shifts: Vec<Shift> = vec![];
    if drag {
        shifts.extend(pin_spots(sch, sheet, items).into_iter().map(|(_, p, out)| (p, p + d, Some(out))));
        let sh = &sch.sheets[sheet];
        for it in items {
            match it {
                SchItem::Wire(id) => {
                    if let Some(w) = sh.wires.iter().find(|w| w.id == *id) {
                        shifts.extend([(w.a, w.a + d, None), (w.b, w.b + d, None)]);
                    }
                }
                SchItem::Label(id) => shifts.extend(sh.labels.iter().filter(|l| l.id == *id).map(|l| (l.text.at, l.text.at + d, None))),
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
        for j in sh.junctions.iter_mut() {
            if shifts.iter().any(|s| s.0 == j.at) && !items.contains(&SchItem::Junction(j.id)) {
                j.at = j.at + d;
            }
        }
        let moved_wires: Vec<Uuid> = items.iter().filter_map(|i| if let SchItem::Wire(id) = i { Some(*id) } else { None }).collect();
        follow_wires(sch, sheet, items, &shifts, &moved_wires);
    }
}


/// The wires joined to `wire` through each other (an end on another wire's end or along it),
/// `wire` among them: a run drawn as one.
pub fn connected_wires(sch: &Schematic, sheet: usize, wire: Uuid) -> Vec<Uuid> {
    let ws = &sch.sheets[sheet].wires;
    let Some(start) = ws.iter().position(|w| w.id == wire) else { return vec![] };
    let touch = |a: &Wire, b: &Wire| {
        use crate::connectivity::on_segment;
        on_segment(a.a, b.a, b.b) || on_segment(a.b, b.a, b.b) || on_segment(b.a, a.a, a.b) || on_segment(b.b, a.a, a.b)
    };
    let mut seen = vec![false; ws.len()];
    let mut stack = vec![start];
    seen[start] = true;
    while let Some(i) = stack.pop() {
        for j in 0..ws.len() {
            if !seen[j] && touch(&ws[i], &ws[j]) {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    ws.iter().zip(seen).filter(|(_, s)| *s).map(|(w, _)| w.id).collect()
}

/// Colours wires (`None`: the theme's wire colour).
pub fn set_wire_color(sch: &mut Schematic, sheet: usize, wires: &[Uuid], color: Option<crate::graphics::Color>) {
    for w in sch.sheets[sheet].wires.iter_mut().filter(|w| wires.contains(&w.id)) {
        w.stroke.color = color;
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

/// Rotates items a quarter turn counter-clockwise about `center` (R). Wires on the turned pins
/// (and on turned wire ends) stay on them, square ([`follow_wires`]).
pub fn rotate_items(sch: &mut Schematic, sheet: usize, items: &[SchItem], center: Pt) {
    keeping_wires(sch, sheet, items, |sch| turn_items(sch, sheet, items, center));
}

/// Mirrors items (X, Y; see [`flip_items`]), wires on them staying on them, square.
pub fn mirror_items(sch: &mut Schematic, sheet: usize, items: &[SchItem], center: Pt, up_down: bool) {
    keeping_wires(sch, sheet, items, |sch| flip_items(sch, sheet, items, center, up_down));
}

/// Runs an edit of `items` and brings the other wires along with their pins and wire ends.
fn keeping_wires(sch: &mut Schematic, sheet: usize, items: &[SchItem], edit: impl FnOnce(&mut Schematic)) {
    let wires: Vec<Uuid> = items.iter().filter_map(|i| if let SchItem::Wire(id) = i { Some(*id) } else { None }).collect();
    let ends = |sch: &Schematic| sch.sheets[sheet].wires.iter().filter(|w| wires.contains(&w.id)).flat_map(|w| [(w.id, 0, w.a), (w.id, 1, w.b)]).collect::<Vec<_>>();
    let (pins, wire_ends) = (pin_spots(sch, sheet, items), ends(sch));
    edit(sch);
    let mut shifts = pin_shifts(&pins, &pin_spots(sch, sheet, items));
    for (id, k, to) in ends(sch) {
        if let Some((_, _, from)) = wire_ends.iter().find(|e| e.0 == id && e.1 == k)
            && *from != to
        {
            shifts.push((*from, to, None));
        }
    }
    follow_wires(sch, sheet, items, &shifts, &wires);
}

/// Rotates items a quarter turn counter-clockwise about `center`, leaving wires that aren't
/// among them where they are (a symbol being placed).
pub fn turn_items(sch: &mut Schematic, sheet: usize, items: &[SchItem], center: Pt) {
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


// ---------------------------------------------------------------------------------------------
// Mirror, copy and paste, net highlighting

/// Mirrors items about the vertical line through `center` (X: left for right) or, with `up_down`,
/// the horizontal one (Y: top for bottom). A symbol keeps its anchor's mirrored place and turns
/// over (its rotation negated, its mirror flag toggled; mirrored twice it is half turned).
pub fn flip_items(sch: &mut Schematic, sheet: usize, items: &[SchItem], center: Pt, up_down: bool) {
    let m = |p: Pt| if up_down { Pt::new(p.x, 2 * center.y - p.y) } else { Pt::new(2 * center.x - p.x, p.y) };
    let flip_text = |t: &mut Text| {
        t.at = m(t.at);
        let horizontal = t.angle.rem_euclid(180.0) < 1.0;
        if horizontal != up_down {
            t.style.h_align = match t.style.h_align {
                crate::graphics::HAlign::Left => crate::graphics::HAlign::Right,
                crate::graphics::HAlign::Right => crate::graphics::HAlign::Left,
                c => c,
            };
        }
    };
    let sh = &mut sch.sheets[sheet];
    for it in items {
        match it {
            SchItem::Symbol(id) => {
                if let Some(s) = sh.symbols.iter_mut().find(|s| s.id == *id) {
                    s.placement.at = m(s.placement.at);
                    let (mirror, half) = match (s.placement.mirror, up_down) {
                        (Mirror::None, false) => (Mirror::Y, false),
                        (Mirror::None, true) => (Mirror::X, false),
                        (Mirror::Y, false) | (Mirror::X, true) => (Mirror::None, false),
                        (Mirror::X, false) | (Mirror::Y, true) => (Mirror::None, true),
                    };
                    s.placement.mirror = mirror;
                    s.placement.angle = normalize_deg(-s.placement.angle + if half { 180.0 } else { 0.0 });
                    s.fields.iter_mut().for_each(|f| flip_text(&mut f.text));
                }
            }
            SchItem::Field(id, name) => {
                if let Some(f) = sh.symbols.iter_mut().find(|s| s.id == *id).and_then(|s| s.field_mut(name)) {
                    let at = f.text.at;
                    flip_text(&mut f.text);
                    f.text.at = at;
                }
            }
            SchItem::Wire(id) => sh.wires.iter_mut().filter(|w| w.id == *id).for_each(|w| (w.a, w.b) = (m(w.a), m(w.b))),
            SchItem::Bus(id) => sh.buses.iter_mut().filter(|w| w.id == *id).for_each(|w| (w.a, w.b) = (m(w.a), m(w.b))),
            SchItem::Junction(id) => sh.junctions.iter_mut().filter(|j| j.id == *id).for_each(|j| j.at = m(j.at)),
            SchItem::NoConnect(id) => sh.no_connects.iter_mut().filter(|j| j.id == *id).for_each(|j| j.at = m(j.at)),
            SchItem::Label(id) => sh.labels.iter_mut().filter(|l| l.id == *id).for_each(|l| {
                l.text.at = m(l.text.at);
                // A label points the other way across its mirror.
                let horizontal = l.text.angle.rem_euclid(180.0) < 1.0;
                if horizontal != up_down {
                    l.text.angle = normalize_deg(l.text.angle + 180.0);
                }
            }),
            SchItem::Note(id) => sh.notes.iter_mut().filter(|n| n.id == *id).for_each(|n| flip_text(&mut n.text)),
            SchItem::Drawing(id) => sh.drawings.iter_mut().filter(|n| n.id == *id).for_each(|n| n.shape.geom = n.shape.geom.map(m)),
        }
    }
}

/// Copied schematic items (Ctrl+C), relative to where they were copied from, with the
/// definitions of their symbols (so they paste into another schematic too).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SchClip {
    pub symbols: Vec<PlacedSymbol>,
    pub definitions: Vec<Symbol>,
    pub wires: Vec<Wire>,
    pub junctions: Vec<Junction>,
    pub no_connects: Vec<NoConnect>,
    pub labels: Vec<Label>,
    pub notes: Vec<Note>,
    pub drawings: Vec<Drawing>,
}

impl SchClip {
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty() && self.wires.is_empty() && self.junctions.is_empty() && self.no_connects.is_empty() && self.labels.is_empty() && self.notes.is_empty() && self.drawings.is_empty()
    }
}

/// Copies `items`; their positions are kept relative to `origin` (on the grid).
pub fn copy_items(sch: &Schematic, sheet: usize, items: &[SchItem], origin: Pt) -> SchClip {
    let mut c = SchClip::default();
    let d = Pt::ZERO - snap(origin, SCHEMATIC_GRID);
    let sh = &sch.sheets[sheet];
    let mut tmp = Schematic { symbols: vec![], sheets: vec![Sheet::default()] };
    for it in items {
        match it {
            SchItem::Symbol(id) => {
                if let Some(s) = sh.symbols.iter().find(|s| s.id == *id) {
                    tmp.sheets[0].symbols.push(s.clone());
                    if let Some(def) = sch.symbol(&s.symbol)
                        && !c.definitions.iter().any(|x| x.id == def.id)
                    {
                        c.definitions.push(def.clone());
                    }
                }
            }
            SchItem::Wire(id) => tmp.sheets[0].wires.extend(sh.wires.iter().filter(|w| w.id == *id).cloned()),
            SchItem::Junction(id) => tmp.sheets[0].junctions.extend(sh.junctions.iter().filter(|w| w.id == *id).cloned()),
            SchItem::NoConnect(id) => tmp.sheets[0].no_connects.extend(sh.no_connects.iter().filter(|w| w.id == *id).cloned()),
            SchItem::Label(id) => tmp.sheets[0].labels.extend(sh.labels.iter().filter(|w| w.id == *id).cloned()),
            SchItem::Note(id) => tmp.sheets[0].notes.extend(sh.notes.iter().filter(|w| w.id == *id).cloned()),
            SchItem::Drawing(id) => tmp.sheets[0].drawings.extend(sh.drawings.iter().filter(|w| w.id == *id).cloned()),
            _ => {}
        }
    }
    let all: Vec<SchItem> = everything(&tmp, 0);
    move_items(&mut tmp, 0, &all, d, false);
    let s = tmp.sheets.remove(0);
    (c.symbols, c.wires, c.junctions, c.no_connects, c.labels, c.notes, c.drawings) = (s.symbols, s.wires, s.junctions, s.no_connects, s.labels, s.notes, s.drawings);
    c
}

/// Every item of a sheet.
pub fn everything(sch: &Schematic, sheet: usize) -> Vec<SchItem> {
    let sh = &sch.sheets[sheet];
    let mut v: Vec<SchItem> = sh.symbols.iter().map(|x| SchItem::Symbol(x.id)).collect();
    v.extend(sh.wires.iter().map(|x| SchItem::Wire(x.id)));
    v.extend(sh.buses.iter().map(|x| SchItem::Bus(x.id)));
    v.extend(sh.junctions.iter().map(|x| SchItem::Junction(x.id)));
    v.extend(sh.no_connects.iter().map(|x| SchItem::NoConnect(x.id)));
    v.extend(sh.labels.iter().map(|x| SchItem::Label(x.id)));
    v.extend(sh.notes.iter().map(|x| SchItem::Note(x.id)));
    v.extend(sh.drawings.iter().map(|x| SchItem::Drawing(x.id)));
    v
}

/// Pastes `clip` with its origin at `at` (snapped): new ids, symbols annotated on from the
/// schematic's references, definitions added when missing. Returns the new items (the
/// selection after a paste).
pub fn paste(sch: &mut Schematic, sheet: usize, clip: &SchClip, at: Pt) -> Vec<SchItem> {
    for def in &clip.definitions {
        if sch.symbol(&def.id).is_none() {
            sch.symbols.push(def.clone());
        }
    }
    let d = snap(at, SCHEMATIC_GRID) - Pt::ZERO;
    let mut out = vec![];
    let mut tmp = Schematic { symbols: vec![], sheets: vec![Sheet::default()] };
    {
        let t = &mut tmp.sheets[0];
        t.symbols = clip.symbols.clone();
        t.wires = clip.wires.clone();
        t.junctions = clip.junctions.clone();
        t.no_connects = clip.no_connects.clone();
        t.labels = clip.labels.clone();
        t.notes = clip.notes.clone();
        t.drawings = clip.drawings.clone();
    }
    let all = everything(&tmp, 0);
    move_items(&mut tmp, 0, &all, d, false);
    let t = tmp.sheets.remove(0);
    for mut s in t.symbols {
        s.id = Uuid::new_v4();
        s.pin_ids.iter_mut().for_each(|(_, id)| *id = Uuid::new_v4());
        let pre = prefix(s.reference()).to_string();
        // Annotated on from what the schematic has (C2 pasted beside C1 becomes C3).
        let r = next_reference(sch, &pre);
        if let Some(f) = s.field_mut(fields::REFERENCE) {
            f.text.text = r;
        }
        out.push(SchItem::Symbol(s.id));
        sch.sheets[sheet].symbols.push(s);
    }
    let sh = &mut sch.sheets[sheet];
    for mut w in t.wires {
        w.id = Uuid::new_v4();
        out.push(SchItem::Wire(w.id));
        sh.wires.push(w);
    }
    for mut j in t.junctions {
        j.id = Uuid::new_v4();
        out.push(SchItem::Junction(j.id));
        sh.junctions.push(j);
    }
    for mut n in t.no_connects {
        n.id = Uuid::new_v4();
        out.push(SchItem::NoConnect(n.id));
        sh.no_connects.push(n);
    }
    for mut l in t.labels {
        l.id = Uuid::new_v4();
        out.push(SchItem::Label(l.id));
        sh.labels.push(l);
    }
    for mut n in t.notes {
        n.id = Uuid::new_v4();
        out.push(SchItem::Note(n.id));
        sh.notes.push(n);
    }
    for mut n in t.drawings {
        n.id = Uuid::new_v4();
        out.push(SchItem::Drawing(n.id));
        sh.drawings.push(n);
    }
    out
}

/// The net under `p` (a wire, a pin or a label there) as the items to highlight: its wires,
/// labels and the junctions on them. With the net's name.
pub fn net_items_at(sch: &Schematic, sheet: usize, p: Pt, tol: Nm) -> Option<(String, Vec<SchItem>)> {
    let nl = crate::connectivity::netlist(sch);
    let sh = &sch.sheets[sheet];
    let wire = sh.wires.iter().find(|w| seg_dist(p, w.a, w.b) <= tol as f64).map(|w| w.id);
    let net = nl.nets.iter().find(|n| {
        wire.is_some_and(|w| n.wires.contains(&(sheet, w))) || n.pins.iter().any(|q| q.sheet == sheet && q.at.dist(p) <= tol as f64)
    })?;
    let wires: Vec<Uuid> = net.wires.iter().filter(|(s, _)| *s == sheet).map(|(_, w)| *w).collect();
    let mut items: Vec<SchItem> = wires.iter().map(|w| SchItem::Wire(*w)).collect();
    let on = |q: Pt| sh.wires.iter().filter(|w| wires.contains(&w.id)).any(|w| crate::connectivity::on_segment(q, w.a, w.b));
    items.extend(sh.labels.iter().filter(|l| on(l.text.at) || net.labels.contains(&l.text.text)).map(|l| SchItem::Label(l.id)));
    items.extend(sh.junctions.iter().filter(|j| on(j.at)).map(|j| SchItem::Junction(j.id)));
    Some((net.name.clone(), items))
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
        // Still on the pin (through a bend: the wire went straight up, the pin moved across).
        assert!(dragged.sheets[0].wires.iter().any(|w| w.a == top + d || w.b == top + d));
        assert!(square_and_joined(&dragged, &[top + Pt::mm(0.0, 10.16)]));
        move_items(&mut s, 0, &[SchItem::Symbol(r)], d, false);
        assert_eq!(s.sheets[0].wires[0].a, top);
        // Rotating about its own anchor turns the pins: the vertical R lies flat.
        let c = selection_center(&dragged, 0, &[SchItem::Symbol(r)]);
        rotate_items(&mut dragged, 0, &[SchItem::Symbol(r)], c);
        let pins: Vec<Pt> = dragged.placed_pins(&dragged.sheets[0].symbols[0]).map(|(_, p)| p).collect();
        assert_eq!(pins[0].y, pins[1].y);
    }

    /// Every wire is level or upright, and every wire end but the `free` ones is on a pin, a
    /// label, another wire's end or along another wire.
    fn square_and_joined(s: &Schematic, free: &[Pt]) -> bool {
        let sh = &s.sheets[0];
        let pins: Vec<Pt> = sh.symbols.iter().flat_map(|x| s.placed_pins(x).map(|(_, p)| p)).collect();
        sh.wires.iter().all(|w| w.a.x == w.b.x || w.a.y == w.b.y)
            && sh.wires.iter().all(|w| {
                [w.a, w.b].iter().all(|&e| {
                    free.contains(&e) || pins.contains(&e) || sh.labels.iter().any(|l| l.text.at == e) || sh.wires.iter().any(|v| v.id != w.id && crate::connectivity::on_segment(e, v.a, v.b))
                })
            })
    }

    /// A pin with an L of wire going off to the left and up: (pin, corner, far end).
    fn l_wire(s: &mut Schematic, lib: &LibraryTable) -> (Uuid, Pt, Pt, Pt) {
        // A level R: its pin 2 on the left.
        let r = place_symbol(s, 0, lib.symbol("Device:R").unwrap(), Pt::mm(50.8, 50.8), Uuid::new_v4());
        turn_items(s, 0, &[SchItem::Symbol(r)], Pt::mm(50.8, 50.8));
        let left = s.placed_pins(&s.sheets[0].symbols[0]).map(|(_, p)| p).min_by_key(|p| p.x).unwrap();
        let corner = left - Pt::mm(10.16, 0.0);
        let far = corner + Pt::mm(0.0, 12.7);
        add_wire(s, 0, &[left, corner, far]);
        (r, left, corner, far)
    }

    #[test]
    fn dragging_slides_the_corner_so_wires_stay_square() {
        let (mut s, lib) = sch();
        let (r, left, corner, far) = l_wire(&mut s, &lib);
        // Down 5.08: the corner comes down with it; the far end stays.
        move_items(&mut s, 0, &[SchItem::Symbol(r)], Pt::mm(0.0, -5.08), true);
        let w = &s.sheets[0].wires;
        assert_eq!(w.len(), 2, "{w:?}");
        let new_corner = corner - Pt::mm(0.0, 5.08);
        assert!(w.iter().any(|x| (x.a, x.b) == (left - Pt::mm(0.0, 5.08), new_corner) || (x.b, x.a) == (left - Pt::mm(0.0, 5.08), new_corner)));
        assert!(w.iter().any(|x| [x.a, x.b].contains(&far) && [x.a, x.b].contains(&new_corner)));
        assert!(square_and_joined(&s, &[far]));
    }

    #[test]
    fn rotating_keeps_wires_on_the_pins_and_square() {
        let (mut s, lib) = sch();
        let (r, _, _, far) = l_wire(&mut s, &lib);
        let right = s.placed_pins(&s.sheets[0].symbols[0]).map(|(_, p)| p).max_by_key(|p| p.x).unwrap();
        let free = right + Pt::mm(7.62, 0.0);
        add_wire(&mut s, 0, &[right, free]);
        for _ in 0..4 {
            let c = selection_center(&s, 0, &[SchItem::Symbol(r)]);
            rotate_items(&mut s, 0, &[SchItem::Symbol(r)], c);
            assert!(square_and_joined(&s, &[far, free]), "{:?}", s.sheets[0].wires);
            let pins: Vec<Pt> = s.placed_pins(&s.sheets[0].symbols[0]).map(|(_, p)| p).collect();
            for p in pins {
                assert!(s.sheets[0].wires.iter().any(|w| w.a == p || w.b == p), "pin at {p:?} left unwired");
            }
            let body = body_box(&s, &s.sheets[0].symbols[0]).unwrap();
            for w in &s.sheets[0].wires {
                assert!(!crosses(w.a, w.b, &body), "{w:?} runs through the body {body:?}");
            }
        }
        // Mirrored too.
        let c = selection_center(&s, 0, &[SchItem::Symbol(r)]);
        mirror_items(&mut s, 0, &[SchItem::Symbol(r)], c, true);
        assert!(square_and_joined(&s, &[far, free]));
    }

    #[test]
    fn a_wire_run_takes_one_colour() {
        let (mut s, _) = sch();
        // An L, a tee off it, and a wire that only crosses (no junction): not joined.
        let l = add_wire(&mut s, 0, &[Pt::mm(0.0, 0.0), Pt::mm(10.16, 0.0), Pt::mm(10.16, 10.16)]);
        add_wire(&mut s, 0, &[Pt::mm(5.08, 0.0), Pt::mm(5.08, -5.08)]);
        let apart = add_wire(&mut s, 0, &[Pt::mm(7.62, 5.08), Pt::mm(12.7, 5.08)]);
        let run = connected_wires(&s, 0, l[0]);
        assert!(!run.contains(&apart[0]));
        // The L's two legs and the tee (split where the tee meets it).
        assert!(run.len() >= 3, "{run:?}");
        let orange = crate::graphics::Color { r: 255, g: 153, b: 0, a: 255 };
        set_wire_color(&mut s, 0, &run, Some(orange));
        let colored = s.sheets[0].wires.iter().filter(|w| w.stroke.color == Some(orange)).count();
        assert_eq!(colored, run.len());
        assert_eq!(s.sheets[0].wires.iter().find(|w| w.id == apart[0]).unwrap().stroke.color, None);
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

    #[test]
    fn mirror_copy_paste_and_net_highlight() {
        let lib = crate::library::LibraryTable::builtin();
        let mut s = Schematic { symbols: vec![], sheets: vec![Sheet::default()] };
        let led = place_symbol(&mut s, 0, lib.symbol("Device:LED").unwrap(), Pt::mm(50.8, 50.8), Uuid::new_v4());
        let pins = |s: &Schematic| {
            let mut v: Vec<(String, Pt)> = s.placed_pins(&s.sheets[0].symbols[0]).map(|(p, at)| (p.number.clone(), at)).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };
        let before = pins(&s);
        // Left for right: the cathode (pin 1, left) goes right.
        mirror_items(&mut s, 0, &[SchItem::Symbol(led)], Pt::mm(50.8, 50.8), false);
        let after = pins(&s);
        assert_eq!(after[0].1, Pt::mm(50.8 + 3.81, 50.8));
        assert_eq!(after[1].1, Pt::mm(50.8 - 3.81, 50.8));
        // Twice is back where it was; X then Y is a half turn.
        mirror_items(&mut s, 0, &[SchItem::Symbol(led)], Pt::mm(50.8, 50.8), false);
        assert_eq!(pins(&s), before);
        mirror_items(&mut s, 0, &[SchItem::Symbol(led)], Pt::mm(50.8, 50.8), false);
        mirror_items(&mut s, 0, &[SchItem::Symbol(led)], Pt::mm(50.8, 50.8), true);
        assert_eq!(s.sheets[0].symbols[0].placement.mirror, Mirror::None);
        assert_eq!(s.sheets[0].symbols[0].placement.angle, 180.0);

        // A wire from the anode; copy both and paste them 25.4 mm up.
        let anode = pins(&s)[1].1;
        let w = add_wire(&mut s, 0, &[anode, Pt::mm(30.48, 50.8)]);
        let items = vec![SchItem::Symbol(led), SchItem::Wire(w[0])];
        let clip = copy_items(&s, 0, &items, Pt::mm(50.8, 50.8));
        assert_eq!((clip.symbols.len(), clip.wires.len(), clip.definitions.len()), (1, 1, 1));
        let pasted = paste(&mut s, 0, &clip, Pt::mm(50.8, 76.2));
        assert_eq!(pasted.len(), 2);
        let refs: Vec<&str> = s.sheets[0].symbols.iter().map(|x| x.reference()).collect();
        assert_eq!(refs, ["D1", "D2"]);
        assert!(s.sheets[0].wires.iter().any(|x| x.a.y == Pt::mm(0.0, 76.2).y));

        // The net under the first wire: that wire alone.
        let (name, hl) = net_items_at(&s, 0, Pt::mm(40.0, 50.8), crate::units::mm(0.5)).unwrap();
        assert_eq!(name, "unconnected-(D1-A-Pad2)");
        assert_eq!(hl, vec![SchItem::Wire(w[0])]);
    }
}
