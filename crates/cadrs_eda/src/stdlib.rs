//! The built-in libraries: a few common symbols and generated footprints, drawn for cadrs, so
//! a design can start without any other tool's libraries.
//!
//! - `Device`: R, R_US, C, L, LED, D, Battery_Cell (pins on the 1.27 mm grid).
//! - `power`: VCC, GND, +3V3, +5V (power inputs naming a global net) and PWR_FLAG (a power
//!   output that tells ERC a net is driven).
//! - `Resistor_THT`, `LED_THT`, `Battery`, `Resistor_SMD`, `Capacitor_SMD`, `LED_SMD`: footprints
//!   from [`axial_tht`], [`radial_led_tht`], [`coin_holder`] and [`chip_smd`].

use crate::footprint::*;
use crate::graphics::{Fill, Geom, HAlign, Shape, Stroke, Text, TextStyle};
use crate::layer::{Layer, LayerSet};
use crate::library::{Library, Scope};
use crate::symbol::*;
use crate::units::{Nm, Pt, Size, mm};

fn p(x: f64, y: f64) -> Pt {
    Pt::mm(x, y)
}

fn line(pts: &[(f64, f64)], w: f64, fill: Fill) -> SymbolGraphic {
    let pts: Vec<Pt> = pts.iter().map(|&(x, y)| p(x, y)).collect();
    let closed = fill != Fill::None && pts.len() > 2;
    SymbolGraphic { item: SymbolItem::Shape(Shape { geom: Geom::Polyline { pts, closed }, stroke: Stroke::width(mm(w)), fill }), unit: 0, style: 0 }
}

fn rect(a: (f64, f64), b: (f64, f64), w: f64, fill: Fill) -> SymbolGraphic {
    SymbolGraphic { item: SymbolItem::Shape(Shape { geom: Geom::Rect { a: p(a.0, a.1), b: p(b.0, b.1) }, stroke: Stroke::width(mm(w)), fill }), unit: 0, style: 0 }
}

fn field(name: &str, value: &str, at: (f64, f64), angle: f64, h: HAlign, visible: bool) -> Field {
    let style = TextStyle { h_align: h, ..Default::default() };
    Field { name: name.into(), text: Text { text: value.into(), at: p(at.0, at.1), angle, style, visible }, show_name: false }
}

/// A pin: number, name, type, where wires connect, the direction towards the body, length.
pub fn pin(number: &str, name: &str, kind: PinType, at: (f64, f64), angle: f64, length: f64) -> Pin {
    Pin {
        number: number.into(),
        name: name.into(),
        kind,
        shape: PinShape::Line,
        at: p(at.0, at.1),
        angle,
        length: mm(length),
        visible: true,
        name_size: mm(1.27),
        number_size: mm(1.27),
        unit: 0,
        style: 0,
    }
}

/// A new symbol with the four standard fields: Reference `prefix?`, Value `name`, an empty
/// Footprint and Datasheet, and a Description.
pub fn new_symbol(name: &str, prefix: &str, description: &str) -> Symbol {
    Symbol {
        id: name.into(),
        fields: vec![
            field(fields::REFERENCE, &format!("{prefix}?"), (2.54, 1.27), 0.0, HAlign::Left, true),
            field(fields::VALUE, name, (2.54, -1.27), 0.0, HAlign::Left, true),
            field(fields::FOOTPRINT, "", (0.0, 0.0), 0.0, HAlign::Center, false),
            field(fields::DATASHEET, "~", (0.0, 0.0), 0.0, HAlign::Center, false),
            field(fields::DESCRIPTION, description, (0.0, 0.0), 0.0, HAlign::Center, false),
        ],
        keywords: String::new(),
        footprint_filters: vec![],
        unit_count: 1,
        units_swappable: false,
        unit_names: vec![],
        has_alternate: false,
        power: false,
        show_pin_numbers: true,
        show_pin_names: true,
        pin_name_offset: mm(0.508),
        in_bom: true,
        on_board: true,
        graphics: vec![],
        pins: vec![],
    }
}

/// A two-pin vertical part (pins 1 at the top, 2 at the bottom, 3.81 mm from the centre).
fn two_pin_vertical(name: &str, prefix: &str, description: &str, keywords: &str, filters: &[&str]) -> Symbol {
    let mut s = new_symbol(name, prefix, description);
    s.keywords = keywords.into();
    s.footprint_filters = filters.iter().map(|f| f.to_string()).collect();
    s.show_pin_names = false;
    s.show_pin_numbers = false;
    s.pins = vec![pin("1", "~", PinType::Passive, (0.0, 3.81), 270.0, 1.27), pin("2", "~", PinType::Passive, (0.0, -3.81), 90.0, 1.27)];
    s
}

fn device() -> Library {
    let mut lib = Library::new("Device", Scope::Global);
    lib.description = "Generic passive and discrete parts".into();

    let mut r = two_pin_vertical("R", "R", "Resistor", "R res resistor", &["R_*"]);
    r.graphics.push(rect((-1.016, -2.54), (1.016, 2.54), 0.254, Fill::None));
    lib.put_symbol(r);

    let mut rus = two_pin_vertical("R_US", "R", "Resistor, US symbol", "R res resistor", &["R_*"]);
    rus.graphics.push(line(
        &[(0.0, 2.54), (0.0, 2.286), (1.016, 1.905), (-1.016, 1.143), (1.016, 0.381), (-1.016, -0.381), (1.016, -1.143), (-1.016, -1.905), (0.0, -2.286), (0.0, -2.54)],
        0.254,
        Fill::None,
    ));
    lib.put_symbol(rus);

    let mut c = two_pin_vertical("C", "C", "Unpolarized capacitor", "cap capacitor", &["C_*"]);
    c.pins.iter_mut().for_each(|pn| pn.length = mm(3.048));
    c.graphics.push(line(&[(-2.032, 0.762), (2.032, 0.762)], 0.508, Fill::None));
    c.graphics.push(line(&[(-2.032, -0.762), (2.032, -0.762)], 0.508, Fill::None));
    lib.put_symbol(c);

    let mut l = two_pin_vertical("L", "L", "Inductor", "inductor choke coil reactor magnetic", &["Choke_*", "*Coil*", "Inductor_*", "L_*"]);
    for i in 0..4 {
        let y0 = 2.54 - 1.27 * i as f64;
        l.graphics.push(SymbolGraphic {
            item: SymbolItem::Shape(Shape {
                geom: Geom::Arc { start: p(0.0, y0), mid: p(0.635, y0 - 0.635), end: p(0.0, y0 - 1.27) },
                stroke: Stroke::width(mm(0.254)),
                fill: Fill::None,
            }),
            unit: 0,
            style: 0,
        });
    }
    lib.put_symbol(l);

    // LED and diode: cathode (pin 1, K) left, anode (pin 2, A) right.
    let diode = |name: &str, desc: &str, kw: &str, filters: &[&str], led: bool| {
        let mut d = new_symbol(name, "D", desc);
        d.keywords = kw.into();
        d.footprint_filters = filters.iter().map(|f| f.to_string()).collect();
        d.show_pin_names = false;
        d.show_pin_numbers = false;
        d.pins = vec![pin("1", "K", PinType::Passive, (-3.81, 0.0), 0.0, 2.54), pin("2", "A", PinType::Passive, (3.81, 0.0), 180.0, 2.54)];
        d.graphics.push(line(&[(-1.27, -1.27), (-1.27, 1.27)], 0.254, Fill::None));
        d.graphics.push(line(&[(1.27, -1.27), (1.27, 1.27), (-1.27, 0.0), (1.27, -1.27)], 0.254, Fill::None));
        d.graphics.push(line(&[(-1.27, 0.0), (1.27, 0.0)], 0.254, Fill::None));
        if led {
            for dx in [0.0, 1.016] {
                d.graphics.push(line(&[(-0.508 - dx, -1.524), (-1.778 - dx, -2.794), (-1.016 - dx, -2.794)], 0.0, Fill::None));
                d.graphics.push(line(&[(-1.778 - dx, -2.794), (-1.778 - dx, -2.032)], 0.0, Fill::None));
            }
        }
        d.fields[0].text.at = p(0.0, 2.54);
        d.fields[1].text.at = p(0.0, -3.81);
        d.fields.iter_mut().take(2).for_each(|f| f.text.style.h_align = HAlign::Center);
        d
    };
    lib.put_symbol(diode("LED", "Light emitting diode", "LED diode", &["LED*", "LED_SMD:*", "LED_THT:*"], true));
    lib.put_symbol(diode("D", "Diode", "diode", &["TO-???*", "*_Diode_*", "*SingleDiode*", "D_*"], false));

    let mut bt = new_symbol("Battery_Cell", "BT", "Single-cell battery");
    bt.keywords = "battery cell".into();
    bt.show_pin_names = false;
    bt.show_pin_numbers = false;
    bt.pins = vec![pin("1", "+", PinType::Passive, (0.0, 3.81), 270.0, 2.54), pin("2", "-", PinType::Passive, (0.0, -3.81), 90.0, 2.54)];
    bt.graphics.push(rect((-2.032, 0.762), (2.032, 1.016), 0.254, Fill::Outline));
    bt.graphics.push(rect((-1.27, 0.254), (1.27, -0.254), 0.254, Fill::Outline));
    bt.graphics.push(line(&[(0.0, 0.254), (0.0, 0.0)], 0.254, Fill::None));
    bt.graphics.push(line(&[(0.0, 1.016), (0.0, 1.27)], 0.254, Fill::None));
    bt.graphics.push(line(&[(0.762, 2.286), (1.778, 2.286)], 0.254, Fill::None));
    bt.graphics.push(line(&[(1.27, 2.794), (1.27, 1.778)], 0.254, Fill::None));
    lib.put_symbol(bt);
    lib
}

/// A power port: a hidden power-input pin at the origin naming the net after its value.
fn power_port(name: &str, ground: bool) -> Symbol {
    let mut s = new_symbol(name, "#PWR", &format!("Power symbol creates a global label with name \"{name}\""));
    s.power = true;
    s.keywords = "global power".into();
    s.show_pin_names = false;
    s.show_pin_numbers = false;
    s.in_bom = false;
    s.on_board = false;
    let mut pn = pin("1", name, PinType::PowerIn, (0.0, 0.0), if ground { 270.0 } else { 90.0 }, 0.0);
    pn.visible = false;
    s.pins = vec![pn];
    s.fields[0].text.visible = false;
    if ground {
        s.graphics.push(line(&[(0.0, 0.0), (0.0, -1.27), (1.27, -1.27), (0.0, -2.54), (-1.27, -1.27), (0.0, -1.27)], 0.254, Fill::None));
        s.fields[1].text.at = p(0.0, -3.81);
    } else {
        s.graphics.push(line(&[(0.0, 0.0), (0.0, 1.905)], 0.254, Fill::None));
        s.graphics.push(line(&[(-0.762, 1.27), (0.0, 2.54), (0.762, 1.27)], 0.254, Fill::None));
        s.fields[1].text.at = p(0.0, 3.556);
    }
    s.fields[1].text.style.h_align = HAlign::Center;
    s
}

fn power() -> Library {
    let mut lib = Library::new("power", Scope::Global);
    lib.description = "Power ports and flags".into();
    lib.put_symbol(power_port("VCC", false));
    lib.put_symbol(power_port("+3V3", false));
    lib.put_symbol(power_port("+5V", false));
    lib.put_symbol(power_port("GND", true));
    let mut flag = new_symbol("PWR_FLAG", "#FLG", "Special symbol for telling ERC where power comes from");
    flag.power = true;
    flag.keywords = "flag power".into();
    flag.in_bom = false;
    flag.on_board = false;
    flag.show_pin_names = false;
    flag.show_pin_numbers = false;
    flag.pins = vec![pin("1", "pwr", PinType::PowerOut, (0.0, 0.0), 90.0, 0.0)];
    flag.fields[0].text.visible = false;
    flag.graphics.push(line(&[(0.0, 0.0), (0.0, 1.27), (-1.016, 1.905), (0.0, 2.54), (1.016, 1.905), (0.0, 1.27)], 0.0, Fill::None));
    flag.fields[1].text.at = p(0.0, 3.302);
    flag.fields[1].text.style.h_align = HAlign::Center;
    lib.put_symbol(flag);
    lib
}

// ---------------------------------------------------------------------------------------------
// Footprints

fn fp_text(text: &str, at: Pt, layer: Layer, size: f64, thickness: f64) -> FpText {
    FpText {
        id: uuid::Uuid::new_v4(),
        text: Text { text: text.into(), at, angle: 0.0, style: TextStyle { size: Size::mm(size, size), thickness: Some(mm(thickness)), ..Default::default() }, visible: true },
        layer,
        keep_upright: true,
    }
}

fn fp_shape(geom: Geom, layer: Layer, width: f64) -> FpShape {
    FpShape { id: uuid::Uuid::new_v4(), shape: Shape { geom, stroke: Stroke::width(mm(width)), fill: Fill::None }, layer }
}

/// An empty footprint `lib:name` with Reference on silkscreen at `ref_at`, Value on fab at
/// `value_at`, and `${REFERENCE}` on fab at the body's centre.
pub fn new_footprint(id: &str, description: &str, mount: MountKind, ref_at: Pt, value_at: Pt, center: Pt) -> Footprint {
    Footprint {
        id: id.into(),
        description: description.into(),
        keywords: String::new(),
        fields: vec![
            FpField { name: fields::REFERENCE.into(), text: fp_text("REF**", ref_at, Layer::TopSilk, 1.0, 0.15) },
            FpField { name: fields::VALUE.into(), text: fp_text(id.rsplit(':').next().unwrap_or(id), value_at, Layer::TopFab, 1.0, 0.15) },
        ],
        attrs: FootprintAttrs { mount, ..Default::default() },
        pads: vec![],
        shapes: vec![],
        texts: vec![fp_text("${REFERENCE}", center, Layer::TopFab, 1.0, 0.15)],
        models: vec![],
        zones: vec![],
    }
}

/// A pad: plated through-hole when `drill` is set, else SMD on the top layers.
pub fn new_pad(number: &str, shape: PadShape, at: Pt, size: Size, drill: Option<Nm>) -> Pad {
    let (kind, layers) = match drill {
        Some(_) => (PadKind::ThroughHole, LayerSet::ALL_COPPER.union(LayerSet::of(&[Layer::TopMask, Layer::BottomMask]))),
        None => (PadKind::Smd, LayerSet::of(&[Layer::TopCopper, Layer::TopMask, Layer::TopPaste])),
    };
    Pad {
        id: uuid::Uuid::new_v4(),
        number: number.into(),
        kind,
        shape,
        at,
        angle: 0.0,
        size,
        drill: drill.map(|d| Drill { size: Size::new(d, d), offset: Pt::ZERO }),
        layers,
        net: None,
        pin_function: String::new(),
        pin_type: String::new(),
        rules: PadRules::default(),
        die_length: 0,
    }
}

fn model(path: &str) -> Model3d {
    Model3d { source: path.into(), blob: None, offset: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3], visible: true, opacity: 1.0 }
}

fn rect_geom(x0: f64, y0: f64, x1: f64, y1: f64) -> Geom {
    Geom::Rect { a: p(x0, y0), b: p(x1, y1) }
}

/// An axial through-hole part lying flat: pads at x = 0 and `pitch`, a `length` × `diameter`
/// body between them. KiCad-style name: `R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal`.
pub fn axial_tht(lib: &str, name: &str, pitch: f64, length: f64, diameter: f64, drill: f64, pad: f64) -> Footprint {
    let id = format!("{lib}:{name}");
    let (x0, x1) = ((pitch - length) / 2.0, (pitch + length) / 2.0);
    let r = diameter / 2.0;
    let mut f = new_footprint(&id, &format!("Axial, horizontal, pin pitch {pitch} mm, body {length} × {diameter} mm"), MountKind::ThroughHole, p(pitch / 2.0, r + 1.0), p(pitch / 2.0, -r - 1.0), p(pitch / 2.0, 0.0));
    f.pads.push(new_pad("1", PadShape::Circle, p(0.0, 0.0), Size::mm(pad, pad), Some(mm(drill))));
    f.pads.push(new_pad("2", PadShape::Circle, p(pitch, 0.0), Size::mm(pad, pad), Some(mm(drill))));
    f.shapes.push(fp_shape(rect_geom(x0, -r, x1, r), Layer::TopFab, 0.1));
    f.shapes.push(fp_shape(Geom::Line { a: p(0.0, 0.0), b: p(x0, 0.0) }, Layer::TopFab, 0.1));
    f.shapes.push(fp_shape(Geom::Line { a: p(pitch, 0.0), b: p(x1, 0.0) }, Layer::TopFab, 0.1));
    let s = 0.12;
    f.shapes.push(fp_shape(rect_geom(x0 - s, -r - s, x1 + s, r + s), Layer::TopSilk, 0.12));
    f.shapes.push(fp_shape(rect_geom(-pad / 2.0 - 0.25, -r - 0.25 - s, pitch + pad / 2.0 + 0.25, r + 0.25 + s), Layer::TopCourtyard, 0.05));
    f.models.push(model(&format!("{lib}.3dshapes/{name}.step")));
    f
}

/// A round radial LED: pad 1 (cathode, square) at the origin, pad 2 at 2.54 mm.
pub fn radial_led_tht(diameter: f64) -> Footprint {
    let name = format!("LED_D{diameter:.1}mm");
    let id = format!("LED_THT:{name}");
    let c = p(1.27, 0.0);
    let r = diameter / 2.0;
    let mut f = new_footprint(&id, &format!("LED, diameter {diameter} mm, 2 pins"), MountKind::ThroughHole, p(1.27, r + 1.0), p(1.27, -r - 1.0), c);
    f.pads.push(new_pad("1", PadShape::Rect, p(0.0, 0.0), Size::mm(1.8, 1.8), Some(mm(0.9))));
    f.pads.push(new_pad("2", PadShape::Circle, p(2.54, 0.0), Size::mm(1.8, 1.8), Some(mm(0.9))));
    f.shapes.push(fp_shape(Geom::Circle { center: c, radius: mm(r) }, Layer::TopFab, 0.1));
    f.shapes.push(fp_shape(Geom::Circle { center: c, radius: mm(r + 0.12) }, Layer::TopSilk, 0.12));
    f.shapes.push(fp_shape(Geom::Circle { center: c, radius: mm(r + 0.5) }, Layer::TopCourtyard, 0.05));
    f.models.push(model(&format!("LED_THT.3dshapes/{name}.step")));
    f
}

/// A surface-mount coin-cell holder for one 20 mm cell: pads 1 (+) left and 2 (−) right.
pub fn coin_holder() -> Footprint {
    let id = "Battery:BatteryHolder_Keystone_1058_1x2032";
    let mut f = new_footprint(id, "Coin cell holder, CR2032, surface mount", MountKind::Smd, p(0.0, 12.0), p(0.0, -12.0), p(0.0, 0.0));
    f.pads.push(new_pad("1", PadShape::Rect, p(-14.73, 0.0), Size::mm(2.54, 5.08), None));
    f.pads.push(new_pad("2", PadShape::Rect, p(14.73, 0.0), Size::mm(2.54, 5.08), None));
    f.shapes.push(fp_shape(Geom::Circle { center: Pt::ZERO, radius: mm(10.0) }, Layer::TopFab, 0.1));
    f.shapes.push(fp_shape(
        Geom::Polyline {
            pts: [(-13.0, 3.5), (-8.0, 3.5), (-6.0, 9.0), (6.0, 9.0), (8.0, 3.5), (13.0, 3.5), (13.0, -3.5), (8.0, -3.5), (5.0, -8.0), (3.0, -6.5), (-3.0, -6.5), (-5.0, -8.0), (-8.0, -3.5), (-13.0, -3.5)]
                .iter()
                .map(|&(x, y)| p(x, y))
                .collect(),
            closed: true,
        },
        Layer::TopSilk,
        0.12,
    ));
    f.shapes.push(fp_shape(rect_geom(-16.5, -11.0, 16.5, 11.0), Layer::TopCourtyard, 0.05));
    f.models.push(model("Battery.3dshapes/BatteryHolder_Keystone_1058_1x2032.step"));
    f
}

/// A two-terminal chip part (0402 … 2512 style): pads at ±`pitch`/2.
pub fn chip_smd(lib: &str, name: &str, pitch: f64, pad: Size, body: Size) -> Footprint {
    let id = format!("{lib}:{name}");
    let (bw, bh) = (crate::units::to_mm(body.w), crate::units::to_mm(body.h));
    let mut f = new_footprint(&id, &format!("Chip, {bw} × {bh} mm"), MountKind::Smd, p(0.0, bh / 2.0 + 1.0), p(0.0, -bh / 2.0 - 1.0), Pt::ZERO);
    f.pads.push(new_pad("1", PadShape::RoundRect { ratio: 0.25 }, p(-pitch / 2.0, 0.0), pad, None));
    f.pads.push(new_pad("2", PadShape::RoundRect { ratio: 0.25 }, p(pitch / 2.0, 0.0), pad, None));
    f.shapes.push(fp_shape(rect_geom(-bw / 2.0, -bh / 2.0, bw / 2.0, bh / 2.0), Layer::TopFab, 0.1));
    let (cx, cy) = (pitch / 2.0 + crate::units::to_mm(pad.w) / 2.0 + 0.25, (bh / 2.0).max(crate::units::to_mm(pad.h) / 2.0) + 0.25);
    f.shapes.push(fp_shape(rect_geom(-cx, -cy, cx, cy), Layer::TopCourtyard, 0.05));
    f.models.push(model(&format!("{lib}.3dshapes/{name}.step")));
    f
}

/// Every built-in library.
pub fn libraries() -> Vec<Library> {
    let mut out = vec![device(), power()];
    let mut add = |name: &str, desc: &str, fps: Vec<Footprint>| {
        let mut l = Library::new(name, Scope::Global);
        l.description = desc.into();
        fps.into_iter().for_each(|f| l.put_footprint(f));
        out.push(l);
    };
    add(
        "Resistor_THT",
        "Through-hole resistors",
        vec![
            axial_tht("Resistor_THT", "R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal", 10.16, 6.3, 2.5, 0.8, 1.6),
            axial_tht("Resistor_THT", "R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal", 12.7, 9.0, 3.2, 0.8, 1.6),
        ],
    );
    add("LED_THT", "Through-hole LEDs", vec![radial_led_tht(3.0), radial_led_tht(5.0)]);
    add("Battery", "Battery holders", vec![coin_holder()]);
    let chip = |lib: &str, name: &str| chip_smd(lib, name, 1.9, Size::mm(1.0, 1.45), Size::mm(2.0, 1.25));
    add("Resistor_SMD", "Surface-mount resistors", vec![chip("Resistor_SMD", "R_0805_2012Metric")]);
    add("Capacitor_SMD", "Surface-mount capacitors", vec![chip("Capacitor_SMD", "C_0805_2012Metric")]);
    add("LED_SMD", "Surface-mount LEDs", vec![chip("LED_SMD", "LED_0805_2012Metric")]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::SCHEMATIC_GRID;

    #[test]
    fn symbol_pins_sit_on_the_grid() {
        for lib in libraries() {
            for s in &lib.symbols {
                for pn in &s.pins {
                    assert_eq!((pn.at.x % SCHEMATIC_GRID, pn.at.y % SCHEMATIC_GRID), (0, 0), "{} pin {}", s.id, pn.number);
                }
                assert!(s.field(fields::REFERENCE).is_some() && s.field(fields::VALUE).is_some());
            }
        }
    }

    #[test]
    fn course_parts_exist() {
        let t = crate::library::LibraryTable::builtin();
        for id in ["Device:LED", "Device:R_US", "Device:Battery_Cell", "power:VCC", "power:GND", "power:PWR_FLAG"] {
            assert!(t.symbol(id).is_some(), "{id}");
        }
        for id in ["Battery:BatteryHolder_Keystone_1058_1x2032", "LED_THT:LED_D5.0mm", "Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal"] {
            let f = t.footprint(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(f.pads.len(), 2);
        }
        let r = t.footprint("Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal").unwrap();
        assert_eq!(r.pads[1].at, Pt::mm(12.7, 0.0));
        assert!(t.symbol("power:GND").unwrap().power);
        assert_eq!(t.symbol("power:PWR_FLAG").unwrap().pins[0].kind, PinType::PowerOut);
    }
}
