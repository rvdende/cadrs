//! The desk power monitor's LoRa board — the KiCad project `desk_power_monitor_mini32_lora`
//! (an ESP32 mini module's two headers, an Ai-Thinker Ra-01SH LoRa module, an 868/915 MHz PCB
//! antenna with its matching network) — redrawn in cadrs with the same operations its editors
//! use: the module symbols and the two custom footprints built in the symbol and footprint
//! editors ([`crate::lib_edit`]), the schematic placed, turned and wired ([`crate::sch_edit`]),
//! Update PCB ([`crate::forward`]), the footprints placed and the board routed
//! ([`crate::board_edit`]). Nothing is read from KiCad's files: positions were taken from the
//! project once, and `cadrs_kicad`'s tests compare this design with the imported one (same
//! connections, same placement, same copper).
//!
//! Coordinates are mm in cadrs' frames (Y up): sheet positions from the page's bottom-left
//! corner (KiCad's sheet grid lands on cadrs' 50 mil grid), board positions as KiCad's with
//! Y negated.

use uuid::Uuid;

use crate::Design;
use crate::board::{BoardShape, Keepout, Zone, ZoneFill};
use crate::board_edit as be;
use crate::footprint::{Footprint, FpText, MountKind, PadKind, PadShape};
use crate::forward;
use crate::graphics::{Fill, Geom, Shape, Stroke, Text, TextStyle};
use crate::layer::{Layer, LayerSet};
use crate::lib_edit::{self as le, Orientation, PinProps};
use crate::library::{Library, LibraryTable, Scope};
use crate::model3d::{Body, colors};
use crate::sch_edit::{self as se, SchItem};
use crate::schematic::LabelKind;
use crate::symbol::{PinType, Symbol, fields};
use crate::units::{Pt, Size, mm};

/// The project library's name.
pub const LIB: &str = "power_monitor";

fn p(x: f64, y: f64) -> Pt {
    Pt::mm(x, y)
}

// ---------------------------------------------------------------------------------------------
// Symbols (the symbol editor)

/// A pin as the Pin Properties dialog makes it.
fn pin(s: &mut Symbol, number: &str, name: &str, kind: PinType, at: (f64, f64), o: Orientation, length: f64) {
    let mut props = PinProps::new(name, number, p(at.0, at.1), o);
    props.kind = kind;
    props.length = mm(length);
    le::add_pin(s, &props);
}

/// The Ra-01SH LoRa module: pins 1–8 down the left, 9–16 up the right.
pub fn ra01sh_symbol() -> Symbol {
    let mut s = le::new_symbol("RA-01SH", "U", true);
    le::set_symbol_field(&mut s, fields::VALUE, "RA-01SH");
    le::set_symbol_field(&mut s, fields::FOOTPRINT, &format!("{LIB}:WIRELM-SMD_RA-01SH"));
    le::set_symbol_field(&mut s, fields::DESCRIPTION, "Ai-Thinker Ra-01SH 868 MHz LoRa module, SPI interface, external antenna");
    s.keywords = "LoRa SX1262 module".into();
    le::add_symbol_shape(&mut s, Geom::Rect { a: p(-8.89, 11.43), b: p(8.89, -11.43) }, mm(0.254), Fill::Background);
    let left = ["ANT", "GND", "3.3V", "RESET", "TXEN", "DIO1", "DIO2", "DIO3"];
    for (i, name) in left.iter().enumerate() {
        pin(&mut s, &(i + 1).to_string(), name, PinType::Unspecified, (-11.43, 8.89 - 2.54 * i as f64), Orientation::Right, 2.54);
    }
    let right = ["GND", "NSS", "MOSI", "MISO", "SCK", "RXEN", "BUSY", "GND"];
    for (i, name) in right.iter().enumerate() {
        pin(&mut s, &(16 - i).to_string(), name, PinType::Unspecified, (11.43, 8.89 - 2.54 * i as f64), Orientation::Left, 2.54);
    }
    s.fields[0].text.at = p(0.0, 13.97);
    s.fields[1].text.at = p(0.0, -13.97);
    s
}

/// A header column's pins: (name, electrical type) from the top.
type PinColumn = [(&'static str, PinType); 10];

/// One of the ESP32 mini module's two 2×10 headers: odd pins left, even pins right.
pub fn esp32_header_symbol(right: bool) -> Symbol {
    use PinType::{Bidirectional as Bi, Input, NoConnect as Nc, Output, Passive as Pa, PowerIn as Pi, PowerOut as Po};
    let (name, odd, even): (&str, PinColumn, PinColumn) = if right {
        (
            "esp32-pins-header-right",
            [("TXD", Output), ("RXD", Input), ("IO22", Bi), ("IO21", Bi), ("IO17", Bi), ("IO16", Bi), ("GND", Pi), ("VCC", Pi), ("TD0", Bi), ("SD0", Bi)],
            [("GND", Pi), ("IO27", Bi), ("IO25", Bi), ("IO32", Bi), ("TDI", Pa), ("IO4", Bi), ("IO0", Bi), ("IO2", Bi), ("SD1", Bi), ("CLK", Pa)],
        )
    } else {
        (
            "esp32-pins-header-left",
            [("GND", Pi), ("NC", Nc), ("SVN", Pa), ("IO35", Bi), ("IO33", Bi), ("IO34", Bi), ("TMS", Pa), ("NC", Nc), ("SD2", Pa), ("CMD", Pa)],
            [("RST", Input), ("SVP", Pa), ("IO26", Bi), ("IO18", Bi), ("IO19", Bi), ("IO23", Bi), ("IO5", Bi), ("3V3", Po), ("TCK", Pa), ("SD3", Pa)],
        )
    };
    let mut s = le::new_symbol(name, "J", true);
    le::set_symbol_field(&mut s, fields::FOOTPRINT, "Connector_PinHeader_2.54mm:PinHeader_2x10_P2.54mm_Vertical");
    le::set_symbol_field(&mut s, fields::DESCRIPTION, name);
    s.keywords = "ESP32 mini header".into();
    le::add_symbol_shape(&mut s, Geom::Rect { a: p(-1.27, 11.43), b: p(15.24, -13.97) }, mm(0.254), Fill::Background);
    for i in 0..10 {
        let y = 10.16 - 2.54 * i as f64;
        pin(&mut s, &(2 * i + 1).to_string(), odd[i].0, odd[i].1, (-5.08, y), Orientation::Right, 3.81);
        pin(&mut s, &(2 * i + 2).to_string(), even[i].0, even[i].1, (19.05, y), Orientation::Left, 3.81);
    }
    s.fields[0].text.at = p(6.985, 16.51);
    s.fields[1].text.at = p(6.985, 13.97);
    s
}

// ---------------------------------------------------------------------------------------------
// Footprints (the footprint editor)

fn fp_text(text: &str, at: Pt, layer: Layer) -> FpText {
    FpText { id: Uuid::new_v4(), text: Text { text: text.into(), at, angle: 0.0, style: TextStyle { size: Size::mm(1.0, 1.0), thickness: Some(mm(0.15)), ..Default::default() }, visible: true }, layer, keep_upright: true }
}

/// The Ra-01SH's land pattern: 16 castellated pads on a 2 mm pitch, 16 mm apart.
pub fn ra01sh_footprint() -> Footprint {
    let mut f = le::new_footprint("WIRELM-SMD_RA-01SH", "WIRELM-SMD_RA-01SH", MountKind::Smd);
    f.description = "Ai-Thinker Ra-01SH LoRa module, 16 castellated pads".into();
    // Pad 1 (the antenna feed) first, then the rest copy it, numbered on.
    let first = le::add_pad(&mut f, p(-7.7, 7.0));
    {
        let pd = &mut f.pads[first];
        pd.shape = PadShape::Rect;
        pd.size = Size::mm(3.0, 1.2);
    }
    let rest: Vec<(f64, f64)> = (1..8).map(|i| (-8.0, 7.0 - 2.0 * i as f64)).chain((0..8).map(|i| (8.0, -7.0 + 2.0 * i as f64))).collect();
    for (x, y) in rest {
        let i = le::add_pad(&mut f, p(x, y));
        f.pads[i].size = Size::mm(2.4, 1.2);
    }
    // Silkscreen: the module's edge, broken at each pad.
    let w = mm(0.25);
    for x in [-8.0, 8.0] {
        le::add_fp_shape(&mut f, Geom::Line { a: p(x, 8.51), b: p(x, 7.83) }, Layer::TopSilk, w);
        le::add_fp_shape(&mut f, Geom::Line { a: p(x, -8.51), b: p(x, -7.83) }, Layer::TopSilk, w);
        for k in 0..7 {
            let y = 5.83 - 2.0 * k as f64;
            le::add_fp_shape(&mut f, Geom::Line { a: p(x, y), b: p(x, y + 0.34) }, Layer::TopSilk, w);
        }
    }
    for y in [8.51, -8.51] {
        le::add_fp_shape(&mut f, Geom::Line { a: p(-8.0, y), b: p(8.0, y) }, Layer::TopSilk, w);
    }
    le::add_fp_shape(&mut f, Geom::Circle { center: p(-8.76, 8.25), radius: mm(0.38) }, Layer::TopSilk, w);
    le::add_fp_shape(&mut f, Geom::Rect { a: p(-8.0, -8.5), b: p(8.0, 8.5) }, Layer::TopFab, mm(0.1));
    le::add_fp_shape(&mut f, Geom::Rect { a: p(-9.45, -8.85), b: p(9.45, 8.85) }, Layer::TopCourtyard, mm(0.05));
    // The module in 3D: its board, the shield can, gold castellations.
    let mut b = Body::default();
    b.cuboid([-8.0, -8.5, 0.0], [8.0, 8.5, 0.8], colors::PLASTIC);
    b.cuboid([-7.0, -6.5, 0.8], [7.0, 7.8, 2.4], colors::SHIELD);
    for pd in &f.pads {
        let (x, y) = (crate::units::to_mm(pd.at.x), crate::units::to_mm(pd.at.y));
        let x0 = if x < 0.0 { -8.0 } else { 7.4 };
        b.cuboid([x0, y - 0.45, 0.0], [x0 + 0.6, y + 0.45, 0.82], colors::GOLD);
    }
    le::set_model(&mut f, "WIRELM-SMD_RA-01SH.step", [0.0; 3], [0.0; 3], [1.0; 3], 1.0);
    f.models[0].body = Some(b);
    f
}

/// TI's SWRA416 868/915 MHz helical PCB antenna: front and back copper strips joined through
/// plated holes, fed at pad 1, inside a keep-out.
pub fn swra416_footprint() -> Footprint {
    let mut f = le::new_footprint("Texas_SWRA416_868MHz_915MHz", "Texas_SWRA416_868MHz_915MHz", MountKind::Smd);
    f.description = "TI SWRA416 868 MHz / 915 MHz helical PCB antenna".into();
    // Copper only: nothing to pick and place.
    f.attrs.exclude_from_pos = true;
    f.models.clear();
    // The feed pad.
    let feed = le::add_pad(&mut f, p(-9.0, -5.9));
    {
        let pd = &mut f.pads[feed];
        pd.kind = PadKind::Smd;
        pd.shape = PadShape::Trapezoid { delta: Size::mm(0.0, 0.3) };
        pd.angle = 180.0;
        pd.size = Size::mm(0.4, 0.8);
        pd.drill = None;
        pd.layers = LayerSet::of(&[Layer::TopCopper]);
    }
    // The helix: strips on the front, diagonals on the back, plated holes at the turns.
    let w = mm(1.0);
    le::add_fp_shape(&mut f, Geom::Line { a: p(-9.0, -5.2), b: p(-9.0, 5.8) }, Layer::TopCopper, w);
    let mut holes = vec![(-9.0, 5.8)];
    for x in [-7.0, -5.0, -3.0, -1.0, 1.0, 3.0, 5.0, 7.0, 9.0] {
        le::add_fp_shape(&mut f, Geom::Line { a: p(x, 5.8), b: p(x, 0.8) }, Layer::TopCopper, w);
        holes.extend([(x, 5.8), (x, 0.8)]);
    }
    for x in [9.0, 7.0, 5.0, 3.0, 1.0, -1.0, -3.0, -5.0, -7.0] {
        le::add_fp_shape(&mut f, Geom::Line { a: p(x - 1.0, 1.8), b: p(x, 0.8) }, Layer::BottomCopper, w);
        le::add_fp_shape(&mut f, Geom::Line { a: p(x - 1.0, 4.8), b: p(x - 1.0, 1.8) }, Layer::BottomCopper, w);
        le::add_fp_shape(&mut f, Geom::Line { a: p(x - 2.0, 5.8), b: p(x - 1.0, 4.8) }, Layer::BottomCopper, w);
    }
    for (x, y) in holes {
        let mut h = crate::footprint::new_pad("", PadShape::Circle, p(x, y), Size::mm(1.0, 1.0), Some(mm(0.4)));
        h.kind = PadKind::ThroughHole;
        f.pads.push(h);
    }
    for layer in [Layer::TopCourtyard, Layer::BottomCourtyard] {
        le::add_fp_shape(&mut f, Geom::Rect { a: p(-9.9, -5.9), b: p(9.9, 6.7) }, layer, mm(0.05));
    }
    for (t, y) in [("KEEP-OUT ZONE", 2.8), ("No metal, traces or ", -0.2), ("any components on", -2.2), (" any PCB layer.", -4.2)] {
        f.texts.push(fp_text(t, p(1.0, y), Layer::Comments));
    }
    // Nothing poured, nothing placed over it.
    f.zones.push(Zone {
        id: Uuid::new_v4(),
        name: String::new(),
        net: String::new(),
        layers: LayerSet::of(&[Layer::TopCopper]),
        priority: 0,
        outline: vec![vec![p(-9.7, -5.7), p(9.7, -5.7), p(9.7, 6.5), p(-9.7, 6.5)]],
        fill: ZoneFill::default(),
        keepout: Some(Keepout { copper_pour: true, footprints: true, ..Default::default() }),
        locked: false,
        filled: vec![],
    });
    f
}

/// The built-in libraries and the project's own (the two module symbols, two footprints).
pub fn libraries() -> LibraryTable {
    let mut t = LibraryTable::builtin();
    let mut l = Library::new(LIB, Scope::Project);
    l.description = "Desk power monitor LoRa board".into();
    l.put_symbol(ra01sh_symbol());
    l.put_symbol(esp32_header_symbol(false));
    l.put_symbol(esp32_header_symbol(true));
    l.put_footprint(ra01sh_footprint());
    l.put_footprint(swra416_footprint());
    t.add(l);
    t
}

// ---------------------------------------------------------------------------------------------
// The schematic

/// A symbol to place: library id, x, y (mm), quarter turns, footprint, reference and value
/// positions.
type Placed = (&'static str, f64, f64, u32, &'static str, (f64, f64), (f64, f64));

/// Where each symbol goes: library id, position (mm), turns of 90° counter-clockwise,
/// footprint, and where its reference and value sit.
const PLACED: [Placed; 9] = [
    ("power:GNDREF", 50.8, 93.98, 0, "", (50.8, 87.63), (50.8, 88.9)),
    ("Device:C", 90.17, 157.48, 3, "Capacitor_SMD:C_0805_2012Metric", (90.17, 165.1), (90.17, 162.56)),
    ("power_monitor:esp32-pins-header-left", 63.5, 120.65, 0, "Connector_PinHeader_2.54mm:PinHeader_2x10_P2.54mm_Vertical", (70.485, 137.16), (70.485, 134.62)),
    ("power_monitor:esp32-pins-header-right", 156.21, 120.65, 0, "Connector_PinHeader_2.54mm:PinHeader_2x10_P2.54mm_Vertical", (163.195, 137.16), (163.195, 134.62)),
    ("Device:C", 82.55, 151.13, 0, "Capacitor_SMD:C_0805_2012Metric", (86.36, 152.4), (86.36, 149.86)),
    ("Device:C", 69.85, 151.13, 0, "Capacitor_SMD:C_0805_2012Metric", (73.66, 152.4), (73.66, 149.86)),
    ("Device:Antenna", 67.31, 167.64, 0, "power_monitor:Texas_SWRA416_868MHz_915MHz", (69.85, 168.28), (69.85, 165.74)),
    ("Device:L", 76.2, 157.48, 1, "Inductor_SMD:L_0805_2012Metric", (76.2, 162.56), (76.2, 160.02)),
    ("power_monitor:RA-01SH", 119.38, 120.65, 0, "power_monitor:WIRELM-SMD_RA-01SH", (119.38, 137.16), (119.38, 134.62)),
];

/// Every wire, end to end (mm).
const WIRES: [(f64, f64, f64, f64); 65] = [
    (87.63, 140.97, 144.78, 140.97),
    (186.69, 130.81, 175.26, 130.81),
    (88.9, 139.7, 88.9, 118.11),
    (186.69, 104.14, 186.69, 130.81),
    (72.39, 157.48, 69.85, 157.48),
    (95.25, 124.46, 107.95, 124.46),
    (50.8, 130.81, 58.42, 130.81),
    (96.52, 104.14, 96.52, 127.0),
    (50.8, 104.14, 96.52, 104.14),
    (69.85, 146.05, 82.55, 146.05),
    (143.51, 124.46, 143.51, 139.7),
    (82.55, 157.48, 82.55, 154.94),
    (144.78, 140.97, 144.78, 121.92),
    (175.26, 128.27, 177.8, 128.27),
    (100.33, 121.92, 100.33, 147.32),
    (146.05, 104.14, 146.05, 115.57),
    (91.44, 125.73, 91.44, 116.84),
    (139.7, 111.76, 130.81, 111.76),
    (90.17, 138.43, 90.17, 115.57),
    (96.52, 104.14, 139.7, 104.14),
    (100.33, 147.32, 177.8, 147.32),
    (86.36, 142.24, 86.36, 123.19),
    (146.05, 119.38, 130.81, 119.38),
    (50.8, 93.98, 50.8, 104.14),
    (96.52, 127.0, 107.95, 127.0),
    (82.55, 113.03, 95.25, 113.03),
    (177.8, 147.32, 177.8, 128.27),
    (146.05, 142.24, 146.05, 119.38),
    (50.8, 146.05, 69.85, 146.05),
    (130.81, 127.0, 142.24, 127.0),
    (96.52, 129.54, 107.95, 129.54),
    (142.24, 127.0, 142.24, 138.43),
    (140.97, 104.14, 146.05, 104.14),
    (142.24, 138.43, 90.17, 138.43),
    (96.52, 157.48, 96.52, 129.54),
    (82.55, 125.73, 91.44, 125.73),
    (69.85, 157.48, 69.85, 154.94),
    (82.55, 115.57, 90.17, 115.57),
    (139.7, 111.76, 139.7, 104.14),
    (140.97, 129.54, 140.97, 104.14),
    (144.78, 121.92, 130.81, 121.92),
    (50.8, 104.14, 50.8, 130.81),
    (140.97, 104.14, 139.7, 104.14),
    (95.25, 113.03, 95.25, 124.46),
    (86.36, 142.24, 146.05, 142.24),
    (143.51, 139.7, 88.9, 139.7),
    (91.44, 116.84, 107.95, 116.84),
    (80.01, 157.48, 82.55, 157.48),
    (130.81, 124.46, 143.51, 124.46),
    (82.55, 157.48, 86.36, 157.48),
    (50.8, 130.81, 50.8, 146.05),
    (87.63, 120.65, 87.63, 140.97),
    (139.7, 104.14, 140.97, 104.14),
    (82.55, 146.05, 82.55, 147.32),
    (140.97, 129.54, 130.81, 129.54),
    (69.85, 157.48, 67.31, 157.48),
    (146.05, 115.57, 151.13, 115.57),
    (82.55, 120.65, 87.63, 120.65),
    (82.55, 123.19, 86.36, 123.19),
    (69.85, 146.05, 69.85, 147.32),
    (93.98, 157.48, 96.52, 157.48),
    (146.05, 104.14, 186.69, 104.14),
    (82.55, 118.11, 88.9, 118.11),
    (67.31, 157.48, 67.31, 162.56),
    (107.95, 121.92, 100.33, 121.92),
];

/// The net labels.
const LABELS: [(&str, f64, f64); 7] = [("3v3", 104.14, 124.46), ("NSS", 133.35, 127.0), ("MOSI", 133.35, 124.46), ("RESET", 101.6, 121.92), ("SCK", 133.35, 119.38), ("DIO1", 102.87, 116.84), ("MISO", 133.35, 121.92)];

/// The schematic: symbols placed (annotated as they go: C1, C2, C3 …), turned, given their
/// footprints; the wires, junctions where three meet, labels and a note.
pub fn schematic(lib: &LibraryTable) -> Design {
    let mut d = Design::new();
    let s = &mut d.schematic;
    for (id, x, y, turns, footprint, r, v) in PLACED {
        let sym = lib.symbol(id).unwrap_or_else(|| panic!("{id}"));
        let at = p(x, y);
        let placed = se::place_symbol(s, 0, sym, at, Uuid::new_v4());
        for _ in 0..turns {
            se::rotate_items(s, 0, &[SchItem::Symbol(placed)], at);
        }
        if !footprint.is_empty() {
            se::set_field(s, placed, fields::FOOTPRINT, footprint);
        }
        let ps = s.sheets[0].symbols.iter_mut().find(|x| x.id == placed).unwrap();
        for (name, (fx, fy)) in [(fields::REFERENCE, r), (fields::VALUE, v)] {
            if let Some(f) = ps.field_mut(name) {
                f.text.at = p(fx, fy);
                f.text.angle = 0.0;
            }
        }
    }
    for (x0, y0, x1, y1) in WIRES {
        se::add_wire(s, 0, &[p(x0, y0), p(x1, y1)]);
    }
    se::fix_junctions(s, 0);
    for (text, x, y) in LABELS {
        se::add_label(s, 0, text, p(x, y), 0.0, LabelKind::Local);
    }
    se::add_note(s, 0, "LoRa", p(120.396, 107.188));
    d
}

// ---------------------------------------------------------------------------------------------
// The board

/// Where each footprint goes: reference, position (mm), angle (°).
const BOARD_PLACEMENT: [(&str, f64, f64, f64); 8] = [
    ("L1", 107.95, -89.408, 90.0),
    ("C1", 110.617, -88.519, 180.0),
    ("C2", 104.902, -88.519, 180.0),
    ("C3", 104.902, -90.678, 180.0),
    ("AE1", 101.648, -96.901, 180.0),
    ("J1", 88.906, -72.05, 0.0),
    ("J2", 114.306, -72.05, 0.0),
    ("U1", 102.87, -78.613, 180.0),
];

/// The outline: eight edges and the arc around the antenna's corner.
const OUTLINE: [(f64, f64, f64, f64); 8] = [
    (88.648737, -97.00672, 87.378737, -97.00672),
    (87.376, -67.147179, 89.779017, -64.805778),
    (115.824, -64.77, 118.374796, -67.321007),
    (89.966737, -103.81, 89.966737, -98.336775),
    (118.374796, -67.321007, 118.364, -103.81),
    (115.829833, -64.775833, 89.779017, -64.805778),
    (118.364, -103.81, 89.966737, -103.81),
    (87.378671, -97.00672, 87.376, -67.147179),
];

#[derive(Clone, Copy)]
enum Side {
    F,
    B,
}
use Side::{B, F};

/// The routed tracks: side, start, end (mm), width (mm) and KiCad's net (for the record; the
/// net each track gets comes from what it connects, as when routing).
const TRACKS: [(Side, f64, f64, f64, f64, f64, &str); 91] = [
    (F, 111.267, -85.613, 111.633, -85.979, 0.2, "Net-(U1-ANT)"),
    (F, 110.57, -85.613, 111.267, -85.613, 0.2, "Net-(U1-ANT)"),
    (F, 111.633, -85.979, 111.567, -86.045, 0.2, "Net-(U1-ANT)"),
    (F, 111.567, -86.045, 111.567, -88.519, 0.2, "Net-(U1-ANT)"),
    (F, 94.742, -88.519, 95.631, -88.519, 0.5, "GNDREF"),
    (F, 94.107, -87.884, 94.742, -88.519, 0.5, "GNDREF"),
    (F, 115.57, -83.947, 115.222, -83.599, 0.2, "GNDREF"),
    (F, 94.6, -85.613, 94.107, -86.106, 0.2, "GNDREF"),
    (F, 94.234, -68.58, 89.408, -68.58, 0.5, "GNDREF"),
    (F, 94.107, -86.106, 94.107, -87.884, 0.5, "GNDREF"),
    (F, 94.87, -71.613, 94.87, -71.502, 0.2, "GNDREF"),
    (F, 114.306, -87.29, 114.306, -87.243, 0.2, "GNDREF"),
    (F, 94.234, -70.866, 94.234, -68.58, 0.5, "GNDREF"),
    (F, 88.9, -69.088, 88.906, -69.094, 0.5, "GNDREF"),
    (F, 102.809, -89.535, 103.952, -90.678, 0.5, "GNDREF"),
    (F, 101.727, -89.535, 102.809, -89.535, 0.5, "GNDREF"),
    (F, 94.87, -71.502, 94.234, -70.866, 0.5, "GNDREF"),
    (F, 105.998, -83.613, 103.952, -85.659, 0.5, "GNDREF"),
    (F, 110.884, -83.599, 110.87, -83.613, 0.2, "GNDREF"),
    (F, 94.87, -85.613, 94.6, -85.613, 0.2, "GNDREF"),
    (F, 101.727, -89.535, 102.936, -89.535, 0.5, "GNDREF"),
    (F, 116.332, -68.58, 94.234, -68.58, 0.5, "GNDREF"),
    (F, 88.906, -69.094, 88.906, -72.05, 0.5, "GNDREF"),
    (F, 89.408, -68.58, 88.9, -69.088, 0.5, "GNDREF"),
    (F, 116.846, -69.094, 116.332, -68.58, 0.5, "GNDREF"),
    (F, 114.306, -87.243, 115.57, -85.979, 0.2, "GNDREF"),
    (F, 116.846, -72.05, 116.846, -69.094, 0.5, "GNDREF"),
    (F, 110.87, -83.613, 105.998, -83.613, 0.5, "GNDREF"),
    (F, 102.936, -89.535, 103.952, -88.519, 0.5, "GNDREF"),
    (F, 103.952, -90.678, 103.952, -88.519, 0.5, "GNDREF"),
    (F, 115.222, -83.599, 110.884, -83.599, 0.2, "GNDREF"),
    (F, 115.57, -85.979, 115.57, -83.947, 0.2, "GNDREF"),
    (F, 103.952, -85.659, 103.952, -88.519, 0.5, "GNDREF"),
    (B, 88.906, -72.05, 88.906, -68.828, 0.5, "GNDREF"),
    (B, 103.124, -71.247, 103.124, -78.6765, 0.5, "GNDREF"),
    (B, 97.536, -66.802, 100.838, -70.104, 0.5, "GNDREF"),
    (B, 114.173, -66.929, 107.442, -66.929, 0.5, "GNDREF"),
    (B, 116.846, -69.602, 114.173, -66.929, 0.5, "GNDREF"),
    (B, 98.933, -85.217, 95.631, -88.519, 0.5, "GNDREF"),
    (B, 103.124, -85.217, 101.727, -86.614, 0.5, "GNDREF"),
    (B, 101.727, -86.614, 101.62955, -86.71145, 0.2, "GNDREF"),
    (B, 101.727, -86.614, 100.711, -87.63, 0.5, "GNDREF"),
    (B, 101.219, -89.535, 101.727, -89.535, 0.5, "GNDREF"),
    (B, 99.949, -85.217, 98.933, -85.217, 0.2, "GNDREF"),
    (B, 103.124, -78.6765, 103.124, -85.217, 0.2, "GNDREF"),
    (B, 94.234, -66.802, 97.536, -66.802, 0.5, "GNDREF"),
    (B, 116.846, -72.05, 116.846, -69.602, 0.5, "GNDREF"),
    (B, 94.234, -66.802, 94.234, -68.58, 0.5, "GNDREF"),
    (B, 90.932, -66.802, 94.234, -66.802, 0.5, "GNDREF"),
    (B, 103.124, -78.6765, 100.838, -76.3905, 0.2, "GNDREF"),
    (B, 100.711, -87.63, 100.711, -89.027, 0.5, "GNDREF"),
    (B, 102.362, -79.629, 99.949, -82.042, 0.2, "GNDREF"),
    (B, 107.442, -66.929, 103.124, -71.247, 0.5, "GNDREF"),
    (B, 103.124, -78.6765, 108.0135, -83.566, 0.2, "GNDREF"),
    (B, 88.906, -68.828, 90.932, -66.802, 0.5, "GNDREF"),
    (B, 99.949, -82.042, 99.949, -85.217, 0.2, "GNDREF"),
    (B, 100.838, -70.104, 100.838, -76.3905, 0.5, "GNDREF"),
    (B, 106.645, -79.629, 102.362, -79.629, 0.2, "GNDREF"),
    (B, 100.711, -89.027, 101.219, -89.535, 0.5, "GNDREF"),
    (F, 94.87, -81.613, 94.583, -81.613, 0.2, "/MOSI"),
    (F, 94.583, -81.613, 91.446, -84.75, 0.2, "/MOSI"),
    (F, 94.87, -77.613, 93.503, -77.613, 0.2, "/SCK"),
    (F, 93.503, -77.613, 91.446, -79.67, 0.2, "/SCK"),
    (F, 92.964, -84.836, 94.187, -83.613, 0.2, "/NSS"),
    (F, 94.187, -83.613, 94.87, -83.613, 0.2, "/NSS"),
    (F, 92.964, -85.772, 92.964, -84.836, 0.2, "/NSS"),
    (F, 91.446, -87.29, 92.964, -85.772, 0.2, "/NSS"),
    (F, 107.95, -88.3455, 107.97325, -88.36875, 0.2, "Net-(C1-Pad2)"),
    (F, 108.1235, -88.519, 107.95, -88.3455, 0.2, "Net-(C1-Pad2)"),
    (F, 109.667, -88.519, 108.1235, -88.519, 0.2, "Net-(C1-Pad2)"),
    (F, 105.852, -88.519, 107.7765, -88.519, 0.2, "Net-(C1-Pad2)"),
    (F, 107.7765, -88.519, 107.95, -88.3455, 0.2, "Net-(C1-Pad2)"),
    (F, 107.95, -90.4705, 110.2185, -90.4705, 0.2, "Net-(AE1-A)"),
    (F, 106.0595, -90.4705, 105.852, -90.678, 0.2, "Net-(AE1-A)"),
    (F, 107.95, -90.4705, 106.0595, -90.4705, 0.2, "Net-(AE1-A)"),
    (F, 110.2185, -90.4705, 110.648, -90.9, 0.2, "Net-(AE1-A)"),
    (F, 110.87, -75.613, 108.791, -75.613, 0.2, "/DIO1"),
    (F, 108.791, -75.613, 107.823, -76.581, 0.2, "/DIO1"),
    (F, 107.823, -76.581, 91.995, -76.581, 0.2, "/DIO1"),
    (F, 91.995, -76.581, 91.446, -77.13, 0.2, "/DIO1"),
    (F, 94.87, -79.613, 94.043, -79.613, 0.2, "/MISO"),
    (F, 94.043, -79.613, 91.446, -82.21, 0.2, "/MISO"),
    (F, 96.86, -89.83, 105.077, -81.613, 0.2, "/3v3"),
    (F, 91.446, -89.83, 96.86, -89.83, 0.2, "/3v3"),
    (F, 105.077, -81.613, 110.87, -81.613, 0.2, "/3v3"),
    (F, 116.846, -74.59, 115.57, -75.866, 0.2, "/RESET"),
    (F, 113.157, -78.359, 111.903, -79.613, 0.2, "/RESET"),
    (F, 115.57, -77.49376, 114.70476, -78.359, 0.2, "/RESET"),
    (F, 111.903, -79.613, 110.87, -79.613, 0.2, "/RESET"),
    (F, 114.70476, -78.359, 113.157, -78.359, 0.2, "/RESET"),
    (F, 115.57, -75.866, 115.57, -77.49376, 0.2, "/RESET"),
];

/// The ground vias.
const VIAS: [(f64, f64); 3] = [(101.727, -89.535), (94.234, -68.58), (95.631, -88.519)];

/// The schematic and the board: Update PCB, the outline, every footprint placed, the tracks
/// routed and the vias set, and the back-copper ground rectangle under the module.
pub fn design() -> (Design, LibraryTable) {
    let lib = libraries();
    let mut d = schematic(&lib);
    d.board.thickness = mm(1.6);
    d.board.shapes.clear();
    for (x0, y0, x1, y1) in OUTLINE {
        d.board.shapes.push(BoardShape { id: Uuid::new_v4(), shape: Shape { geom: Geom::Line { a: p(x0, y0), b: p(x1, y1) }, stroke: Stroke::width(mm(0.05)), fill: Fill::None }, layer: Layer::Outline, locked: false, net: String::new() });
    }
    d.board.shapes.push(BoardShape {
        id: Uuid::new_v4(),
        shape: Shape { geom: Geom::Arc { start: p(88.648737, -97.00672), mid: p(89.584975, -97.397022), end: p(89.966737, -98.336775) }, stroke: Stroke::width(mm(0.05)), fill: Fill::None },
        layer: Layer::Outline,
        locked: false,
        net: String::new(),
    });
    forward::update_pcb(&mut d, &lib, &forward::Options::default());
    let b = &mut d.board;
    for (r, x, y, angle) in BOARD_PLACEMENT {
        let i = be::footprint_index(b, r).unwrap_or_else(|| panic!("{r}"));
        be::move_footprint(b, i, p(x, y));
        be::rotate_footprint(b, i, angle);
    }
    let gnd = b.footprints.iter().flat_map(|f| f.footprint.pads.iter()).filter_map(|pd| pd.net.clone()).find(|n| n == "GNDREF").unwrap_or_default();
    for (x, y) in VIAS {
        be::add_via(b, p(x, y), &gnd);
    }
    for (side, x0, y0, x1, y1, w, _) in TRACKS {
        let layer = match side {
            F => Layer::TopCopper,
            B => Layer::BottomCopper,
        };
        be::route(b, &[p(x0, y0), p(x1, y1)], layer, Some(mm(w)));
    }
    // A segment routed out of a bend has no net yet: it takes its neighbours'.
    loop {
        let mut changed = false;
        for i in 0..b.tracks.len() {
            if !b.tracks[i].net.is_empty() {
                continue;
            }
            let (a, z, layer) = (b.tracks[i].a, b.tracks[i].b, b.tracks[i].layer);
            let net = [a, z].into_iter().map(|q| be::net_at(b, q, layer)).find(|n| !n.is_empty());
            if let Some(n) = net {
                b.tracks[i].net = n;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    b.shapes.push(BoardShape {
        id: Uuid::new_v4(),
        shape: Shape { geom: Geom::Rect { a: p(98.044, -71.501), b: p(108.204, -85.852) }, stroke: Stroke::width(mm(0.2)), fill: Fill::Outline },
        layer: Layer::BottomCopper,
        locked: false,
        net: gnd,
    });
    (d, lib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redrawn_board_is_complete() {
        let (d, lib) = design();
        assert!(le::check_footprint(lib.footprint(&format!("{LIB}:WIRELM-SMD_RA-01SH")).unwrap()).is_empty());
        assert_eq!(d.schematic.sheets[0].symbols.len(), 9);
        assert_eq!(d.schematic.sheets[0].junctions.len(), 9);
        let refs: Vec<&str> = d.schematic.sheets[0].symbols.iter().map(|s| s.reference()).collect();
        for r in ["C1", "C2", "C3", "L1", "J1", "J2", "U1", "AE1"] {
            assert!(refs.contains(&r), "{r} in {refs:?}");
        }
        // No wire end in the air.
        let erc = crate::erc::check(&d.schematic);
        assert!(erc.iter().all(|v| v.rule != crate::erc::Rule::DanglingWire), "{erc:?}");
        let b = &d.board;
        assert_eq!((b.footprints.len(), b.tracks.len(), b.vias.len()), (8, 91, 3));
        assert!(b.tracks.iter().all(|t| !t.net.is_empty()), "{:?}", b.tracks.iter().filter(|t| t.net.is_empty()).map(|t| (t.a.to_mm(), t.b.to_mm())).collect::<Vec<_>>());
        assert_eq!(crate::outline::loops(b).len(), 1);
        // Everything that should be is connected.
        let r = crate::drc::check(b);
        assert!(r.unconnected.is_empty(), "{:?}", r.unconnected);
    }
}

#[cfg(test)]
mod outputs {
    #[test]
    fn jlcpcb_bom_and_placement() {
        let (d, _) = super::design();
        let bom = crate::bom::jlcpcb_csv(&d.schematic);
        assert!(bom.starts_with("Comment,Designator,Footprint,LCSC Part #\n"));
        // The three 0805 capacitors share a row; the module keeps its LCSC part number.
        assert!(bom.contains("\"C\",\"C1,C2,C3\",\"C_0805_2012Metric\",\"\""), "{bom}");
        let cpl = crate::fab::jlcpcb_cpl(&d.board);
        assert!(cpl.contains("\"U1\",\"RA-01SH\",\"WIRELM-SMD_RA-01SH\",102.870000,-78.613000,180.000000,top"), "{cpl}");
        // Through-hole headers are left to the hand.
        assert!(!cpl.contains("\"J1\""));
    }
}
