//! Making parts (GS23–GS26): the symbol editor's and footprint editor's operations — new
//! symbol / footprint, pins and pads with "repeat last" (Insert), pad properties pushed to the
//! other pads, drawing on layers, and the checks that keep a part usable (pins on the 50 mil
//! grid, a courtyard around everything).

use crate::footprint::*;
use crate::graphics::{Fill, Geom, Shape, Stroke};
use crate::layer::Layer;
use crate::stdlib;
use crate::symbol::*;
use crate::units::{MIL, Nm, Pt, SCHEMATIC_GRID, Size, mm};

/// Which way a pin points from where wires connect towards the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Right,
    Left,
    Up,
    Down,
}

impl Orientation {
    pub fn angle(self) -> f64 {
        match self {
            Orientation::Right => 0.0,
            Orientation::Up => 90.0,
            Orientation::Left => 180.0,
            Orientation::Down => 270.0,
        }
    }
}

/// File → New symbol: a name, a reference prefix, pin names shown or not.
pub fn new_symbol(name: &str, prefix: &str, show_pin_names: bool) -> Symbol {
    let mut s = stdlib::new_symbol(name, prefix, "");
    s.show_pin_names = show_pin_names;
    // The reference starts above the body and the value below it, clear of a two-grid drawing;
    // the user moves them.
    for (f, y) in s.fields.iter_mut().zip([3, -3]) {
        f.text.at = Pt::new(0, SCHEMATIC_GRID * y);
        f.text.style.h_align = crate::graphics::HAlign::Center;
    }
    s
}

/// Pin properties (the Pin Properties dialog / the properties panel).
#[derive(Clone, Debug, PartialEq)]
pub struct PinProps {
    pub name: String,
    pub number: String,
    pub kind: PinType,
    pub shape: PinShape,
    pub at: Pt,
    pub orientation: Orientation,
    pub length: Nm,
    pub name_size: Nm,
    pub number_size: Nm,
    pub visible: bool,
}

impl PinProps {
    /// The dialog's defaults: passive, 100 mil long, 50 mil text.
    pub fn new(name: &str, number: &str, at: Pt, orientation: Orientation) -> PinProps {
        PinProps {
            name: name.into(),
            number: number.into(),
            kind: PinType::Passive,
            shape: PinShape::Line,
            at,
            orientation,
            length: 100 * MIL,
            name_size: 50 * MIL,
            number_size: 50 * MIL,
            visible: true,
        }
    }
}

fn pin_from(p: &PinProps) -> Pin {
    Pin {
        number: p.number.clone(),
        name: p.name.clone(),
        kind: p.kind,
        shape: p.shape,
        at: p.at,
        angle: p.orientation.angle(),
        length: p.length,
        visible: p.visible,
        name_size: p.name_size,
        number_size: p.number_size,
        unit: 0,
        style: 0,
    }
}

/// Adds a pin; returns its index.
pub fn add_pin(s: &mut Symbol, p: &PinProps) -> usize {
    s.pins.push(pin_from(p));
    s.pins.len() - 1
}

/// Sets a pin's properties.
pub fn set_pin(s: &mut Symbol, i: usize, p: &PinProps) {
    s.pins[i] = Pin { unit: s.pins[i].unit, style: s.pins[i].style, ..pin_from(p) };
}

/// The next of a series: "2" → "3", "D7" → "D8", "A" stays "A".
pub fn increment(text: &str) -> String {
    let digits = text.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return text.to_string();
    }
    let (head, n) = text.split_at(text.len() - digits);
    format!("{head}{}", n.parse::<u64>().unwrap_or(0) + 1)
}

/// Insert (repeat last): a copy of the last pin 100 mil lower, its number (and name, if
/// numbered) one up. Returns the new pin's index.
pub fn repeat_pin(s: &mut Symbol) -> Option<usize> {
    let mut p = s.pins.last()?.clone();
    p.at = p.at - Pt::new(0, 100 * MIL);
    p.number = increment(&p.number);
    p.name = increment(&p.name);
    s.pins.push(p);
    Some(s.pins.len() - 1)
}

/// Pins off the 50 mil grid (they couldn't connect to wires).
pub fn off_grid_pins(s: &Symbol) -> Vec<String> {
    s.pins.iter().filter(|p| p.at.x % SCHEMATIC_GRID != 0 || p.at.y % SCHEMATIC_GRID != 0).map(|p| p.number.clone()).collect()
}

/// Draws a shape on the symbol (all units, all styles).
pub fn add_symbol_shape(s: &mut Symbol, geom: Geom, width: Nm, fill: Fill) {
    s.graphics.push(SymbolGraphic { item: SymbolItem::Shape(Shape { geom, stroke: Stroke::width(width), fill }), unit: 0, style: 0 });
}

/// Symbol properties: a field's value (adding the field if new), keywords.
pub fn set_symbol_field(s: &mut Symbol, name: &str, value: &str) {
    match s.fields.iter_mut().find(|f| f.name == name) {
        Some(f) => f.text.text = value.into(),
        None => {
            let mut f = s.fields[0].clone();
            f.name = name.into();
            f.text.text = value.into();
            f.text.visible = false;
            s.fields.push(f);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Footprints

/// File → New footprint, then its properties: name, value, mount type.
pub fn new_footprint(name: &str, value: &str, mount: MountKind) -> Footprint {
    let mut f = stdlib::new_footprint(name, "", mount, Pt::new(0, mm(2.0)), Pt::new(0, -mm(2.0)), Pt::ZERO);
    if let Some(v) = f.field_mut(fields::VALUE) {
        v.text.text.text = value.into();
    }
    f
}

/// Adds a pad at `at`: the first is a through-hole circle (1.6 / 0.8 mm) numbered 1; later
/// ones copy the previous pad's properties with the next number. Returns its index.
pub fn add_pad(f: &mut Footprint, at: Pt) -> usize {
    let pad = match f.pads.last() {
        Some(last) => Pad { id: uuid::Uuid::new_v4(), number: increment(&last.number), at, ..last.clone() },
        None => {
            let drill = if f.attrs.mount == MountKind::Smd { None } else { Some(mm(0.8)) };
            stdlib::new_pad("1", PadShape::Circle, at, Size::mm(1.6, 1.6), drill)
        }
    };
    f.pads.push(pad);
    f.pads.len() - 1
}

/// Push pad properties to other pads: every other pad takes pad `from`'s shape, size, drill,
/// layers and kind (keeping its own number and position).
pub fn push_pad_properties(f: &mut Footprint, from: usize) {
    let src = f.pads[from].clone();
    for (i, p) in f.pads.iter_mut().enumerate() {
        if i != from {
            p.shape = src.shape.clone();
            p.size = src.size;
            p.drill = src.drill;
            p.layers = src.layers;
            p.kind = src.kind;
            p.angle = src.angle;
        }
    }
}

/// Draws a shape on a layer of the footprint; returns its index.
pub fn add_fp_shape(f: &mut Footprint, geom: Geom, layer: Layer, width: Nm) -> usize {
    f.shapes.push(FpShape { id: uuid::Uuid::new_v4(), shape: Shape { geom, stroke: Stroke::width(width), fill: Fill::None }, layer });
    f.shapes.len() - 1
}

/// The footprint's box on a layer.
pub fn layer_bounds(f: &Footprint, layer: Layer) -> Option<crate::units::Bounds> {
    let mut b: Option<crate::units::Bounds> = None;
    for s in f.shapes.iter().filter(|s| s.layer == layer) {
        for p in s.shape.geom.extent() {
            b = Some(crate::units::Bounds::union(b, crate::units::Bounds::of(p)));
        }
    }
    b
}

/// Problems a footprint check reports: no courtyard, pads or fabrication outline outside it.
pub fn check_footprint(f: &Footprint) -> Vec<String> {
    let mut out = vec![];
    let Some(cy) = layer_bounds(f, Layer::TopCourtyard) else {
        if !f.attrs.allow_missing_courtyard {
            out.push("Missing courtyard".into());
        }
        return out;
    };
    for p in &f.pads {
        let r = p.size.w.max(p.size.h) / 2;
        if !cy.contains(p.at - Pt::new(r, r)) || !cy.contains(p.at + Pt::new(r, r)) {
            out.push(format!("Pad {} outside the courtyard", p.number));
        }
    }
    if let Some(fab) = layer_bounds(f, Layer::TopFab)
        && (!cy.contains(fab.min) || !cy.contains(fab.max))
    {
        out.push("Fabrication outline outside the courtyard".into());
    }
    out
}

/// Sets a footprint's 3D model (the 3D Models tab), adding it if there is none.
pub fn set_model(f: &mut Footprint, source: &str, offset: [f64; 3], rotation: [f64; 3], scale: [f64; 3], opacity: f64) {
    let m = Model3d { source: source.into(), blob: None, offset, rotation, scale, visible: true, opacity };
    match f.models.first_mut() {
        Some(x) => *x = m,
        None => f.models.push(m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments() {
        assert_eq!(increment("2"), "3");
        assert_eq!(increment("D7"), "D8");
        assert_eq!(increment("A"), "A");
        assert_eq!(increment("PA09"), "PA10");
    }
}

/// A part of a symbol being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SymbolPart {
    Pin(usize),
    Graphic(usize),
    Field(usize),
}

/// The part of a symbol under `p` (symbol coordinates, within `tol`): pins, then fields,
/// then graphics.
pub fn symbol_hit(s: &Symbol, p: Pt, tol: Nm) -> Option<SymbolPart> {
    let near_seg = |a: Pt, b: Pt| {
        let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
        let l2 = dx * dx + dy * dy;
        let t = if l2 == 0.0 { 0.0 } else { (((p.x - a.x) as f64 * dx + (p.y - a.y) as f64 * dy) / l2).clamp(0.0, 1.0) };
        (p.x as f64 - (a.x as f64 + t * dx)).hypot(p.y as f64 - (a.y as f64 + t * dy)) <= tol as f64
    };
    if let Some(i) = s.pins.iter().position(|pin| near_seg(pin.at, pin.inner_end())) {
        return Some(SymbolPart::Pin(i));
    }
    if let Some(i) = s.fields.iter().position(|f| f.text.visible && crate::font::bounds(&f.text).is_some_and(|b| b.grow(tol).contains(p))) {
        return Some(SymbolPart::Field(i));
    }
    s.graphics.iter().position(|g| match &g.item {
        SymbolItem::Shape(sh) => {
            let (pts, closed) = crate::poly::geom_points(&sh.geom);
            let n = pts.len();
            (0..n.saturating_sub(if closed { 0 } else { 1 })).any(|i| near_seg(pts[i], pts[(i + 1) % n]))
        }
        SymbolItem::Text(t) => crate::font::bounds(t).is_some_and(|b| b.grow(tol).contains(p)),
    })
    .map(SymbolPart::Graphic)
}

/// Deletes parts of a symbol (highest indices first so the others stay valid).
pub fn delete_symbol_parts(s: &mut Symbol, parts: &[SymbolPart]) {
    let mut v = parts.to_vec();
    v.sort_by_key(|p| std::cmp::Reverse(match p {
        SymbolPart::Pin(i) | SymbolPart::Graphic(i) | SymbolPart::Field(i) => *i,
    }));
    for p in v {
        match p {
            SymbolPart::Pin(i) if i < s.pins.len() => {
                s.pins.remove(i);
            }
            SymbolPart::Graphic(i) if i < s.graphics.len() => {
                s.graphics.remove(i);
            }
            SymbolPart::Field(i) => {
                if let Some(f) = s.fields.get_mut(i) {
                    f.text.visible = false;
                }
            }
            _ => {}
        }
    }
}
