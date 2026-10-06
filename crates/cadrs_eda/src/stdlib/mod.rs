//! The built-in libraries, drawn and generated for cadrs (no other tool's library data), with
//! KiCad's library, part and footprint names so designs read the same:
//!
//! - Symbols ([`symbols`]): `Device` (passives, diodes, transistors, crystal, antenna, …),
//!   `Switch`, `Connector` (1×N and 2×N generic connectors, screw terminals, test point),
//!   `power` (supply and ground ports, PWR_FLAG), `Regulator_Linear`, `Amplifier_Operational`,
//!   `LED`. Pins on the 1.27 mm grid.
//! - Footprints ([`footprints`]): chips 0201–2512, SOD/SMA diodes, SOT-23/223, SOIC/TSSOP/MSOP,
//!   QFN, LQFP/TQFP, pin headers and sockets, JST PH/XH, crystals, radial electrolytics, axial
//!   and TO-92 parts, mounting holes, test points, a push button, WS2812B. Each has a
//!   generated 3D body ([`crate::model3d`]).

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

/// A model file reference with its generated body (shown when the file isn't there).
fn model(path: &str, body: crate::model3d::Body) -> Model3d {
    Model3d { body: Some(body), ..Model3d::file(path) }
}

fn rect_geom(x0: f64, y0: f64, x1: f64, y1: f64) -> Geom {
    Geom::Rect { a: p(x0, y0), b: p(x1, y1) }
}

pub mod footprints;
pub mod symbols;

pub use footprints::{axial_tht, chip_smd, coin_holder, pin_header, radial_led_tht};
pub use symbols::connector;

/// Every built-in library (made once, then copied).
pub fn libraries() -> Vec<Library> {
    static ALL: std::sync::OnceLock<Vec<Library>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut out = vec![symbols::device(), symbols::power(), symbols::switch(), symbols::connector_lib(), symbols::regulators(), symbols::opamps(), symbols::leds()];
        out.extend(footprints::footprint_libraries());
        out
    })
    .clone()
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

    use crate::units::to_mm;

    /// The pad's box (mm).
    fn pad_box(pd: &Pad) -> (f64, f64, f64, f64) {
        let (mut w, mut h) = (to_mm(pd.size.w), to_mm(pd.size.h));
        if (pd.angle.rem_euclid(180.0) - 90.0).abs() < 1.0 {
            std::mem::swap(&mut w, &mut h);
        }
        let (x, y) = (to_mm(pd.at.x), to_mm(pd.at.y));
        (x - w / 2.0, y - h / 2.0, x + w / 2.0, y + h / 2.0)
    }

    fn box_dist(b: (f64, f64, f64, f64), x: f64, y: f64) -> f64 {
        let dx = (b.0 - x).max(x - b.2).max(0.0);
        let dy = (b.1 - y).max(y - b.3).max(0.0);
        dx.hypot(dy)
    }

    #[test]
    fn libraries_are_broad() {
        let all = libraries();
        let symbols: usize = all.iter().map(|l| l.symbols.len()).sum();
        let footprints: usize = all.iter().map(|l| l.footprints.len()).sum();
        assert!(symbols > 120, "{symbols} symbols");
        assert!(footprints > 300, "{footprints} footprints");
        let t = crate::library::LibraryTable::builtin();
        for id in ["Device:Antenna", "Device:C", "Device:L", "Device:Q_NPN_BEC", "Device:Crystal", "power:GNDREF", "power:+3V3", "Connector:Conn_02x10_Odd_Even", "Switch:SW_Push", "Regulator_Linear:AMS1117-3.3", "LED:WS2812B"] {
            assert!(t.symbol(id).is_some(), "{id}");
        }
        for id in ["Capacitor_SMD:C_0805_2012Metric", "Inductor_SMD:L_0805_2012Metric", "Connector_PinHeader_2.54mm:PinHeader_2x10_P2.54mm_Vertical", "Package_TO_SOT_SMD:SOT-223-3_TabPin2", "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm", "Package_DFN_QFN:QFN-32-1EP_5x5mm_P0.5mm_EP3.45x3.45mm"] {
            assert!(t.footprint(id).is_some(), "{id}");
        }
    }

    #[test]
    fn footprints_are_well_formed() {
        for lib in libraries() {
            for f in &lib.footprints {
                assert_eq!(crate::lib_edit::check_footprint(f), Vec::<String>::new(), "{}", f.id);
                assert!(f.models.first().and_then(|m| m.body.as_ref()).is_some() || f.id.starts_with("MountingHole") || f.id.starts_with("TestPoint"), "{} has no 3D body", f.id);
                // Pads of different numbers don't overlap.
                for (i, a) in f.pads.iter().enumerate() {
                    for b in &f.pads[i + 1..] {
                        if a.number == b.number {
                            continue;
                        }
                        let (p, q) = (pad_box(a), pad_box(b));
                        let overlap = p.0 < q.2 && q.0 < p.2 && p.1 < q.3 && q.1 < p.3;
                        assert!(!overlap, "{}: pads {} and {} overlap", f.id, a.number, b.number);
                    }
                }
                // Silkscreen stays off the copper.
                for s in f.shapes.iter().filter(|s| s.layer == Layer::TopSilk) {
                    let (pts, closed) = crate::poly::geom_points(&s.shape.geom);
                    let n = pts.len();
                    let segs = if closed { n } else { n.saturating_sub(1) };
                    for k in 0..segs {
                        let (a, b) = (pts[k], pts[(k + 1) % n]);
                        for t in 0..=20 {
                            let u = t as f64 / 20.0;
                            let (x, y) = (to_mm(a.x) + (to_mm(b.x) - to_mm(a.x)) * u, to_mm(a.y) + (to_mm(b.y) - to_mm(a.y)) * u);
                            for pd in &f.pads {
                                let to_pad = if pd.shape == PadShape::Circle { (x - to_mm(pd.at.x)).hypot(y - to_mm(pd.at.y)) - to_mm(pd.size.w) / 2.0 } else { box_dist(pad_box(pd), x, y) };
                                let d = to_pad - to_mm(s.shape.stroke.width) / 2.0;
                                assert!(d > 0.05, "{}: silkscreen {d:.3} mm from pad {}", f.id, pd.number);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn footprints_match_kicad_pin_positions() {
        let t = crate::library::LibraryTable::builtin();
        let at = |id: &str, n: &str| t.footprint(id).unwrap().pads.iter().find(|p| p.number == n).unwrap().at;
        // Pin headers: pin 1 at the origin, 2 beside it, rows going down.
        let h = "Connector_PinHeader_2.54mm:PinHeader_2x10_P2.54mm_Vertical";
        assert_eq!((at(h, "1"), at(h, "2"), at(h, "20")), (Pt::ZERO, Pt::mm(2.54, 0.0), Pt::mm(2.54, -22.86)));
        // SOIC-8: pin 1 top-left, 8 top-right.
        let so = "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm";
        assert_eq!((at(so, "1"), at(so, "4"), at(so, "8")), (Pt::mm(-2.475, 1.905), Pt::mm(-2.475, -1.905), Pt::mm(2.475, 1.905)));
        // SOT-23: 1 and 2 on the left, 3 on the right.
        let sot = "Package_TO_SOT_SMD:SOT-23";
        assert_eq!((at(sot, "1"), at(sot, "3")), (Pt::mm(-1.1375, 0.95), Pt::mm(1.1375, 0.0)));
        // QFN-32: 32 pins + the exposed pad 33.
        let q = t.footprint("Package_DFN_QFN:QFN-32-1EP_5x5mm_P0.5mm_EP3.45x3.45mm").unwrap();
        assert_eq!(q.pads.len(), 33);
        assert_eq!(at("Package_DFN_QFN:QFN-32-1EP_5x5mm_P0.5mm_EP3.45x3.45mm", "1"), Pt::mm(-2.45, 1.75));
    }
}
