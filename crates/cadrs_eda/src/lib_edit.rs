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
        // The pad's box: its size, turned a quarter if it is; any other angle, its longest side.
        let a = p.angle.rem_euclid(180.0);
        let half = if a.abs() < 0.01 {
            Pt::new(p.size.w / 2, p.size.h / 2)
        } else if (a - 90.0).abs() < 0.01 {
            Pt::new(p.size.h / 2, p.size.w / 2)
        } else {
            let r = p.size.w.max(p.size.h) / 2;
            Pt::new(r, r)
        };
        if !cy.contains(p.at - half) || !cy.contains(p.at + half) {
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
    // The generated body stays; so does the stored file while the source names it.
    let old = f.models.first();
    let body = old.and_then(|m| m.body.clone());
    let blob = old.filter(|m| m.source == source).and_then(|m| m.blob.clone());
    let m = Model3d { source: source.into(), blob, offset, rotation, scale, visible: true, opacity, body };
    match f.models.first_mut() {
        Some(x) => *x = m,
        None => f.models.push(m),
    }
}


// ---------------------------------------------------------------------------------------------
// Bulk editing: arrange a box symbol, pad arrays, renumbering

/// Lays a symbol's pins out round a body rectangle, KiCad style: pins pointing right (into the
/// body from the left) go down the left side in their order, pins pointing left down the right
/// side, pins pointing down along the top and pins pointing up along the bottom, 100 mil apart,
/// 100 mil long. The body is the symbol's first rectangle (made if there is none), sized to fit
/// the longer side (at least `min_width` wide), centred on the origin; the reference and value
/// go above it.
pub fn arrange_box(s: &mut Symbol, min_width: Nm) {
    let step = 100 * MIL;
    let side = |a: f64| match a.rem_euclid(360.0).round() as i32 {
        0 => 0,   // into the body from the left
        180 => 1, // from the right
        270 => 2, // from the top
        _ => 3,   // from the bottom
    };
    let counts: Vec<i64> = (0..4).map(|k| s.pins.iter().filter(|p| side(p.angle) == k).count() as i64).collect();
    let count = |k: i32| counts[k as usize];
    let rows = count(0).max(count(1)).max(1);
    let cols = count(2).max(count(3));
    // Half sizes on the 50 mil grid: a row of pins fits the side with 100 mil to spare at each end.
    let half_h = ((rows + 1) * step / 2 / SCHEMATIC_GRID + 1) * SCHEMATIC_GRID;
    let half_w = (((cols + 1) * step / 2).max(min_width / 2) / SCHEMATIC_GRID + 1) * SCHEMATIC_GRID;
    let mut seen = [0i64; 4];
    for p in &mut s.pins {
        let k = side(p.angle);
        let i = seen[k as usize];
        seen[k as usize] += 1;
        let n = [count(0), count(1), count(2), count(3)][k as usize];
        // Centred along the side, on the grid.
        let first = ((n - 1) * step / 2 / SCHEMATIC_GRID) * SCHEMATIC_GRID;
        p.length = step;
        p.at = match k {
            0 => Pt::new(-half_w - step, first - i * step),
            1 => Pt::new(half_w + step, first - i * step),
            2 => Pt::new(-first + i * step, half_h + step),
            _ => Pt::new(-first + i * step, -half_h - step),
        };
    }
    let rect = Geom::Rect { a: Pt::new(-half_w, half_h), b: Pt::new(half_w, -half_h) };
    match s.graphics.iter_mut().find(|g| matches!(&g.item, SymbolItem::Shape(sh) if matches!(sh.geom, Geom::Rect { .. }))) {
        Some(g) => {
            if let SymbolItem::Shape(sh) = &mut g.item {
                sh.geom = rect;
            }
        }
        None => s.graphics.insert(0, SymbolGraphic { item: SymbolItem::Shape(Shape { geom: rect, stroke: Stroke::width(mm(0.254)), fill: Fill::Background }), unit: 0, style: 0 }),
    }
    if let Some(f) = s.fields.get_mut(0) {
        f.text.at = Pt::new(0, half_h + step + 2 * SCHEMATIC_GRID);
    }
    if let Some(f) = s.fields.get_mut(1) {
        f.text.at = Pt::new(0, -half_h - step - 2 * SCHEMATIC_GRID);
    }
}

/// How a pad array numbers on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArrayShape {
    /// `count` pads, each `step` from the last.
    Line { step: Pt },
    /// `count` pads round `center`, `angle` degrees apart (counter-clockwise), turned with it.
    Circle { center: Pt, angle: f64 },
}

/// Copies pad `from` `count - 1` times along the shape, numbered on from it. Returns the new
/// pads' indices.
pub fn pad_array(f: &mut Footprint, from: usize, count: usize, shape: ArrayShape) -> Vec<usize> {
    let mut out = vec![];
    let Some(src) = f.pads.get(from).cloned() else { return out };
    let mut number = src.number.clone();
    for i in 1..count {
        number = increment(&number);
        let mut p = src.clone();
        p.id = uuid::Uuid::new_v4();
        p.number = number.clone();
        match shape {
            ArrayShape::Line { step } => p.at = src.at + Pt::new(step.x * i as Nm, step.y * i as Nm),
            ArrayShape::Circle { center, angle } => {
                let a = angle * i as f64;
                p.at = center + (src.at - center).rotated(a);
                p.angle = crate::units::normalize_deg(src.angle + a);
            }
        }
        f.pads.push(p);
        out.push(f.pads.len() - 1);
    }
    out
}

/// Numbers the numbered pads from `first` on: row by row from the top left, or (`ccw`)
/// counter-clockwise round the centre from the top of the left side, as ICs are.
pub fn renumber_pads(f: &mut Footprint, first: u32, ccw: bool) {
    let idx: Vec<usize> = (0..f.pads.len()).filter(|&i| !f.pads[i].number.is_empty()).collect();
    if idx.is_empty() {
        return;
    }
    let (mut sx, mut sy) = (0i128, 0i128);
    for &i in &idx {
        sx += f.pads[i].at.x as i128;
        sy += f.pads[i].at.y as i128;
    }
    let c = Pt::new((sx / idx.len() as i128) as Nm, (sy / idx.len() as i128) as Nm);
    let key = |p: Pt| -> (i64, i64) {
        if ccw {
            // From straight left-and-up (just past 90° from +x), going counter-clockwise.
            let a = ((p.y - c.y) as f64).atan2((p.x - c.x) as f64).to_degrees();
            let from_top_left = (a - 135.0).rem_euclid(360.0);
            ((from_top_left * 1000.0) as i64, 0)
        } else {
            (-(p.y / 1000), p.x / 1000)
        }
    };
    let mut order = idx.clone();
    order.sort_by_key(|&i| key(f.pads[i].at));
    for (k, i) in order.into_iter().enumerate() {
        f.pads[i].number = (first + k as u32).to_string();
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

#[cfg(test)]
mod bulk_tests {
    use super::*;

    #[test]
    fn arrange_pad_array_and_renumber() {
        // A box symbol: 3 pins in from the left, 2 from the right, then arranged.
        let mut s = new_symbol("MODULE", "U", true);
        for (n, o) in [("1", Orientation::Right), ("2", Orientation::Right), ("3", Orientation::Right), ("4", Orientation::Left), ("5", Orientation::Left)] {
            add_pin(&mut s, &PinProps::new("P", n, Pt::ZERO, o));
        }
        arrange_box(&mut s, mm(10.16));
        let at = |n: &str| s.pins.iter().find(|p| p.number == n).unwrap().at;
        assert_eq!(at("1").x, at("2").x);
        assert!(at("1").y > at("2").y && at("2").y > at("3").y, "down the left side");
        assert_eq!(at("2").y, 0, "centred");
        assert!(at("4").x > 0 && at("4").y > at("5").y);
        assert!(off_grid_pins(&s).is_empty(), "{:?}", s.pins.iter().map(|p| p.at.to_mm()).collect::<Vec<_>>());
        assert!(s.graphics.iter().any(|g| matches!(&g.item, SymbolItem::Shape(sh) if matches!(sh.geom, Geom::Rect { .. }))));

        // A row of four pads from one, 2.54 mm apart.
        let mut f = new_footprint("Row", "Row", MountKind::ThroughHole);
        add_pad(&mut f, Pt::ZERO);
        let made = pad_array(&mut f, 0, 4, ArrayShape::Line { step: Pt::mm(2.54, 0.0) });
        assert_eq!(made.len(), 3);
        assert_eq!((f.pads[3].number.as_str(), f.pads[3].at), ("4", Pt::mm(7.62, 0.0)));
        // Eight round a circle.
        let mut c = new_footprint("Ring", "Ring", MountKind::ThroughHole);
        add_pad(&mut c, Pt::mm(5.0, 0.0));
        pad_array(&mut c, 0, 8, ArrayShape::Circle { center: Pt::ZERO, angle: 45.0 });
        assert_eq!(c.pads[2].at, Pt::mm(0.0, 5.0));

        // SOIC-8 numbers shuffled, renumbered counter-clockwise: back to KiCad's order.
        let lib = crate::library::LibraryTable::builtin();
        let mut so = lib.footprint("Package_SO:SOIC-8_3.9x4.9mm_P1.27mm").unwrap().clone();
        let want: Vec<(String, Pt)> = so.pads.iter().map(|p| (p.number.clone(), p.at)).collect();
        so.pads.reverse();
        so.pads.iter_mut().for_each(|p| p.number = "?".into());
        renumber_pads(&mut so, 1, true);
        let mut got: Vec<(String, Pt)> = so.pads.iter().map(|p| (p.number.clone(), p.at)).collect();
        got.sort_by_key(|x| x.0.parse::<u32>().unwrap());
        assert_eq!(got, want);
    }
}
