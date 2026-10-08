//! The "Getting Started" course's design (docs/PLAN.md, GS1–GS26) at each step: a coin
//! cell, a 1k resistor and a red LED; then the board; then a custom switch. The course tests
//! (`tests/getting_started.rs`) check each step; the app can open any stage as an example.

use crate::Design;
use crate::board::Board;
use crate::board_edit as be;
use crate::forward;
use crate::graphics::{Fill, Geom};
use crate::layer::Layer;
use crate::lib_edit::{self as le, Orientation, PinProps};
use crate::library::{Library, LibraryTable, Scope};
use crate::sch_edit::{self as se, SchItem};
use crate::schematic::{LabelKind, Schematic, TitleBlock};
use crate::footprint::MountKind;
use crate::expr::{Unit, eval};
use crate::units::{MIL, Pt, mm};
use crate::{outline, zone};
use uuid::Uuid;

pub fn p(x: f64, y: f64) -> Pt {
    Pt::mm(x, y)
}

/// GS1: a new board's design: one empty sheet.
pub fn new_design() -> Design {
    Design::new()
}

pub fn sym(sch: &Schematic, reference: &str) -> Uuid {
    sch.sheets[0].symbols.iter().find(|s| s.reference() == reference).unwrap_or_else(|| panic!("{reference}")).id
}

pub fn pin(sch: &Schematic, reference: &str, number: &str) -> Pt {
    let s = sch.sheets[0].symbols.iter().find(|s| s.reference() == reference).unwrap();
    sch.placed_pins(s).find(|(p, _)| p.number == number).unwrap().1
}

/// GS3–GS4: page set up; LED, resistor and battery placed (in that order).
pub fn gs04(lib: &LibraryTable) -> Design {
    let mut d = new_design();
    let tb = TitleBlock { title: "Getting Started".into(), date: "2026-10-06".into(), revision: "0".into(), ..Default::default() };
    se::set_page(&mut d.schematic, 0, se::paper("B").unwrap(), tb);
    let led = lib.search_symbols("LED", false)[0].clone();
    let r = lib.search_symbols("R_US", false)[0].clone();
    let bt = lib.search_symbols("Battery_Cell", false)[0].clone();
    se::place_symbol(&mut d.schematic, 0, &led, p(152.4, 101.6), Uuid::new_v4());
    se::place_symbol(&mut d.schematic, 0, &r, p(203.2, 101.6), Uuid::new_v4());
    se::place_symbol(&mut d.schematic, 0, &bt, p(101.6, 101.6), Uuid::new_v4());
    d
}

/// GS5: R1 above D1 on the right, the LED turned so its anode points up at R1.
pub fn gs05(lib: &LibraryTable) -> Design {
    let mut d = gs04(lib);
    let s = &mut d.schematic;
    let (r1, d1) = (sym(s, "R1"), sym(s, "D1"));
    // R1 up to (203.2, 114.3); D1 across under it to (203.2, 93.98), then turned.
    se::move_items(s, 0, &[SchItem::Symbol(r1)], p(0.0, 12.7), false);
    se::move_items(s, 0, &[SchItem::Symbol(d1)], p(50.8, -7.62), false);
    let c = se::selection_center(s, 0, &[SchItem::Symbol(d1)]);
    se::rotate_items(s, 0, &[SchItem::Symbol(d1)], c);
    d
}

/// GS6–GS7: the loop wired; VCC and GND ports; the `led` label.
pub fn gs07(lib: &LibraryTable) -> Design {
    let mut d = gs05(lib);
    let s = &mut d.schematic;
    let (bt_p, bt_n) = (pin(s, "BT1", "1"), pin(s, "BT1", "2"));
    let (r_1, r_2) = (pin(s, "R1", "1"), pin(s, "R1", "2"));
    let (d_a, d_k) = (pin(s, "D1", "2"), pin(s, "D1", "1"));
    se::add_wire(s, 0, &[bt_p, p(bt_p.to_mm()[0], 127.0), p(r_1.to_mm()[0], 127.0), r_1]);
    se::add_wire(s, 0, &[r_2, d_a]);
    se::add_wire(s, 0, &[d_k, p(d_k.to_mm()[0], 76.2), p(bt_n.to_mm()[0], 76.2), bt_n]);
    se::place_symbol(s, 0, lib.symbol("power:VCC").unwrap(), p(152.4, 132.08), Uuid::new_v4());
    se::add_wire(s, 0, &[p(152.4, 132.08), p(152.4, 127.0)]);
    se::place_symbol(s, 0, lib.symbol("power:GND").unwrap(), p(152.4, 71.12), Uuid::new_v4());
    se::add_wire(s, 0, &[p(152.4, 71.12), p(152.4, 76.2)]);
    let mid = Pt::new(r_2.x, (r_2.y + d_a.y) / 2);
    se::add_label(s, 0, "led", mid, 90.0, LabelKind::Local);
    d
}

/// GS9–GS10: values and footprints.
pub fn gs10(lib: &LibraryTable) -> Design {
    let mut d = gs07(lib);
    let s = &mut d.schematic;
    for (r, v) in [("D1", "red"), ("BT1", "3V"), ("R1", "1k")] {
        let id = sym(s, r);
        se::set_field(s, id, "Value", v);
    }
    for (r, f) in [
        ("BT1", "Battery:BatteryHolder_Keystone_1058_1x2032"),
        ("D1", "LED_THT:LED_D5.0mm"),
        ("R1", "Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal"),
    ] {
        crate::assign::assign(s, r, f);
    }
    d
}

/// GS11: PWR_FLAGs on the VCC and GND nets.
pub fn gs11(lib: &LibraryTable) -> Design {
    let mut d = gs10(lib);
    let s = &mut d.schematic;
    let bt = pin(s, "BT1", "1").to_mm()[0];
    se::place_symbol(s, 0, lib.symbol("power:PWR_FLAG").unwrap(), p(bt, 116.84), Uuid::new_v4());
    se::place_symbol(s, 0, lib.symbol("power:PWR_FLAG").unwrap(), p(bt, 86.36), Uuid::new_v4());
    // The flags' pins sit on the battery's wires: they join them.
    se::fix_junctions(s, 0);
    d
}


/// GS13: board setup: two layers, 1.6 mm; the Default class's tracks 0.4 mm.
pub fn gs13(lib: &LibraryTable) -> Design {
    let mut d = gs11(lib);
    d.board.title_block = TitleBlock { title: "Getting Started".into(), revision: "0".into(), ..Default::default() };
    d.board.paper = se::paper("A").unwrap();
    d.board.rules.class_mut("Default").unwrap().track_width = mm(0.4);
    d
}

/// GS14: Update PCB from schematic.
pub fn gs14(lib: &LibraryTable) -> (Design, forward::Report) {
    let mut d = gs13(lib);
    let rep = forward::update_pcb(&mut d, lib, &forward::Options { place_at: p(0.0, 60.0), ..Default::default() });
    (d, rep)
}

/// GS15–GS16: a 45 × 50 mm outline; BT1 flipped to the back, D1 and R1 arranged below it.
pub fn gs16(lib: &LibraryTable) -> Design {
    let (mut d, _) = gs14(lib);
    let b = &mut d.board;
    outline::add_rect(b, p(0.0, 0.0), p(45.0, 50.0));
    let bt = be::footprint_index(b, "BT1").unwrap();
    be::move_footprint(b, bt, p(22.5, 32.0));
    be::flip_footprint(b, bt);
    let d1 = be::footprint_index(b, "D1").unwrap();
    be::move_footprint(b, d1, p(8.0, 14.0));
    be::rotate_footprint(b, d1, 270.0);
    let r1 = be::footprint_index(b, "R1").unwrap();
    be::move_footprint(b, r1, p(37.0, 11.46));
    be::rotate_footprint(b, r1, 180.0);
    d
}

pub fn pad(b: &Board, r: &str, n: &str) -> Pt {
    let f = be::footprint_index(b, r).unwrap();
    let i = b.footprints[f].footprint.pads.iter().position(|x| x.number == n).unwrap();
    be::pad_center(b, f, i)
}

/// GS17: led on the front; GND on the back (starting at BT1's bottom pad switches layers);
/// VCC down the back, through a via, to R1 on the front.
pub fn gs17(lib: &LibraryTable) -> Design {
    let mut d = gs16(lib);
    let b = &mut d.board;
    let (d1a, r1b) = (pad(b, "D1", "2"), pad(b, "R1", "2"));
    let l = be::start_layer(b, d1a, Layer::TopCopper);
    be::route(b, &be::posture(d1a, r1b, false), l, None);
    let (bt2, d1k) = (pad(b, "BT1", "2"), pad(b, "D1", "1"));
    let l = be::start_layer(b, bt2, Layer::TopCopper);
    be::route(b, &be::posture(bt2, d1k, false), l, None);
    let (bt1, r1a) = (pad(b, "BT1", "1"), pad(b, "R1", "1"));
    let via = Pt::new(bt1.x, mm(20.0));
    let l = be::start_layer(b, bt1, Layer::TopCopper);
    be::route(b, &[bt1, via], l, None);
    be::add_via(b, via, "VCC");
    be::route(b, &be::posture(via, r1a, true), Layer::TopCopper, None);
    d
}

/// GS18: a GND zone over the whole back, filled.
pub fn gs18(lib: &LibraryTable) -> Design {
    let mut d = gs17(lib);
    let b = &mut d.board;
    zone::add_zone(b, "GND", Layer::BottomCopper, vec![p(0.0, 0.0), p(45.0, 0.0), p(45.0, 50.0), p(0.0, 50.0)]);
    zone::fill_all(b);
    d
}

pub const SW_FP: &str = "getting-started:Switch_Toggle_SPST_NKK_M2011S3A1x03";

/// GS23: the switch symbol, pins A/2 and B/3 200 mil either side, a lever drawn between.
pub fn switch_symbol() -> crate::symbol::Symbol {
    let mut s = le::new_symbol("M2011S3A1W03", "SW", false);
    le::add_pin(&mut s, &PinProps::new("A", "2", Pt::new(-200 * MIL, 0), Orientation::Right));
    let b = le::repeat_pin(&mut s).unwrap();
    le::set_pin(&mut s, b, &PinProps::new("B", "3", Pt::new(200 * MIL, 0), Orientation::Left));
    let lever = |x: f64| Geom::Circle { center: Pt::new((x * MIL as f64) as i64, 0), radius: 15 * MIL };
    le::add_symbol_shape(&mut s, lever(-100.0), mm(0.254), Fill::None);
    le::add_symbol_shape(&mut s, lever(100.0), mm(0.254), Fill::None);
    le::add_symbol_shape(&mut s, Geom::Line { a: Pt::new(-85 * MIL, 5 * MIL), b: Pt::new(80 * MIL, 70 * MIL) }, mm(0.254), Fill::None);
    le::set_symbol_field(&mut s, "Value", "M2011S3A1W03");
    s.keywords = "spst switch toggle".into();
    le::set_symbol_field(&mut s, "Footprint", SW_FP);
    s
}

/// GS24: the switch footprint: pads 2 and 3 4.7 mm apart, fab, silkscreen and courtyard.
pub fn switch_footprint() -> crate::footprint::Footprint {
    let mut f = le::new_footprint("Switch_Toggle_SPST_NKK_M2011S3A1x03", "Switch_Toggle_SPST_NKK_M2011S3A1x03", MountKind::ThroughHole);
    // (KiCad's Y runs down; ours up: its (0, 4.7) is our (0, -4.7).)
    let p2 = le::add_pad(&mut f, p(0.0, -4.7));
    f.pads[p2].number = "2".into();
    let hole = eval("1.42 + 0.2", Unit::Mm).unwrap();
    let ring = eval("1.62 + 2*0.15", Unit::Mm).unwrap();
    f.pads[p2].drill = Some(crate::footprint::Drill { size: crate::units::Size::new(hole, hole), offset: Pt::ZERO });
    f.pads[p2].size = crate::units::Size::new(ring, ring);
    le::add_pad(&mut f, p(0.0, -9.4));
    // Bigger annular rings: pad 2 to 1.62+2*0.3, then pushed to the other pad.
    let bigger = eval("1.62+2*0.3", Unit::Mm).unwrap();
    f.pads[p2].size = crate::units::Size::new(bigger, bigger);
    le::push_pad_properties(&mut f, p2);
    let rect = |a: (f64, f64), b: (f64, f64)| Geom::Rect { a: p(a.0, a.1), b: p(b.0, b.1) };
    le::add_fp_shape(&mut f, rect((-3.95, 1.8), (3.95, -11.2)), Layer::TopFab, mm(0.1));
    le::add_fp_shape(&mut f, rect((-4.06, 1.91), (4.06, -11.31)), Layer::TopSilk, mm(0.12));
    le::add_fp_shape(&mut f, rect((-4.2, 2.05), (4.2, -11.45)), Layer::TopCourtyard, mm(0.05));
    le::set_model(&mut f, "getting-started.3dshapes/Switch_Toggle_SPST_NKK_M2011S3A1x03.step", [0.0, 0.0, 0.0], [0.0, 0.0, 90.0], [1.0, 1.0, 1.0], 1.0);
    f
}

/// GS22: the project's own library, holding the switch.
pub fn project_library() -> LibraryTable {
    let mut t = LibraryTable::builtin();
    let mut l = Library::new("getting-started", Scope::Project);
    l.put_symbol(switch_symbol());
    l.put_footprint(switch_footprint());
    t.add(l);
    t
}

/// GS25: SW1 between the battery and VCC; then the board: the old battery-to-resistor
/// connection removed, SW1 placed on a wider board, routed, refilled.
pub fn gs25() -> (Design, LibraryTable, forward::Report) {
    let lib = project_library();
    let mut d = gs18(&lib);
    let s = &mut d.schematic;
    let bt_p = pin(s, "BT1", "1");
    let x = bt_p.to_mm()[0];
    // The wire from the battery up to the corner, and the flag on it, make way.
    let w = s.sheets[0].wires.iter().find(|w| w.a == bt_p || w.b == bt_p).unwrap().id;
    let flag = s.sheets[0].symbols.iter().find(|x| x.value() == "PWR_FLAG" && x.placement.at.y > bt_p.y).unwrap().id;
    se::delete_items(s, 0, &[SchItem::Wire(w), SchItem::Symbol(flag)]);
    let sw = se::place_symbol(s, 0, lib.symbol("getting-started:M2011S3A1W03").unwrap(), p(x, 115.57), Uuid::new_v4());
    let c = se::selection_center(s, 0, &[SchItem::Symbol(sw)]);
    se::rotate_items(s, 0, &[SchItem::Symbol(sw)], c);
    let (s2, s3) = (pin(s, "SW1", "2"), pin(s, "SW1", "3"));
    se::add_wire(s, 0, &[bt_p, s2]);
    se::add_wire(s, 0, &[s3, p(x, 127.0)]);
    se::place_symbol(s, 0, lib.symbol("power:PWR_FLAG").unwrap(), p(x, 123.19), Uuid::new_v4());
    se::fix_junctions(s, 0);

    let rep = forward::update_pcb(&mut d, &lib, &forward::Options { place_at: p(60.0, 20.0), ..Default::default() });
    let b = &mut d.board;
    // The battery-to-resistor connection (U, U again: through the via) goes.
    let t = b.tracks.iter().find(|t| t.net == "VCC" && t.layer == Layer::BottomCopper).unwrap().id;
    let chain = be::select_connected(b, t, true);
    be::delete_tracks(b, &chain);
    // A wider board (and zone) for SW1.
    let outline = b.shapes.iter_mut().find(|s| s.layer == Layer::Outline).unwrap();
    outline.shape.geom = Geom::Rect { a: p(0.0, 0.0), b: p(55.0, 50.0) };
    b.zones[0].outline = vec![vec![p(0.0, 0.0), p(55.0, 0.0), p(55.0, 50.0), p(0.0, 50.0)]];
    let swi = be::footprint_index(b, "SW1").unwrap();
    be::move_footprint(b, swi, p(49.0, 26.0));
    let (bt1, sw2, sw3, r1a) = (pad(b, "BT1", "1"), pad(b, "SW1", "2"), pad(b, "SW1", "3"), pad(b, "R1", "1"));
    let l = be::start_layer(b, bt1, Layer::TopCopper);
    be::route(b, &be::posture(bt1, sw2, false), l, None);
    be::route(b, &be::posture(sw3, r1a, false), Layer::TopCopper, None);
    zone::fill_all(b);
    (d, lib, rep)
}

