//! Built-in symbols: `Device`, `Switch`, `Connector`, `power`, `Regulator_Linear`,
//! `Amplifier_Operational`, `LED`. Drawn for cadrs on the 1.27 mm grid (pins on it), KiCad's
//! naming and pin numbering so designs and footprints line up.

use super::*;

fn arc(start: (f64, f64), mid: (f64, f64), end: (f64, f64), w: f64) -> SymbolGraphic {
    SymbolGraphic { item: SymbolItem::Shape(Shape { geom: Geom::Arc { start: p(start.0, start.1), mid: p(mid.0, mid.1), end: p(end.0, end.1) }, stroke: Stroke::width(mm(w)), fill: Fill::None }), unit: 0, style: 0 }
}

fn circle(c: (f64, f64), r: f64, w: f64, fill: Fill) -> SymbolGraphic {
    SymbolGraphic { item: SymbolItem::Shape(Shape { geom: Geom::Circle { center: p(c.0, c.1), radius: mm(r) }, stroke: Stroke::width(mm(w)), fill }), unit: 0, style: 0 }
}

/// Reference and value centred above and below (`up`, `down` mm from the origin).
fn fields_above_below(s: &mut Symbol, up: f64, down: f64) {
    s.fields[0].text.at = p(0.0, up);
    s.fields[1].text.at = p(0.0, -down);
    s.fields.iter_mut().take(2).for_each(|f| f.text.style.h_align = HAlign::Center);
}

/// Reference and value left-aligned to the right of the body, `x` mm from the origin.
fn fields_right(s: &mut Symbol, x: f64) {
    s.fields[0].text.at = p(x, 1.27);
    s.fields[1].text.at = p(x, -1.27);
}

fn with_footprint(mut s: Symbol, footprint: &str) -> Symbol {
    if let Some(f) = s.fields.iter_mut().find(|f| f.name == fields::FOOTPRINT) {
        f.text.text = footprint.into();
    }
    s
}

fn filters(s: &mut Symbol, f: &[&str]) {
    s.footprint_filters = f.iter().map(|x| x.to_string()).collect();
}

pub(super) fn device() -> Library {
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
    more_device(&mut lib);
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

pub(super) fn power() -> Library {
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
    more_power(&mut lib);
    lib
}


// ---------------------------------------------------------------------------------------------
// More of `Device`

/// A diode body (cathode pin 1 left, anode pin 2 right) with the cathode bar drawn by `bar`.
fn diode_like(name: &str, desc: &str, kw: &str, f: &[&str], bar: &[(f64, f64)]) -> Symbol {
    let mut d = new_symbol(name, "D", desc);
    d.keywords = kw.into();
    filters(&mut d, f);
    d.show_pin_names = false;
    d.show_pin_numbers = false;
    d.pins = vec![pin("1", "K", PinType::Passive, (-3.81, 0.0), 0.0, 2.54), pin("2", "A", PinType::Passive, (3.81, 0.0), 180.0, 2.54)];
    d.graphics.push(line(bar, 0.254, Fill::None));
    d.graphics.push(line(&[(1.27, -1.27), (1.27, 1.27), (-1.27, 0.0), (1.27, -1.27)], 0.254, Fill::None));
    d.graphics.push(line(&[(-1.27, 0.0), (1.27, 0.0)], 0.254, Fill::None));
    fields_above_below(&mut d, 2.54, 3.81);
    d
}

/// A bipolar transistor: base 1 left, emitter 2 below, collector 3 above (BEC order).
fn bjt(name: &str, npn: bool) -> Symbol {
    let kind = if npn { "NPN" } else { "PNP" };
    let mut q = new_symbol(name, "Q", &format!("{kind} transistor, base/emitter/collector"));
    q.keywords = format!("transistor {kind} BJT").to_lowercase();
    filters(&mut q, &["SOT?23*", "TO?92*", "SOT?223*"]);
    q.show_pin_names = false;
    q.pins = vec![
        pin("1", "B", PinType::Input, (-5.08, 0.0), 0.0, 5.715),
        pin("2", "E", PinType::Passive, (2.54, -5.08), 90.0, 2.54),
        pin("3", "C", PinType::Passive, (2.54, 5.08), 270.0, 2.54),
    ];
    q.graphics.push(circle((1.27, 0.0), 2.8194, 0.254, Fill::None));
    q.graphics.push(line(&[(0.635, 1.905), (0.635, -1.905)], 0.508, Fill::None));
    q.graphics.push(line(&[(0.635, 0.635), (2.54, 2.54)], 0.254, Fill::None));
    q.graphics.push(line(&[(0.635, -0.635), (2.54, -2.54)], 0.254, Fill::None));
    // The emitter arrow: out of the base for NPN, into it for PNP.
    let arrow: &[(f64, f64)] = if npn { &[(2.286, -2.286), (1.897, -1.403), (1.403, -1.897), (2.286, -2.286)] } else { &[(1.143, -1.143), (2.027, -1.533), (1.533, -2.027), (1.143, -1.143)] };
    q.graphics.push(line(arrow, 0.254, Fill::Outline));
    fields_right(&mut q, 5.08);
    q
}

/// A MOSFET: gate 1 left, source 2 below, drain 3 above (GSD order).
fn mosfet(name: &str, n: bool) -> Symbol {
    let kind = if n { "N" } else { "P" };
    let mut q = new_symbol(name, "Q", &format!("{kind}-channel MOSFET, gate/source/drain"));
    q.keywords = format!("transistor {kind}MOS {kind}-MOS {kind}-MOSFET").to_lowercase();
    filters(&mut q, &["SOT?23*", "TO?252*", "TO?220*", "SOT?223*"]);
    q.show_pin_names = false;
    q.pins = vec![
        pin("1", "G", PinType::Input, (-5.08, 0.0), 0.0, 5.334),
        pin("2", "S", PinType::Passive, (2.54, -5.08), 90.0, 2.54),
        pin("3", "D", PinType::Passive, (2.54, 5.08), 270.0, 2.54),
    ];
    q.graphics.push(circle((1.651, 0.0), 2.794, 0.254, Fill::None));
    q.graphics.push(line(&[(0.254, 1.905), (0.254, -1.905)], 0.254, Fill::None));
    for (a, b) in [(1.27, 2.286), (-0.508, 0.508), (-2.286, -1.27)] {
        q.graphics.push(line(&[(0.762, a), (0.762, b)], 0.254, Fill::None));
    }
    q.graphics.push(line(&[(0.762, 1.778), (2.54, 1.778), (2.54, 2.54)], 0.254, Fill::None));
    q.graphics.push(line(&[(0.762, -1.778), (2.54, -1.778), (2.54, -2.54)], 0.254, Fill::None));
    q.graphics.push(line(&[(0.762, 0.0), (2.54, 0.0), (2.54, -1.778)], 0.254, Fill::None));
    let arrow: &[(f64, f64)] = if n { &[(1.016, 0.0), (2.032, 0.508), (2.032, -0.508), (1.016, 0.0)] } else { &[(2.286, 0.0), (1.27, 0.508), (1.27, -0.508), (2.286, 0.0)] };
    q.graphics.push(line(arrow, 0.254, Fill::Outline));
    fields_right(&mut q, 5.08);
    q
}

fn more_device(lib: &mut Library) {
    let mut cp = two_pin_vertical("C_Polarized", "C", "Polarized capacitor", "cap capacitor electrolytic polarized", &["CP_*"]);
    cp.pins.iter_mut().for_each(|pn| pn.length = mm(2.794));
    cp.graphics.push(rect((-2.032, 0.508), (2.032, 1.016), 0.254, Fill::None));
    cp.graphics.push(rect((-2.032, -1.016), (2.032, -0.508), 0.254, Fill::Outline));
    cp.graphics.push(line(&[(-1.524, 2.286), (-0.508, 2.286)], 0.254, Fill::None));
    cp.graphics.push(line(&[(-1.016, 2.794), (-1.016, 1.778)], 0.254, Fill::None));
    lib.put_symbol(cp);

    let mut rs = two_pin_vertical("R_Small", "R", "Resistor, small symbol", "R res resistor", &["R_*"]);
    rs.pins.iter_mut().for_each(|pn| {
        pn.at.y = pn.at.y.signum() * mm(2.54);
        pn.length = mm(0.762);
    });
    rs.graphics.push(rect((-0.762, -1.778), (0.762, 1.778), 0.2032, Fill::None));
    fields_right(&mut rs, 1.524);
    lib.put_symbol(rs);

    let mut cs = two_pin_vertical("C_Small", "C", "Unpolarized capacitor, small symbol", "cap capacitor", &["C_*"]);
    cs.pins.iter_mut().for_each(|pn| {
        pn.at.y = pn.at.y.signum() * mm(2.54);
        pn.length = mm(2.032);
    });
    cs.graphics.push(line(&[(-1.524, 0.508), (1.524, 0.508)], 0.3302, Fill::None));
    cs.graphics.push(line(&[(-1.524, -0.508), (1.524, -0.508)], 0.3048, Fill::None));
    fields_right(&mut cs, 2.54);
    lib.put_symbol(cs);

    let mut fb = two_pin_vertical("FerriteBead", "FB", "Ferrite bead", "L ferrite bead inductor filter", &["Inductor_*", "L_*", "*Ferrite*"]);
    fb.pins.iter_mut().for_each(|pn| pn.length = mm(1.27));
    fb.graphics.push(line(&[(0.898, 1.796), (1.796, 0.898), (-0.898, -1.796), (-1.796, -0.898), (0.898, 1.796)], 0.254, Fill::None));
    fb.graphics.push(line(&[(0.0, 2.54), (0.0, 0.898)], 0.0, Fill::None));
    fb.graphics.push(line(&[(0.0, -2.54), (0.0, -0.898)], 0.0, Fill::None));
    lib.put_symbol(fb);

    let mut fuse = two_pin_vertical("Fuse", "F", "Fuse", "fuse", &["*Fuse*"]);
    fuse.pins.iter_mut().for_each(|pn| pn.length = mm(1.27));
    fuse.graphics.push(rect((-0.762, -2.54), (0.762, 2.54), 0.254, Fill::None));
    fuse.graphics.push(line(&[(0.0, 2.54), (0.0, -2.54)], 0.0, Fill::None));
    lib.put_symbol(fuse);

    let mut pot = two_pin_vertical("R_Pot", "RV", "Potentiometer", "resistor variable potentiometer", &["Potentiometer*"]);
    pot.pins[1].number = "3".into();
    pot.pins.insert(1, pin("2", "3", PinType::Passive, (3.81, 0.0), 180.0, 1.524));
    pot.pins[1].name = "W".into();
    pot.graphics.push(rect((-1.016, -2.54), (1.016, 2.54), 0.254, Fill::None));
    pot.graphics.push(line(&[(1.143, 0.0), (1.778, 0.381), (1.778, -0.381), (1.143, 0.0)], 0.0, Fill::Outline));
    pot.graphics.push(line(&[(1.778, 0.0), (2.286, 0.0)], 0.0, Fill::None));
    fields_right(&mut pot, 3.81);
    lib.put_symbol(pot);

    let mut ntc = two_pin_vertical("Thermistor_NTC", "TH", "Temperature dependent resistor, negative temperature coefficient", "thermistor NTC resistor sensor", &["*NTC*", "*Thermistor*", "R_*"]);
    ntc.graphics.push(rect((-1.016, -2.54), (1.016, 2.54), 0.254, Fill::None));
    ntc.graphics.push(line(&[(-2.032, -2.54), (-2.032, -1.778), (2.032, 1.778)], 0.254, Fill::None));
    lib.put_symbol(ntc);

    lib.put_symbol(diode_like("D_Zener", "Zener diode", "diode zener", &["TO-???*", "*_Diode_*", "D_*"], &[(-1.778, -1.524), (-1.27, -1.27), (-1.27, 1.27), (-0.762, 1.524)]));
    lib.put_symbol(diode_like("D_Schottky", "Schottky diode", "diode Schottky", &["TO-???*", "*_Diode_*", "D_*"], &[(-1.905, -0.635), (-1.905, -1.27), (-1.27, -1.27), (-1.27, 1.27), (-0.635, 1.27), (-0.635, 0.635)]));

    lib.put_symbol(bjt("Q_NPN_BEC", true));
    lib.put_symbol(bjt("Q_PNP_BEC", false));
    lib.put_symbol(mosfet("Q_NMOS_GSD", true));
    lib.put_symbol(mosfet("Q_PMOS_GSD", false));

    let mut y = new_symbol("Crystal", "Y", "Two-pin crystal");
    y.keywords = "quartz ceramic resonator oscillator".into();
    filters(&mut y, &["Crystal*"]);
    y.show_pin_names = false;
    y.show_pin_numbers = false;
    y.pins = vec![pin("1", "1", PinType::Passive, (-3.81, 0.0), 0.0, 1.524), pin("2", "2", PinType::Passive, (3.81, 0.0), 180.0, 1.524)];
    y.graphics.push(line(&[(-1.905, -1.524), (-1.905, 1.524)], 0.508, Fill::None));
    y.graphics.push(line(&[(1.905, -1.524), (1.905, 1.524)], 0.508, Fill::None));
    y.graphics.push(rect((-1.143, -2.286), (1.143, 2.286), 0.254, Fill::None));
    y.graphics.push(line(&[(-2.286, 0.0), (-1.905, 0.0)], 0.0, Fill::None));
    y.graphics.push(line(&[(2.286, 0.0), (1.905, 0.0)], 0.0, Fill::None));
    fields_above_below(&mut y, 3.81, 3.81);
    lib.put_symbol(y);

    let mut ant = new_symbol("Antenna", "AE", "Antenna");
    ant.keywords = "antenna".into();
    ant.show_pin_names = false;
    ant.show_pin_numbers = false;
    ant.pins = vec![pin("1", "A", PinType::Input, (0.0, -5.08), 90.0, 3.81)];
    ant.graphics.push(line(&[(0.0, -1.27), (0.0, 3.81)], 0.254, Fill::None));
    ant.graphics.push(line(&[(-1.905, 3.81), (0.0, 1.27), (1.905, 3.81), (-1.905, 3.81)], 0.254, Fill::None));
    fields_right(&mut ant, 2.54);
    ant.fields[0].text.at = p(2.54, 2.54);
    ant.fields[1].text.at = p(2.54, 0.0);
    lib.put_symbol(ant);

    let mut bat = new_symbol("Battery", "BT", "Multiple-cell battery");
    bat.keywords = "batt voltage-source cell".into();
    bat.show_pin_names = false;
    bat.show_pin_numbers = false;
    bat.pins = vec![pin("1", "+", PinType::Passive, (0.0, 5.08), 270.0, 2.54), pin("2", "-", PinType::Passive, (0.0, -5.08), 90.0, 2.54)];
    for dy in [1.27, -1.27] {
        bat.graphics.push(rect((-2.032, dy + 0.762), (2.032, dy + 1.016), 0.254, Fill::Outline));
        bat.graphics.push(rect((-1.27, dy + 0.254), (1.27, dy - 0.254), 0.254, Fill::Outline));
    }
    bat.graphics.push(line(&[(0.0, 2.286), (0.0, 2.54)], 0.254, Fill::None));
    bat.graphics.push(line(&[(0.0, -1.524), (0.0, -2.54)], 0.254, Fill::None));
    bat.graphics.push(line(&[(0.0, 1.524), (0.0, 1.016)], 0.254, Fill::None));
    bat.graphics.push(line(&[(0.762, 3.556), (1.778, 3.556)], 0.254, Fill::None));
    bat.graphics.push(line(&[(1.27, 4.064), (1.27, 3.048)], 0.254, Fill::None));
    fields_right(&mut bat, 3.048);
    lib.put_symbol(bat);

    let mut bz = new_symbol("Buzzer", "BZ", "Buzzer, polarized");
    bz.keywords = "quartz resonator ceramic buzzer".into();
    filters(&mut bz, &["*Buzzer*", "*Beeper*"]);
    bz.show_pin_names = false;
    bz.pins = vec![pin("1", "+", PinType::Passive, (-5.08, 1.27), 0.0, 2.54), pin("2", "-", PinType::Passive, (-5.08, -1.27), 0.0, 2.54)];
    bz.graphics.push(line(&[(-2.54, 2.54), (-2.54, -2.54)], 0.254, Fill::None));
    bz.graphics.push(arc((-2.54, 2.54), (0.0, 0.0), (-2.54, -2.54), 0.254));
    bz.graphics.push(line(&[(-3.556, 2.286), (-3.556, 3.302)], 0.0, Fill::None));
    bz.graphics.push(line(&[(-4.064, 2.794), (-3.048, 2.794)], 0.0, Fill::None));
    fields_right(&mut bz, 1.27);
    lib.put_symbol(bz);

    let mut sp = new_symbol("Speaker", "LS", "Speaker");
    sp.keywords = "speaker sound".into();
    sp.show_pin_names = false;
    sp.pins = vec![pin("1", "1", PinType::Input, (-5.08, 1.27), 0.0, 2.54), pin("2", "2", PinType::Input, (-5.08, -1.27), 0.0, 2.54)];
    sp.graphics.push(rect((-2.54, -1.778), (-1.016, 1.778), 0.254, Fill::None));
    sp.graphics.push(line(&[(-1.016, 1.778), (1.016, 3.81), (1.016, -3.81), (-1.016, -1.778)], 0.254, Fill::None));
    fields_right(&mut sp, 2.54);
    lib.put_symbol(sp);
}

// ---------------------------------------------------------------------------------------------
// Switch

fn switch_base(name: &str, desc: &str) -> Symbol {
    let mut s = new_symbol(name, "SW", desc);
    s.show_pin_names = false;
    s.show_pin_numbers = false;
    s.keywords = "switch".into();
    fields_above_below(&mut s, 3.81, 2.54);
    s
}

pub(super) fn switch() -> Library {
    let mut lib = Library::new("Switch", Scope::Global);
    lib.description = "Switches and push buttons".into();

    let mut push = switch_base("SW_Push", "Push button switch, generic, two pins");
    push.keywords = "switch normally-open pushbutton push-button".into();
    filters(&mut push, &["SW_PUSH*", "*Push*", "*Button*", "SW_*"]);
    push.pins = vec![pin("1", "1", PinType::Passive, (-5.08, 0.0), 0.0, 2.032), pin("2", "2", PinType::Passive, (5.08, 0.0), 180.0, 2.032)];
    push.graphics.push(circle((-2.54, 0.0), 0.508, 0.0, Fill::None));
    push.graphics.push(circle((2.54, 0.0), 0.508, 0.0, Fill::None));
    push.graphics.push(line(&[(-3.048, 1.27), (3.048, 1.27)], 0.0, Fill::None));
    push.graphics.push(line(&[(0.0, 1.27), (0.0, 3.048)], 0.0, Fill::None));
    push.fields[0].text.at = p(0.0, 5.08);
    lib.put_symbol(push);

    let mut spst = switch_base("SW_SPST", "Single pole single throw switch");
    filters(&mut spst, &["SW_*"]);
    spst.pins = vec![pin("1", "A", PinType::Passive, (-5.08, 0.0), 0.0, 2.032), pin("2", "B", PinType::Passive, (5.08, 0.0), 180.0, 2.032)];
    spst.graphics.push(circle((-2.54, 0.0), 0.508, 0.0, Fill::None));
    spst.graphics.push(circle((2.54, 0.0), 0.508, 0.0, Fill::None));
    spst.graphics.push(line(&[(-2.032, 0.254), (2.032, 1.524)], 0.0, Fill::None));
    lib.put_symbol(spst);

    let mut spdt = switch_base("SW_SPDT", "Single pole double throw switch");
    filters(&mut spdt, &["SW_*"]);
    spdt.pins = vec![
        pin("1", "A", PinType::Passive, (5.08, 2.54), 180.0, 2.032),
        pin("2", "B", PinType::Passive, (-5.08, 0.0), 0.0, 2.032),
        pin("3", "C", PinType::Passive, (5.08, -2.54), 180.0, 2.032),
    ];
    spdt.graphics.push(circle((-2.54, 0.0), 0.508, 0.0, Fill::None));
    spdt.graphics.push(circle((2.54, 2.54), 0.508, 0.0, Fill::None));
    spdt.graphics.push(circle((2.54, -2.54), 0.508, 0.0, Fill::None));
    spdt.graphics.push(line(&[(-2.032, 0.254), (2.159, 2.159)], 0.0, Fill::None));
    spdt.fields[0].text.at = p(0.0, 5.08);
    spdt.fields[1].text.at = p(0.0, -5.08);
    lib.put_symbol(spdt);
    lib
}

// ---------------------------------------------------------------------------------------------
// Connector

/// Pin rows from the top: `n` rows 2.54 mm apart, centred on y = 0 (on the 1.27 mm grid).
fn rows(n: usize) -> impl Iterator<Item = f64> {
    let top = (n as f64 - 1.0) * 1.27;
    (0..n).map(move |i| top - i as f64 * 2.54)
}

/// A generic connector: `cols` 1 (pins left) or 2 (odd left, even right), `n` rows.
pub fn connector(cols: usize, n: usize) -> Symbol {
    let name = if cols == 1 { format!("Conn_01x{n:02}") } else { format!("Conn_02x{n:02}_Odd_Even") };
    let pins = cols * n;
    let mut s = new_symbol(&name, "J", &format!("Generic connector, {} row{}, {pins} pins", if cols == 1 { "single" } else { "double" }, if cols == 1 { "" } else { "s" }));
    s.keywords = "connector".into();
    filters(&mut s, &[if cols == 1 { "Connector*:*_1x??_*" } else { "Connector*:*_2x??_*" }]);
    s.show_pin_names = false;
    let top = (n as f64 - 1.0) * 1.27;
    let (x0, x1) = (-1.27, 1.27);
    s.graphics.push(rect((x0, top + 1.27), (x1, -top - 1.27), 0.254, Fill::Background));
    for (i, y) in rows(n).enumerate() {
        if cols == 1 {
            s.pins.push(pin(&(i + 1).to_string(), &format!("Pin_{}", i + 1), PinType::Passive, (-5.08, y), 0.0, 3.81));
            s.graphics.push(rect((-1.27, y + 0.127), (0.0, y - 0.127), 0.1524, Fill::None));
        } else {
            let (a, b) = (2 * i + 1, 2 * i + 2);
            s.pins.push(pin(&a.to_string(), &format!("Pin_{a}"), PinType::Passive, (-5.08, y), 0.0, 3.81));
            s.pins.push(pin(&b.to_string(), &format!("Pin_{b}"), PinType::Passive, (5.08, y), 180.0, 3.81));
            s.graphics.push(rect((-1.27, y + 0.127), (0.0, y - 0.127), 0.1524, Fill::None));
            s.graphics.push(rect((0.0, y + 0.127), (1.27, y - 0.127), 0.1524, Fill::None));
        }
    }
    s.fields[0].text.at = p(0.0, top + 2.54);
    s.fields[1].text.at = p(0.0, -top - 2.54);
    s.fields.iter_mut().take(2).for_each(|f| f.text.style.h_align = HAlign::Center);
    s
}

pub(super) fn connector_lib() -> Library {
    let mut lib = Library::new("Connector", Scope::Global);
    lib.description = "Generic connectors, screw terminals, test points".into();
    for n in 1..=40 {
        lib.put_symbol(connector(1, n));
        lib.put_symbol(connector(2, n));
    }
    for n in 2..=8 {
        let mut s = connector(1, n);
        let name = format!("Screw_Terminal_01x{n:02}");
        s.id = name.clone();
        s.fields[1].text.text = name;
        s.keywords = "screw terminal".into();
        s.footprint_filters = vec!["TerminalBlock*:*".into()];
        if let Some(d) = s.field_mut(fields::DESCRIPTION) {
            d.text.text = format!("Generic screw terminal, single row, {n} pins");
        }
        lib.put_symbol(s);
    }
    let mut tp = new_symbol("TestPoint", "TP", "Test point");
    tp.keywords = "test point tp".into();
    filters(&mut tp, &["Pin*", "Test*"]);
    tp.show_pin_names = false;
    tp.show_pin_numbers = false;
    tp.pins = vec![pin("1", "1", PinType::Passive, (0.0, 0.0), 90.0, 2.286)];
    tp.graphics.push(circle((0.0, 3.048), 0.762, 0.254, Fill::None));
    tp.fields[0].text.at = p(1.524, 4.318);
    tp.fields[1].text.at = p(1.524, 2.54);
    lib.put_symbol(tp);
    lib
}

// ---------------------------------------------------------------------------------------------
// power

#[derive(Clone, Copy)]
enum Port {
    Up,
    Down,
    Ground,
    GroundRef,
    Earth,
}

fn port(name: &str, style: Port) -> Symbol {
    let mut s = power_port(name, matches!(style, Port::Ground | Port::GroundRef | Port::Earth | Port::Down));
    s.graphics.clear();
    match style {
        Port::Up => {
            s.graphics.push(line(&[(0.0, 0.0), (0.0, 1.905)], 0.254, Fill::None));
            s.graphics.push(line(&[(-0.762, 1.27), (0.0, 2.54), (0.762, 1.27)], 0.254, Fill::None));
            s.fields[1].text.at = p(0.0, 3.556);
        }
        Port::Down => {
            s.graphics.push(line(&[(0.0, 0.0), (0.0, -1.905)], 0.254, Fill::None));
            s.graphics.push(line(&[(-0.762, -1.27), (0.0, -2.54), (0.762, -1.27)], 0.254, Fill::None));
            s.fields[1].text.at = p(0.0, -3.81);
        }
        Port::Ground => {
            s.graphics.push(line(&[(0.0, 0.0), (0.0, -1.27), (1.27, -1.27), (0.0, -2.54), (-1.27, -1.27), (0.0, -1.27)], 0.254, Fill::None));
            s.fields[1].text.at = p(0.0, -3.81);
        }
        Port::GroundRef => {
            s.graphics.push(line(&[(0.0, 0.0), (0.0, -1.27)], 0.254, Fill::None));
            s.graphics.push(line(&[(-1.27, -1.27), (1.27, -1.27), (0.0, -2.54), (-1.27, -1.27)], 0.254, Fill::Outline));
            s.fields[1].text.at = p(0.0, -3.81);
        }
        Port::Earth => {
            s.graphics.push(line(&[(0.0, 0.0), (0.0, -1.27)], 0.254, Fill::None));
            s.graphics.push(line(&[(-1.27, -1.27), (1.27, -1.27)], 0.254, Fill::None));
            s.graphics.push(line(&[(-0.762, -1.778), (0.762, -1.778)], 0.254, Fill::None));
            s.graphics.push(line(&[(-0.254, -2.286), (0.254, -2.286)], 0.254, Fill::None));
            s.fields[1].text.at = p(0.0, -3.81);
        }
    }
    s
}

fn more_power(lib: &mut Library) {
    for n in ["VDD", "VBUS", "VBAT", "+1V8", "+2V5", "+3V0", "+3.3VA", "+12V", "+24V"] {
        lib.put_symbol(port(n, Port::Up));
    }
    for n in ["-5V", "-12V", "VSS", "VEE"] {
        lib.put_symbol(port(n, Port::Down));
    }
    for n in ["GNDA", "GNDD", "GNDPWR"] {
        lib.put_symbol(port(n, Port::Ground));
    }
    lib.put_symbol(port("GNDREF", Port::GroundRef));
    lib.put_symbol(port("Earth", Port::Earth));
}

// ---------------------------------------------------------------------------------------------
// ICs: regulators, op-amps, addressable LEDs

/// A box symbol `w` × `h` (mm, centred) with named pins: (number, name, type, x, y, angle).
fn ic(name: &str, prefix: &str, desc: &str, w: f64, h: f64, pins: &[(&str, &str, PinType, f64, f64, f64)]) -> Symbol {
    let mut s = new_symbol(name, prefix, desc);
    s.graphics.push(rect((-w / 2.0, h / 2.0), (w / 2.0, -h / 2.0), 0.254, Fill::Background));
    for &(num, nm, kind, x, y, angle) in pins {
        // Pins run from (x, y) to the body's edge.
        let len = match angle as i32 {
            0 => -w / 2.0 - x,
            180 => x - w / 2.0,
            90 => -h / 2.0 - y,
            _ => y - h / 2.0,
        };
        s.pins.push(pin(num, nm, kind, (x, y), angle, len.abs()));
    }
    s.fields[0].text.at = p(-w / 2.0, h / 2.0 + 1.27);
    // The value stacked above the reference: long part names stay clear of it.
    s.fields[1].text.at = p(-w / 2.0, h / 2.0 + 3.302);
    s
}

pub(super) fn regulators() -> Library {
    let mut lib = Library::new("Regulator_Linear", Scope::Global);
    lib.description = "Linear regulators".into();
    use PinType::{PowerIn, PowerOut};
    let three = |name: &str, desc: &str, gnd: &str, out: &str, inp: &str, fp: &str| {
        let mut s = ic(name, "U", desc, 10.16, 6.35, &[(gnd, "GND", PowerIn, 0.0, -7.62, 90.0), (out, "VO", PowerOut, 7.62, 0.0, 180.0), (inp, "VI", PowerIn, -7.62, 0.0, 0.0)]);
        s.graphics.clear();
        s.graphics.push(rect((-5.08, 1.905), (5.08, -5.08), 0.254, Fill::Background));
        s.keywords = "linear regulator ldo fixed positive".into();
        s.fields[0].text.at = p(-3.81, 3.175);
        s.fields[1].text.at = p(0.0, 3.175);
        s.fields[1].text.style.h_align = HAlign::Left;
        with_footprint(s, fp)
    };
    for (v, vo) in [("3.3", "3.3"), ("5.0", "5.0"), ("1.8", "1.8")] {
        lib.put_symbol(three(&format!("AMS1117-{v}"), &format!("1 A low dropout linear regulator, fixed {vo} V output, SOT-223"), "1", "2", "3", "Package_TO_SOT_SMD:SOT-223-3_TabPin2"));
    }
    lib.put_symbol(three("LM1117-3.3", "800 mA low dropout linear regulator, fixed 3.3 V output, SOT-223", "1", "2", "3", "Package_TO_SOT_SMD:SOT-223-3_TabPin2"));
    lib.put_symbol(three("MCP1700x-330xxTT", "250 mA low quiescent current LDO, 3.3 V output, SOT-23", "1", "2", "3", "Package_TO_SOT_SMD:SOT-23"));
    let mut ap = ic(
        "AP2112K-3.3",
        "U",
        "600 mA low dropout linear regulator with enable, 3.3 V output, SOT-23-5",
        10.16,
        7.62,
        &[
            ("1", "VIN", PowerIn, -7.62, 2.54, 0.0),
            ("2", "GND", PowerIn, 0.0, -7.62, 90.0),
            ("3", "EN", PinType::Input, -7.62, 0.0, 0.0),
            ("4", "NC", PinType::NoConnect, 7.62, 0.0, 180.0),
            ("5", "VOUT", PowerOut, 7.62, 2.54, 180.0),
        ],
    );
    ap.keywords = "linear regulator ldo fixed positive enable".into();
    lib.put_symbol(with_footprint(ap, "Package_TO_SOT_SMD:SOT-23-5"));
    lib
}

pub(super) fn opamps() -> Library {
    let mut lib = Library::new("Amplifier_Operational", Scope::Global);
    lib.description = "Operational amplifiers".into();
    let amp = |name: &str, desc: &str, nums: [&str; 5], fp: &str| {
        let mut s = new_symbol(name, "U", desc);
        s.keywords = "single opamp".into();
        s.show_pin_names = true;
        s.graphics.push(line(&[(-5.08, 5.08), (5.08, 0.0), (-5.08, -5.08), (-5.08, 5.08)], 0.254, Fill::Background));
        s.pins = vec![
            pin(nums[0], "+", PinType::Input, (-7.62, 2.54), 0.0, 2.54),
            pin(nums[1], "-", PinType::Input, (-7.62, -2.54), 0.0, 2.54),
            pin(nums[2], "~", PinType::Output, (7.62, 0.0), 180.0, 2.54),
            pin(nums[3], "V+", PinType::PowerIn, (-2.54, 7.62), 270.0, 3.81),
            pin(nums[4], "V-", PinType::PowerIn, (-2.54, -7.62), 90.0, 3.81),
        ];
        s.fields[0].text.at = p(0.0, 6.35);
        s.fields[1].text.at = p(0.0, 3.81);
        with_footprint(s, fp)
    };
    lib.put_symbol(amp("Opamp_Generic", "Generic single operational amplifier", ["3", "2", "1", "7", "4"], ""));
    lib.put_symbol(amp("MCP6001-OT", "1 MHz, low-power op amp, SOT-23-5", ["3", "4", "1", "5", "2"], "Package_TO_SOT_SMD:SOT-23-5"));
    lib.put_symbol(amp("LMV321", "Low-voltage rail-to-rail output op amp, SOT-23-5", ["1", "3", "4", "5", "2"], "Package_TO_SOT_SMD:SOT-23-5"));
    lib
}

pub(super) fn leds() -> Library {
    let mut lib = Library::new("LED", Scope::Global);
    lib.description = "Addressable and multi-pin LEDs".into();
    let mut ws = ic(
        "WS2812B",
        "D",
        "RGB LED with integrated controller, 5 × 5 mm PLCC-4",
        10.16,
        10.16,
        &[
            ("1", "VDD", PinType::PowerIn, 0.0, 7.62, 270.0),
            ("2", "DOUT", PinType::Output, 7.62, 0.0, 180.0),
            ("3", "VSS", PinType::PowerIn, 0.0, -7.62, 90.0),
            ("4", "DIN", PinType::Input, -7.62, 0.0, 0.0),
        ],
    );
    ws.keywords = "RGB LED NeoPixel addressable".into();
    lib.put_symbol(with_footprint(ws, "LED_SMD:LED_WS2812B_PLCC4_5.0x5.0mm_P3.2mm"));
    lib
}
