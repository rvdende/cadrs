//! Built-in footprints, generated: chips, diodes, SOT/SOIC/TSSOP/QFN/QFP packages, pin headers
//! and sockets, JST connectors, crystals, radial and axial parts, mounting holes, test points,
//! buttons. KiCad's library and footprint names (so symbols' footprint fields and filters
//! match), cadrs' own drawing. Every footprint carries a generated 3D body ([`model3d`]).
//!
//! Coordinates: mm, Y up (KiCad's footprints have Y down: its pin 1 at the top-left is ours
//! too, at +y). Pin 1 of a header sits at the origin, the rest below it.

use super::*;
use cadrs_eda::model3d::{self, Body, colors};

/// Silkscreen line width and its gap to copper.
const SILK: f64 = 0.12;
const SILK_GAP: f64 = 0.2;

/// A pad's box (mm): x0, y0, x1, y1.
fn pad_box(pd: &Pad) -> (f64, f64, f64, f64) {
    let (mut w, mut h) = (cadrs_eda::units::to_mm(pd.size.w), cadrs_eda::units::to_mm(pd.size.h));
    if (pd.angle.rem_euclid(180.0) - 90.0).abs() < 1.0 {
        std::mem::swap(&mut w, &mut h);
    }
    let (x, y) = (cadrs_eda::units::to_mm(pd.at.x), cadrs_eda::units::to_mm(pd.at.y));
    (x - w / 2.0, y - h / 2.0, x + w / 2.0, y + h / 2.0)
}

/// The parts of the segment from `a` to `b` (horizontal or vertical) clear of the pads.
fn clear_of_pads(f: &Footprint, a: (f64, f64), b: (f64, f64)) -> Vec<((f64, f64), (f64, f64))> {
    let horizontal = (a.1 - b.1).abs() < 1e-9;
    let (fixed, lo, hi) = if horizontal { (a.1, a.0.min(b.0), a.0.max(b.0)) } else { (a.0, a.1.min(b.1), a.1.max(b.1)) };
    let g = SILK_GAP + SILK / 2.0;
    let mut blocked: Vec<(f64, f64)> = f
        .pads
        .iter()
        .map(pad_box)
        .filter_map(|(x0, y0, x1, y1)| {
            let (c0, c1, s0, s1) = if horizontal { (y0 - g, y1 + g, x0 - g, x1 + g) } else { (x0 - g, x1 + g, y0 - g, y1 + g) };
            (fixed > c0 && fixed < c1).then_some((s0, s1))
        })
        .collect();
    blocked.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut out = vec![];
    let mut at = lo;
    for (s0, s1) in blocked {
        if s0 > at {
            out.push((at, s0.min(hi)));
        }
        at = at.max(s1);
    }
    if at < hi {
        out.push((at, hi));
    }
    out.into_iter()
        .filter(|(s, e)| e - s > 0.15)
        .map(|(s, e)| if horizontal { ((s, fixed), (e, fixed)) } else { ((fixed, s), (fixed, e)) })
        .collect()
}

/// A silkscreen rectangle `gap` outside the box, broken where it would touch a pad.
fn silk_rect(f: &mut Footprint, x0: f64, y0: f64, x1: f64, y1: f64) {
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    for i in 0..4 {
        for (a, b) in clear_of_pads(f, corners[i], corners[(i + 1) % 4]) {
            f.shapes.push(fp_shape(Geom::Line { a: p(a.0, a.1), b: p(b.0, b.1) }, Layer::TopSilk, SILK));
        }
    }
}

/// Fabrication outline: the body, its pin-1 corner (top-left) cut by `chamfer`.
fn fab_outline(f: &mut Footprint, x0: f64, y0: f64, x1: f64, y1: f64, chamfer: f64) {
    if chamfer <= 0.0 {
        f.shapes.push(fp_shape(rect_geom(x0, y0, x1, y1), Layer::TopFab, 0.1));
        return;
    }
    let pts = [(x0 + chamfer, y1), (x1, y1), (x1, y0), (x0, y0), (x0, y1 - chamfer)];
    let geom = Geom::Polyline { pts: pts.iter().map(|&(x, y)| p(x, y)).collect(), closed: true };
    f.shapes.push(fp_shape(geom, Layer::TopFab, 0.1));
}

/// The courtyard: around the pads and the drawn body, `margin` out, on a 0.01 mm grid.
fn courtyard(f: &mut Footprint, margin: f64) {
    let mut b: Option<(f64, f64, f64, f64)> = None;
    let mut add = |x0: f64, y0: f64, x1: f64, y1: f64| {
        b = Some(match b {
            None => (x0, y0, x1, y1),
            Some((a0, b0, a1, b1)) => (a0.min(x0), b0.min(y0), a1.max(x1), b1.max(y1)),
        });
    };
    for pd in &f.pads {
        let (x0, y0, x1, y1) = pad_box(pd);
        add(x0, y0, x1, y1);
    }
    for s in f.shapes.iter().filter(|s| s.layer == Layer::TopFab) {
        for q in s.shape.geom.extent() {
            let (x, y) = (cadrs_eda::units::to_mm(q.x), cadrs_eda::units::to_mm(q.y));
            add(x, y, x, y);
        }
    }
    let Some((x0, y0, x1, y1)) = b else { return };
    let r = |v: f64, up: bool| if up { (v * 100.0).ceil() / 100.0 } else { (v * 100.0).floor() / 100.0 };
    f.shapes.push(fp_shape(rect_geom(r(x0 - margin, false), r(y0 - margin, false), r(x1 + margin, true), r(y1 + margin, true)), Layer::TopCourtyard, 0.05));
}

/// Reference above and value below the box (mm), and the fab `${REFERENCE}` at its centre.
fn place_texts(f: &mut Footprint, top: f64, bottom: f64, cx: f64, cy: f64) {
    f.fields[0].text.text.at = p(cx, top + 1.0);
    f.fields[1].text.text.at = p(cx, bottom - 1.0);
    if let Some(t) = f.texts.first_mut() {
        t.text.at = p(cx, cy);
    }
}

fn footprint(lib: &str, name: &str, description: &str, keywords: &str, mount: MountKind) -> Footprint {
    let mut f = new_footprint(&format!("{lib}:{name}"), description, mount, Pt::ZERO, Pt::ZERO, Pt::ZERO);
    f.keywords = keywords.into();
    f
}

fn smd(number: impl ToString, shape: PadShape, x: f64, y: f64, w: f64, h: f64) -> Pad {
    new_pad(&number.to_string(), shape, p(x, y), Size::mm(w, h), None)
}

fn tht(number: impl ToString, shape: PadShape, x: f64, y: f64, w: f64, h: f64, drill: f64) -> Pad {
    new_pad(&number.to_string(), shape, p(x, y), Size::mm(w, h), Some(mm(drill)))
}

const RR: PadShape = PadShape::RoundRect { ratio: 0.25 };

// ---------------------------------------------------------------------------------------------
// The generators the course uses

/// An axial through-hole part lying flat: pads at x = 0 and `pitch`, a `length` × `diameter`
/// body between them. KiCad-style name: `R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal`.
fn axial_tht(lib: &str, name: &str, pitch: f64, length: f64, diameter: f64, drill: f64, pad: f64) -> Footprint {
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
    let color = if lib.starts_with("Diode") { colors::PLASTIC } else { colors::AXIAL };
    f.models.push(model(&format!("{lib}.3dshapes/{name}.step"), model3d::axial(pitch, length, diameter, color)));
    f
}

/// A round radial LED: pad 1 (cathode, square) at the origin, pad 2 at 2.54 mm.
fn radial_led_tht(diameter: f64) -> Footprint {
    let name = format!("LED_D{diameter:.1}mm");
    let id = format!("LED_THT:{name}");
    let c = p(1.27, 0.0);
    let r = diameter / 2.0;
    let mut f = new_footprint(&id, &format!("LED, diameter {diameter} mm, 2 pins"), MountKind::ThroughHole, p(1.27, r + 1.0), p(1.27, -r - 1.0), c);
    f.pads.push(new_pad("1", PadShape::Rect, p(0.0, 0.0), Size::mm(1.8, 1.8), Some(mm(0.9))));
    f.pads.push(new_pad("2", PadShape::Circle, p(2.54, 0.0), Size::mm(1.8, 1.8), Some(mm(0.9))));
    f.shapes.push(fp_shape(Geom::Circle { center: c, radius: mm(r) }, Layer::TopFab, 0.1));
    // Clear of the pads on small LEDs.
    f.shapes.push(fp_shape(Geom::Circle { center: c, radius: mm((r + 0.12).max(2.6)) }, Layer::TopSilk, 0.12));
    f.shapes.push(fp_shape(Geom::Circle { center: c, radius: mm((r + 0.5).max(1.27 + 0.9 + 0.5)) }, Layer::TopCourtyard, 0.05));
    let mut body = model3d::can([1.27, 0.0], r, diameter + 2.6, colors::LED_RED);
    body.cylinder([1.27, 0.0, 0.0], model3d::Axis::Z, r + 0.4, 1.0, colors::LED_RED);
    for x in [0.0, 2.54] {
        body.cuboid([x - 0.25, -0.25, -3.0], [x + 0.25, 0.25, 0.0], colors::TIN);
    }
    f.models.push(model(&format!("LED_THT.3dshapes/{name}.step"), body));
    f
}

/// A surface-mount coin-cell holder for one 20 mm cell: pads 1 (+) left and 2 (−) right.
fn coin_holder() -> Footprint {
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
    let mut body = Body::default();
    body.cuboid([-13.0, -3.5, 0.0], [13.0, 3.5, 4.2], colors::PLASTIC);
    body.cylinder([0.0, 0.0, 0.0], model3d::Axis::Z, 10.0, 3.2, colors::PLASTIC);
    body.cuboid([-16.0, -2.5, 0.0], [-13.0, 2.5, 0.3], colors::TIN);
    body.cuboid([13.0, -2.5, 0.0], [16.0, 2.5, 0.3], colors::TIN);
    f.models.push(model("Battery.3dshapes/BatteryHolder_Keystone_1058_1x2032.step", body));
    f
}

/// A two-terminal chip part (0402 … 2512 style): pads at ±`pitch`/2.
fn chip_smd(lib: &str, name: &str, pitch: f64, pad: Size, body: Size) -> Footprint {
    let id = format!("{lib}:{name}");
    let (bw, bh) = (cadrs_eda::units::to_mm(body.w), cadrs_eda::units::to_mm(body.h));
    let mut f = new_footprint(&id, &format!("Chip, {bw} × {bh} mm"), MountKind::Smd, p(0.0, bh / 2.0 + 1.0), p(0.0, -bh / 2.0 - 1.0), Pt::ZERO);
    f.pads.push(new_pad("1", PadShape::RoundRect { ratio: 0.25 }, p(-pitch / 2.0, 0.0), pad, None));
    f.pads.push(new_pad("2", PadShape::RoundRect { ratio: 0.25 }, p(pitch / 2.0, 0.0), pad, None));
    f.shapes.push(fp_shape(rect_geom(-bw / 2.0, -bh / 2.0, bw / 2.0, bh / 2.0), Layer::TopFab, 0.1));
    let (cx, cy) = (pitch / 2.0 + cadrs_eda::units::to_mm(pad.w) / 2.0 + 0.25, (bh / 2.0).max(cadrs_eda::units::to_mm(pad.h) / 2.0) + 0.25);
    f.shapes.push(fp_shape(rect_geom(-cx, -cy, cx, cy), Layer::TopCourtyard, 0.05));
    // Silkscreen along the long sides, between the pads.
    let sx = (pitch / 2.0 - cadrs_eda::units::to_mm(pad.w) / 2.0 - SILK_GAP - SILK / 2.0).max(0.0);
    let sy = (bh / 2.0).max(cadrs_eda::units::to_mm(pad.h) / 2.0 - 0.1) + SILK_GAP;
    if sx > 0.1 && bh > 0.7 {
        for y in [sy, -sy] {
            f.shapes.push(fp_shape(Geom::Line { a: p(-sx, y), b: p(sx, y) }, Layer::TopSilk, SILK));
        }
    }
    let t = (bw * 0.2).min(0.6);
    let h = (bh * 0.7).clamp(0.3, 1.6);
    let b = if lib.starts_with("LED") {
        model3d::chip_led(bw, bh, h, t)
    } else {
        let c = if lib.starts_with("Resistor") {
            colors::RESISTOR
        } else if lib.starts_with("Inductor") {
            colors::FERRITE
        } else {
            colors::CERAMIC
        };
        model3d::chip(bw, bh, h, t, c)
    };
    f.models.push(model(&format!("{lib}.3dshapes/{name}.step"), b));
    f
}

// ---------------------------------------------------------------------------------------------
// Chips

/// Chip sizes: imperial code, metric code, body l × w, pad pitch, pad w × h.
const CHIPS: [(&str, &str, f64, f64, f64, f64, f64); 9] = [
    ("0201", "0603", 0.6, 0.3, 0.64, 0.4, 0.4),
    ("0402", "1005", 1.0, 0.5, 0.96, 0.56, 0.62),
    ("0603", "1608", 1.6, 0.8, 1.55, 0.9, 0.95),
    ("0805", "2012", 2.0, 1.25, 1.9, 1.0, 1.45),
    ("1206", "3216", 3.2, 1.6, 2.95, 1.15, 1.8),
    ("1210", "3225", 3.2, 2.5, 2.95, 1.15, 2.7),
    ("1812", "4532", 4.5, 3.2, 4.2, 1.3, 3.4),
    ("2010", "5025", 5.0, 2.5, 4.6, 1.4, 2.65),
    ("2512", "6332", 6.3, 3.2, 5.8, 1.65, 3.4),
];

fn chips(lib: &str, prefix: &str) -> Vec<Footprint> {
    CHIPS
        .iter()
        .map(|&(imp, met, l, w, pitch, pw, ph)| {
            // Chip inductors are thinner, with longer pads, than capacitors of the same size.
            let (l, w, pitch, pw, ph) = if prefix == "L" && imp == "0805" { (2.0, 0.9, 2.125, 0.875, 1.2) } else { (l, w, pitch, pw, ph) };
            let mut f = chip_smd(lib, &format!("{prefix}_{imp}_{met}Metric"), pitch, Size::mm(pw, ph), Size::mm(l, w));
            f.description = format!("{prefix} chip {imp} ({met} metric), {l} × {w} mm");
            f.keywords = format!("{} {imp}", prefix.to_lowercase());
            f
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Two-terminal SMD diodes

fn diode_smd(name: &str, body: (f64, f64, f64), pitch: f64, pad: (f64, f64)) -> Footprint {
    let mut f = footprint("Diode_SMD", name, &format!("{name} diode package"), "diode", MountKind::Smd);
    let (l, w, h) = body;
    f.pads.push(smd(1, RR, -pitch / 2.0, 0.0, pad.0, pad.1));
    f.pads.push(smd(2, RR, pitch / 2.0, 0.0, pad.0, pad.1));
    fab_outline(&mut f, -l / 2.0, -w / 2.0, l / 2.0, w / 2.0, 0.0);
    // The cathode side (pad 1) gets a closed silkscreen end.
    let (x0, x1, y) = (-pitch / 2.0 - pad.0 / 2.0 - 0.3, l / 2.0, w / 2.0 + SILK_GAP);
    f.shapes.push(fp_shape(Geom::Polyline { pts: vec![p(x1, y), p(x0, y), p(x0, -y), p(x1, -y)], closed: false }, Layer::TopSilk, SILK));
    courtyard(&mut f, 0.25);
    place_texts(&mut f, w / 2.0 + 0.5, -w / 2.0 - 0.5, 0.0, 0.0);
    let mut b = model3d::gull_wing(l, w, h, 0.05, &[], 0.0, 0.0);
    b.cuboid([-l / 2.0 + 0.2, -w / 2.0, h - 0.02], [-l / 2.0 + 0.6, w / 2.0, h + 0.01], colors::SHIELD);
    b.cuboid([-pitch / 2.0 - pad.0 / 2.0 + 0.1, -0.4, 0.0], [-l / 2.0, 0.4, 0.15], colors::TIN);
    b.cuboid([l / 2.0, -0.4, 0.0], [pitch / 2.0 + pad.0 / 2.0 - 0.1, 0.4, 0.15], colors::TIN);
    f.models.push(model(&format!("Diode_SMD.3dshapes/{name}.step"), b));
    f
}

// ---------------------------------------------------------------------------------------------
// Leaded IC packages

/// Dual-row gull-wing package: `n` pins, `pitch`, pads `pw` × `ph` centred `x` from the axis,
/// body `bl` (across) × `bw` (along) × `h`, lead tips at `span`/2.
#[allow(clippy::too_many_arguments)]
fn dual_row(lib: &str, name: &str, desc: &str, n: usize, pitch: f64, x: f64, pw: f64, ph: f64, bl: f64, bw: f64, h: f64, span: f64) -> Footprint {
    let mut f = footprint(lib, name, desc, &name.split(['-', '_']).next().unwrap_or("").to_lowercase(), MountKind::Smd);
    let per = n / 2;
    let top = (per as f64 - 1.0) * pitch / 2.0;
    let mut leads = vec![];
    for i in 0..per {
        let y = top - i as f64 * pitch;
        f.pads.push(smd(i + 1, RR, -x, y, pw, ph));
        leads.push((-span / 2.0, y));
    }
    for i in 0..per {
        let y = -top + i as f64 * pitch;
        f.pads.push(smd(per + i + 1, RR, x, y, pw, ph));
        leads.push((span / 2.0, y));
    }
    fab_outline(&mut f, -bl / 2.0, -bw / 2.0, bl / 2.0, bw / 2.0, (bl * 0.25).min(1.0));
    let sy = bw / 2.0 + 0.11;
    let sx = (bl / 2.0 + 0.11).min(x - pw / 2.0 - SILK_GAP);
    f.shapes.push(fp_shape(Geom::Line { a: p(-sx, sy), b: p(sx, sy) }, Layer::TopSilk, SILK));
    f.shapes.push(fp_shape(Geom::Line { a: p(-sx, -sy), b: p(sx, -sy) }, Layer::TopSilk, SILK));
    // Pin 1: a short silkscreen line out along its row.
    f.shapes.push(fp_shape(Geom::Line { a: p(-sx, sy), b: p(-x - pw / 2.0, sy) }, Layer::TopSilk, SILK));
    courtyard(&mut f, 0.25);
    place_texts(&mut f, bw / 2.0 + 0.5, -bw / 2.0 - 0.5, 0.0, 0.0);
    let body = model3d::gull_wing(bl, bw, h, 0.1, &leads, ph * 0.7, 0.25);
    f.models.push(model(&format!("{lib}.3dshapes/{name}.step"), body));
    f
}

fn sot23(name: &str, n: usize) -> Footprint {
    let desc = format!("SOT-23, {n} pins");
    let (x, pw, ph) = (1.1375, 1.325, 0.6);
    let mut f = footprint("Package_TO_SOT_SMD", name, &desc, "SOT TO_SOT_SMD", MountKind::Smd);
    let mut leads = vec![];
    if n == 3 {
        f.pads.push(smd(1, RR, -x, 0.95, pw, ph));
        f.pads.push(smd(2, RR, -x, -0.95, pw, ph));
        f.pads.push(smd(3, RR, x, 0.0, pw, ph));
        leads.extend([(-1.4, 0.95), (-1.4, -0.95), (1.4, 0.0)]);
    } else {
        for i in 0..3 {
            let y = 0.95 - i as f64 * 0.95;
            f.pads.push(smd(i + 1, RR, -x, y, pw, ph));
            leads.push((-1.4, y));
        }
        let right: &[f64] = if n == 5 { &[-0.95, 0.95] } else { &[-0.95, 0.0, 0.95] };
        for (i, &y) in right.iter().enumerate() {
            f.pads.push(smd(4 + i, RR, x, y, pw, ph));
            leads.push((1.4, y));
        }
    }
    fab_outline(&mut f, -0.8, -1.45, 0.8, 1.45, 0.4);
    for y in [1.56, -1.56] {
        f.shapes.push(fp_shape(Geom::Line { a: p(-0.3, y), b: p(0.3, y) }, Layer::TopSilk, SILK));
    }
    courtyard(&mut f, 0.25);
    place_texts(&mut f, 1.6, -1.6, 0.0, 0.0);
    f.models.push(model(&format!("Package_TO_SOT_SMD.3dshapes/{name}.step"), model3d::gull_wing(1.6, 2.9, 1.1, 0.05, &leads, 0.4, 0.15)));
    f
}

fn sot223() -> Footprint {
    let name = "SOT-223-3_TabPin2";
    let mut f = footprint("Package_TO_SOT_SMD", name, "SOT-223, 3 pins and a tab (pin 2)", "SOT-223 TO_SOT_SMD", MountKind::Smd);
    let mut leads = vec![];
    for i in 0..3 {
        let y = 2.3 - i as f64 * 2.3;
        f.pads.push(smd(i + 1, RR, -3.15, y, 2.0, 1.5));
        leads.push((-3.5, y));
    }
    f.pads.push(smd(2, RR, 3.15, 0.0, 2.0, 3.8));
    fab_outline(&mut f, -1.75, -3.25, 1.75, 3.25, 0.8);
    for y in [3.36, -3.36] {
        f.shapes.push(fp_shape(Geom::Line { a: p(-1.86, y), b: p(1.86, y) }, Layer::TopSilk, SILK));
    }
    courtyard(&mut f, 0.25);
    place_texts(&mut f, 3.4, -3.4, 0.0, 0.0);
    let mut b = model3d::gull_wing(3.5, 6.5, 1.7, 0.05, &leads, 0.7, 0.25);
    b.cuboid([1.75, -1.5, 0.0], [3.5, 1.5, 0.25], colors::TIN);
    f.models.push(model(&format!("Package_TO_SOT_SMD.3dshapes/{name}.step"), b));
    f
}

/// QFN with an exposed pad: `n` pins on 4 sides (counter-clockwise from the top of the left
/// side), body `l` square, `pitch`, exposed pad `ep` square numbered `n + 1`.
fn qfn(n: usize, l: f64, pitch: f64, ep: f64) -> Footprint {
    let name = format!("QFN-{n}-1EP_{l}x{l}mm_P{pitch}mm_EP{ep}x{ep}mm");
    let mut f = footprint("Package_DFN_QFN", &name, &format!("QFN, {n} pins, {l} × {l} mm, pitch {pitch} mm, exposed pad"), "QFN DFN_QFN", MountKind::Smd);
    let per = n / 4;
    let c = l / 2.0 - 0.05;
    let (len, wid) = (0.875, pitch / 2.0);
    let first = (per as f64 - 1.0) * pitch / 2.0;
    let mut terms = vec![];
    for side in 0..4 {
        for i in 0..per {
            let t = first - i as f64 * pitch;
            let (x, y, w, h) = match side {
                0 => (-c, t, len, wid),
                1 => (-t, -c, wid, len),
                2 => (c, -t, len, wid),
                _ => (t, c, wid, len),
            };
            f.pads.push(smd(side * per + i + 1, RR, x, y, w, h));
            terms.push((x, y, w.min(0.5), h.min(0.5)));
        }
    }
    f.pads.push(smd(n + 1, PadShape::Rect, 0.0, 0.0, ep, ep));
    fab_outline(&mut f, -l / 2.0, -l / 2.0, l / 2.0, l / 2.0, 0.6);
    let s = l / 2.0 + 0.11;
    let e = first + wid / 2.0 + SILK_GAP + SILK / 2.0;
    for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        f.shapes.push(fp_shape(Geom::Polyline { pts: vec![p(sx * e, sy * s), p(sx * s, sy * s), p(sx * s, sy * e)], closed: false }, Layer::TopSilk, SILK));
    }
    f.shapes.push(fp_shape(Geom::Line { a: p(-e, s), b: p(-s, s) }, Layer::TopSilk, SILK));
    courtyard(&mut f, 0.25);
    place_texts(&mut f, l / 2.0 + 0.6, -l / 2.0 - 0.6, 0.0, 0.0);
    terms.push((0.0, 0.0, ep, ep));
    f.models.push(model(&format!("Package_DFN_QFN.3dshapes/{name}.step"), model3d::no_lead(l, l, 0.9, &terms)));
    f
}

/// LQFP/TQFP: `n` pins on 4 sides, body `l` square, `pitch`.
fn qfp(kind: &str, n: usize, l: f64, pitch: f64) -> Footprint {
    let name = format!("{kind}-{n}_{l}x{l}mm_P{pitch}mm");
    let mut f = footprint("Package_QFP", &name, &format!("{kind}, {n} pins, {l} × {l} mm, pitch {pitch} mm"), &format!("{kind} QFP"), MountKind::Smd);
    let per = n / 4;
    let c = l / 2.0 + 0.6625;
    let (len, wid) = (1.475, if pitch >= 0.8 { 0.55 } else { 0.3 });
    let first = (per as f64 - 1.0) * pitch / 2.0;
    let tip = l / 2.0 + 1.0;
    let mut leads = vec![];
    for side in 0..4 {
        for i in 0..per {
            let t = first - i as f64 * pitch;
            let (x, y, w, h, lead) = match side {
                0 => (-c, t, len, wid, (-tip, t)),
                1 => (-t, -c, wid, len, (-t, -tip)),
                2 => (c, -t, len, wid, (tip, -t)),
                _ => (t, c, wid, len, (t, tip)),
            };
            f.pads.push(smd(side * per + i + 1, RR, x, y, w, h));
            leads.push(lead);
        }
    }
    fab_outline(&mut f, -l / 2.0, -l / 2.0, l / 2.0, l / 2.0, 1.0);
    let s = l / 2.0 + 0.11;
    let e = first + wid / 2.0 + SILK_GAP + SILK / 2.0;
    for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        f.shapes.push(fp_shape(Geom::Polyline { pts: vec![p(sx * e, sy * s), p(sx * s, sy * s), p(sx * s, sy * e)], closed: false }, Layer::TopSilk, SILK));
    }
    f.shapes.push(fp_shape(Geom::Polyline { pts: vec![p(-e, s), p(-s, s), p(-s, e), p(-c - len / 2.0, e)], closed: false }, Layer::TopSilk, SILK));
    courtyard(&mut f, 0.25);
    place_texts(&mut f, c + len / 2.0, -c - len / 2.0, 0.0, 0.0);
    f.models.push(model(&format!("Package_QFP.3dshapes/{name}.step"), model3d::gull_wing(l, l, 1.6, 0.1, &leads, wid * 0.8, 0.2)));
    f
}

// ---------------------------------------------------------------------------------------------
// Connectors

/// Pin header or socket: `cols` × `rows`, `pitch`; pin 1 at the origin, rows going down (−y),
/// the second column at +x.
fn pin_header(socket: bool, cols: usize, rows: usize, pitch: f64) -> Footprint {
    let kind = if socket { "PinSocket" } else { "PinHeader" };
    let lib = format!("Connector_{kind}_{pitch:.2}mm");
    let name = format!("{kind}_{cols}x{rows:02}_P{pitch:.2}mm_Vertical");
    let mut f = footprint(&lib, &name, &format!("Through hole straight {} {cols}x{rows:02}, {pitch:.2} mm pitch", if socket { "socket strip" } else { "pin header" }), &format!("Through hole {} THT {cols}x{rows:02} {pitch:.2}mm", if socket { "socket strip" } else { "pin header" }), MountKind::ThroughHole);
    let (pad, drill) = if pitch > 2.0 { (1.7, 1.0) } else { (1.0, 0.65) };
    let mut pins = vec![];
    for r in 0..rows {
        for c in 0..cols {
            let n = r * cols + c + 1;
            let (x, y) = (c as f64 * pitch, -(r as f64) * pitch);
            let shape = if n == 1 { PadShape::Rect } else { PadShape::Oval };
            f.pads.push(tht(n, shape, x, y, pad, pad, drill));
            pins.push((x, y));
        }
    }
    let h = pitch / 2.0;
    let (x0, x1, y0, y1) = (-h, (cols as f64 - 1.0) * pitch + h, -(rows as f64 - 1.0) * pitch - h, h);
    fab_outline(&mut f, x0, y0, x1, y1, if socket { 0.0 } else { h / 2.0 });
    let g = 0.11;
    silk_rect(&mut f, x0 - g, y0 - g, x1 + g, y1 + g);
    // Pin 1: a corner mark outside the outline.
    f.shapes.push(fp_shape(Geom::Polyline { pts: vec![p(x0 - g - 0.2, 0.0), p(x0 - g - 0.2, y1 + g + 0.2), p(0.0, y1 + g + 0.2)], closed: false }, Layer::TopSilk, SILK));
    courtyard(&mut f, 0.5);
    place_texts(&mut f, y1 + 0.4, y0 - 0.4, (x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let body = if socket {
        let mut b = Body::default();
        b.cuboid([x0, y0, 0.0], [x1, y1, 8.5], colors::PLASTIC);
        for &(x, y) in &pins {
            b.cuboid([x - 0.3, y - 0.3, -3.0], [x + 0.3, y + 0.3, 0.0], colors::GOLD);
        }
        b
    } else {
        model3d::pin_header(&pins, pitch, if pitch > 2.0 { 8.5 } else { 4.4 })
    };
    f.models.push(model(&format!("{lib}.3dshapes/{name}.step"), body));
    f
}

/// JST PH (2.0 mm) or XH (2.5 mm) vertical wire-to-board header, `n` pins.
fn jst(series: &str, n: usize) -> Footprint {
    let (pitch, pw, ph, drill, below, above, h, part) = match series {
        "PH" => (2.0, 1.2, 1.75, 0.75, 2.8, 1.7, 6.0, format!("B{n}B-PH-K")),
        _ => (2.5, 1.7, 2.0, 0.95, 3.4, 2.35, 7.0, format!("B{n}B-XH-A")),
    };
    let name = format!("JST_{series}_{part}_1x{n:02}_P{pitch:.2}mm_Vertical");
    let mut f = footprint("Connector_JST", &name, &format!("JST {series} series connector, {part}, {n} pins, vertical"), &format!("connector JST {series} vertical"), MountKind::ThroughHole);
    let mut pins = vec![];
    for i in 0..n {
        let x = i as f64 * pitch;
        f.pads.push(tht(i + 1, if i == 0 { RR } else { PadShape::Oval }, x, 0.0, pw, ph, drill));
        pins.push((x, 0.0));
    }
    let side = if series == "PH" { 1.95 } else { 2.45 };
    let (x0, x1, y0, y1) = (-side, (n as f64 - 1.0) * pitch + side, -below, above);
    fab_outline(&mut f, x0, y0, x1, y1, 0.0);
    silk_rect(&mut f, x0 - 0.11, y0 - 0.11, x1 + 0.11, y1 + 0.11);
    courtyard(&mut f, 0.5);
    place_texts(&mut f, y1 + 0.3, y0 - 0.3, (x0 + x1) / 2.0, (y0 + y1) / 2.0);
    f.models.push(model(&format!("Connector_JST.3dshapes/{name}.step"), model3d::housing([x0, y0], [x1, y1], h, &pins, colors::WHITE)));
    f
}

// ---------------------------------------------------------------------------------------------
// Crystals, radial and through-hole parts

fn crystal_3225() -> Footprint {
    let name = "Crystal_SMD_3225-4Pin_3.2x2.5mm";
    let mut f = footprint("Crystal", name, "SMD crystal, 3.2 × 2.5 mm, 4 pins", "crystal SMD", MountKind::Smd);
    for (n, x, y) in [(1, -1.1, -0.85), (2, 1.1, -0.85), (3, 1.1, 0.85), (4, -1.1, 0.85)] {
        f.pads.push(smd(n, RR, x, y, 1.4, 1.15));
    }
    fab_outline(&mut f, -1.6, -1.25, 1.6, 1.25, 0.0);
    f.shapes.push(fp_shape(Geom::Polyline { pts: vec![p(-2.0, 0.0), p(-2.0, -1.65), p(0.0, -1.65)], closed: false }, Layer::TopSilk, SILK));
    courtyard(&mut f, 0.25);
    place_texts(&mut f, 1.6, -1.6, 0.0, 0.0);
    let mut b = Body::default();
    b.cuboid([-1.6, -1.25, 0.0], [1.6, 1.25, 0.2], colors::CERAMIC);
    b.cuboid([-1.4, -1.05, 0.2], [1.4, 1.05, 0.8], colors::SHIELD);
    f.models.push(model(&format!("Crystal.3dshapes/{name}.step"), b));
    f
}

fn crystal_hc49() -> Footprint {
    let name = "Crystal_HC49-U_Vertical";
    let mut f = footprint("Crystal", name, "HC-49/U crystal, vertical", "crystal THT HC-49", MountKind::ThroughHole);
    f.pads.push(tht(1, PadShape::Circle, 0.0, 0.0, 1.5, 1.5, 0.8));
    f.pads.push(tht(2, PadShape::Circle, 4.88, 0.0, 1.5, 1.5, 0.8));
    fab_outline(&mut f, -3.235, -2.325, 8.115, 2.325, 0.0);
    silk_rect(&mut f, -3.35, -2.44, 8.23, 2.44);
    courtyard(&mut f, 0.5);
    place_texts(&mut f, 2.4, -2.4, 2.44, 0.0);
    let mut b = Body::default();
    b.cuboid([-3.235, -2.325, 0.0], [8.115, 2.325, 13.5], colors::ALUMINIUM);
    f.models.push(model(&format!("Crystal.3dshapes/{name}.step"), b));
    f
}

/// Radial electrolytic: can `d`, lead pitch `pitch`, height `h`.
fn cp_radial(d: f64, pitch: f64, h: f64) -> Footprint {
    let name = format!("CP_Radial_D{d:.1}mm_P{pitch:.2}mm");
    let mut f = footprint("Capacitor_THT", &name, &format!("Radial electrolytic capacitor, diameter {d} mm, pitch {pitch} mm"), "CP radial electrolytic", MountKind::ThroughHole);
    let (pad, drill) = if pitch < 3.0 { (1.6, 0.8) } else { (2.0, 1.0) };
    f.pads.push(tht(1, PadShape::Rect, 0.0, 0.0, pad, pad, drill));
    f.pads.push(tht(2, PadShape::Circle, pitch, 0.0, pad, pad, drill));
    let c = p(pitch / 2.0, 0.0);
    let r = d / 2.0;
    f.shapes.push(fp_shape(Geom::Circle { center: c, radius: mm(r) }, Layer::TopFab, 0.1));
    f.shapes.push(fp_shape(Geom::Circle { center: c, radius: mm(r + 0.12) }, Layer::TopSilk, SILK));
    // "+" by pin 1.
    let (px, py) = (pitch / 2.0 - r * 0.8, r * 0.6);
    f.shapes.push(fp_shape(Geom::Line { a: p(px - 0.5, py), b: p(px + 0.5, py) }, Layer::TopSilk, SILK));
    f.shapes.push(fp_shape(Geom::Line { a: p(px, py - 0.5), b: p(px, py + 0.5) }, Layer::TopSilk, SILK));
    f.shapes.push(fp_shape(Geom::Circle { center: c, radius: mm(r + 0.5) }, Layer::TopCourtyard, 0.05));
    place_texts(&mut f, r + 0.3, -r - 0.3, pitch / 2.0, 0.0);
    let mut b = model3d::can([pitch / 2.0, 0.0], r, h - 0.3, colors::SLEEVE);
    b.cylinder([pitch / 2.0, 0.0, h - 0.3], model3d::Axis::Z, r - 0.2, 0.3, colors::ALUMINIUM);
    for x in [0.0, pitch] {
        b.cuboid([x - 0.25, -0.25, -3.0], [x + 0.25, 0.25, 0.0], colors::TIN);
    }
    f.models.push(model(&format!("Capacitor_THT.3dshapes/{name}.step"), b));
    f
}

fn to92() -> Footprint {
    let name = "TO-92_Inline";
    let mut f = footprint("Package_TO_SOT_THT", name, "TO-92 leads in-line, narrow, oval pads, drill 0.75 mm", "TO-92 transistor", MountKind::ThroughHole);
    for i in 0..3 {
        f.pads.push(tht(i + 1, if i == 0 { PadShape::Rect } else { PadShape::Oval }, i as f64 * 1.27, 0.0, 1.05, 1.5, 0.75));
    }
    fab_outline(&mut f, -1.13, -1.6, 3.67, 2.0, 0.0);
    f.shapes.push(fp_shape(Geom::Line { a: p(-0.53, -1.85), b: p(3.07, -1.85) }, Layer::TopSilk, SILK));
    courtyard(&mut f, 0.5);
    place_texts(&mut f, 2.4, -2.4, 1.27, 0.0);
    let mut b = Body::default();
    b.cuboid([-1.13, -1.6, 0.0], [3.67, 2.0, 4.6], colors::PLASTIC);
    for i in 0..3 {
        let x = i as f64 * 1.27;
        b.cuboid([x - 0.2, -0.2, -3.0], [x + 0.2, 0.2, 0.0], colors::TIN);
    }
    f.models.push(model(&format!("Package_TO_SOT_THT.3dshapes/{name}.step"), b));
    f
}

fn mounting_hole(d: f64, screw: &str, pad: bool) -> Footprint {
    let name = if pad { format!("MountingHole_{d:.1}mm_{screw}_Pad") } else { format!("MountingHole_{d:.1}mm_{screw}") };
    let mut f = footprint("MountingHole", &name, &format!("Mounting hole, {screw}{}", if pad { ", plated with a pad" } else { "" }), "mounting hole", MountKind::Unspecified);
    f.attrs.exclude_from_bom = true;
    let ring = d * 2.0;
    if pad {
        f.pads.push(tht(1, PadShape::Circle, 0.0, 0.0, ring, ring, d));
    } else {
        let mut h = tht("", PadShape::Circle, 0.0, 0.0, d, d, d);
        h.kind = PadKind::NonPlated;
        h.layers = LayerSet::of(&[Layer::TopMask, Layer::BottomMask]);
        f.pads.push(h);
    }
    f.shapes.push(fp_shape(Geom::Circle { center: Pt::ZERO, radius: mm(ring / 2.0) }, Layer::Comments, 0.15));
    f.shapes.push(fp_shape(Geom::Circle { center: Pt::ZERO, radius: mm(ring / 2.0 + 0.25) }, Layer::TopCourtyard, 0.05));
    place_texts(&mut f, ring / 2.0 + 0.3, -ring / 2.0 - 0.3, 0.0, 0.0);
    f
}

fn test_point(d: f64, drill: Option<f64>) -> Footprint {
    let name = match drill {
        Some(dr) => format!("TestPoint_THTPad_D{d:.1}mm_Drill{dr:.1}mm"),
        None => format!("TestPoint_Pad_D{d:.1}mm"),
    };
    let mut f = footprint("TestPoint", &name, &format!("Test point, {d} mm pad"), "test point", if drill.is_some() { MountKind::ThroughHole } else { MountKind::Smd });
    f.attrs.exclude_from_bom = true;
    f.pads.push(match drill {
        Some(dr) => tht(1, PadShape::Circle, 0.0, 0.0, d, d, dr),
        None => smd(1, PadShape::Circle, 0.0, 0.0, d, d),
    });
    f.shapes.push(fp_shape(Geom::Circle { center: Pt::ZERO, radius: mm(d / 2.0 + 0.2) }, Layer::TopSilk, SILK));
    f.shapes.push(fp_shape(Geom::Circle { center: Pt::ZERO, radius: mm(d / 2.0 + 0.5) }, Layer::TopCourtyard, 0.05));
    place_texts(&mut f, d / 2.0 + 0.3, -d / 2.0 - 0.3, 0.0, 0.0);
    f
}

fn push_6mm() -> Footprint {
    let name = "SW_PUSH_6mm";
    let mut f = footprint("Button_Switch_THT", name, "Tactile push button, 6 × 6 mm, through hole", "tact sw push 6mm", MountKind::ThroughHole);
    for (n, x, y) in [(1, 0.0, 0.0), (2, 6.5, 0.0), (1, 0.0, -4.5), (2, 6.5, -4.5)] {
        f.pads.push(tht(n, PadShape::Circle, x, y, 2.0, 2.0, 1.1));
    }
    fab_outline(&mut f, 0.25, -5.25, 6.25, 0.75, 0.0);
    silk_rect(&mut f, 0.0, -5.5, 6.5, 1.0);
    courtyard(&mut f, 0.25);
    place_texts(&mut f, 1.5, -6.0, 3.25, -2.25);
    let mut b = Body::default();
    b.cuboid([0.25, -5.25, 0.0], [6.25, 0.75, 3.5], colors::PLASTIC);
    b.cylinder([3.25, -2.25, 3.5], model3d::Axis::Z, 1.75, 1.5, colors::PLASTIC);
    f.models.push(model(&format!("Button_Switch_THT.3dshapes/{name}.step"), b));
    f
}

fn ws2812b() -> Footprint {
    let name = "LED_WS2812B_PLCC4_5.0x5.0mm_P3.2mm";
    let mut f = footprint("LED_SMD", name, "5.0 × 5.0 mm addressable RGB LED (WS2812B)", "LED RGB NeoPixel", MountKind::Smd);
    for (n, x, y) in [(1, 2.45, 1.6), (2, 2.45, -1.6), (3, -2.45, -1.6), (4, -2.45, 1.6)] {
        f.pads.push(smd(n, PadShape::Rect, x, y, 1.6, 1.0));
    }
    fab_outline(&mut f, -2.5, -2.5, 2.5, 2.5, 1.0);
    for y in [2.75, -2.75] {
        f.shapes.push(fp_shape(Geom::Line { a: p(-3.5, y), b: p(3.5, y) }, Layer::TopSilk, SILK));
    }
    f.shapes.push(fp_shape(Geom::Line { a: p(3.5, 2.75), b: p(3.5, 1.0) }, Layer::TopSilk, SILK));
    courtyard(&mut f, 0.25);
    place_texts(&mut f, 2.8, -2.8, 0.0, 0.0);
    let mut b = Body::default();
    b.cuboid([-2.5, -2.5, 0.0], [2.5, 2.5, 1.6], colors::WHITE);
    b.cylinder([0.0, 0.0, 1.6], model3d::Axis::Z, 1.8, 0.02, colors::LED_RED);
    f.models.push(model(&format!("LED_SMD.3dshapes/{name}.step"), b));
    f
}

// ---------------------------------------------------------------------------------------------
// The libraries

fn lib(name: &str, desc: &str, fps: Vec<Footprint>) -> Library {
    let mut l = Library::new(name, Scope::Global);
    l.description = desc.into();
    fps.into_iter().for_each(|f| l.put_footprint(f));
    l
}

pub(crate) fn footprint_libraries() -> Vec<Library> {
    let mut out = vec![
        lib(
            "Resistor_THT",
            "Through-hole resistors",
            vec![
                axial_tht("Resistor_THT", "R_Axial_DIN0204_L3.6mm_D1.6mm_P7.62mm_Horizontal", 7.62, 3.6, 1.6, 0.8, 1.6),
                axial_tht("Resistor_THT", "R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal", 10.16, 6.3, 2.5, 0.8, 1.6),
                axial_tht("Resistor_THT", "R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal", 12.7, 9.0, 3.2, 0.8, 1.6),
            ],
        ),
        lib("Diode_THT", "Through-hole diodes", vec![axial_tht("Diode_THT", "D_DO-41_SOD81_P10.16mm_Horizontal", 10.16, 5.2, 2.7, 1.1, 2.2), axial_tht("Diode_THT", "D_DO-35_SOD27_P7.62mm_Horizontal", 7.62, 4.0, 2.0, 0.8, 1.6)]),
        lib("LED_THT", "Through-hole LEDs", vec![radial_led_tht(3.0), radial_led_tht(5.0)]),
        lib("Battery", "Battery holders", vec![coin_holder()]),
        lib("Resistor_SMD", "Surface-mount resistors", chips("Resistor_SMD", "R")),
        lib("Capacitor_SMD", "Surface-mount capacitors", chips("Capacitor_SMD", "C")),
        lib("Inductor_SMD", "Surface-mount inductors", chips("Inductor_SMD", "L")),
    ];
    let mut leds = chips("LED_SMD", "LED");
    leds.push(ws2812b());
    out.push(lib("LED_SMD", "Surface-mount LEDs", leds));
    out.push(lib(
        "Diode_SMD",
        "Surface-mount diodes",
        vec![diode_smd("D_SOD-123", (2.7, 1.6, 1.15), 3.27, (0.91, 1.22)), diode_smd("D_SOD-323", (1.7, 1.25, 0.95), 2.1, (0.6, 0.45)), diode_smd("D_SMA", (4.3, 2.6, 2.1), 4.0, (2.5, 1.7)), diode_smd("D_SMB", (4.3, 3.6, 2.3), 4.3, (2.5, 2.3))],
    ));
    out.push(lib("Package_TO_SOT_SMD", "SOT and TO surface-mount packages", vec![sot23("SOT-23", 3), sot23("SOT-23-5", 5), sot23("SOT-23-6", 6), sot223()]));
    out.push(lib("Package_TO_SOT_THT", "TO through-hole packages", vec![to92()]));
    let mut so = vec![];
    for (n, len) in [(8, 4.9), (14, 8.7), (16, 9.9)] {
        so.push(dual_row("Package_SO", &format!("SOIC-{n}_3.9x{len}mm_P1.27mm"), &format!("SOIC, {n} pins, 3.9 × {len} mm body"), n, 1.27, 2.475, 1.95, 0.6, 3.9, len, 1.75, 6.0));
    }
    for (n, len) in [(8, 3.0), (14, 5.0), (16, 5.0), (20, 6.5), (24, 7.8)] {
        so.push(dual_row("Package_SO", &format!("TSSOP-{n}_4.4x{len}mm_P0.65mm"), &format!("TSSOP, {n} pins, 4.4 × {len} mm body"), n, 0.65, 2.8625, 1.475, 0.4, 4.4, len, 1.2, 6.4));
    }
    so.push(dual_row("Package_SO", "MSOP-8_3x3mm_P0.65mm", "MSOP, 8 pins, 3 × 3 mm body", 8, 0.65, 2.2, 1.45, 0.42, 3.0, 3.0, 1.1, 4.9));
    out.push(lib("Package_SO", "Small-outline IC packages", so));
    out.push(lib("Package_DFN_QFN", "No-lead packages", vec![qfn(16, 3.0, 0.5, 1.7), qfn(20, 4.0, 0.5, 2.5), qfn(24, 4.0, 0.5, 2.6), qfn(32, 5.0, 0.5, 3.45), qfn(48, 7.0, 0.5, 5.6)]));
    out.push(lib("Package_QFP", "Quad flat packages", vec![qfp("LQFP", 32, 7.0, 0.8), qfp("LQFP", 48, 7.0, 0.5), qfp("LQFP", 64, 10.0, 0.5), qfp("LQFP", 100, 14.0, 0.5), qfp("TQFP", 44, 10.0, 0.8)]));
    for (socket, pitch) in [(false, 2.54), (true, 2.54), (false, 1.27)] {
        let mut fps = vec![];
        for cols in 1..=2 {
            for rows in 1..=40 {
                fps.push(pin_header(socket, cols, rows, pitch));
            }
        }
        let kind = if socket { "PinSocket" } else { "PinHeader" };
        out.push(lib(&format!("Connector_{kind}_{pitch:.2}mm"), &format!("{} strips, {pitch:.2} mm pitch", if socket { "Socket" } else { "Pin header" }), fps));
    }
    let mut jsts = vec![];
    for n in 2..=12 {
        jsts.push(jst("PH", n));
        jsts.push(jst("XH", n));
    }
    out.push(lib("Connector_JST", "JST wire-to-board connectors", jsts));
    out.push(lib("Crystal", "Crystals", vec![crystal_3225(), crystal_hc49()]));
    out.push(lib("Capacitor_THT", "Through-hole capacitors", vec![cp_radial(5.0, 2.0, 11.0), cp_radial(6.3, 2.5, 11.0), cp_radial(8.0, 3.5, 11.5), cp_radial(10.0, 5.0, 16.0)]));
    out.push(lib("MountingHole", "Mounting holes", vec![mounting_hole(2.2, "M2", false), mounting_hole(2.2, "M2", true), mounting_hole(3.2, "M3", false), mounting_hole(3.2, "M3", true)]));
    out.push(lib("TestPoint", "Test points", vec![test_point(1.0, None), test_point(1.5, None), test_point(1.5, Some(0.7))]));
    out.push(lib("Button_Switch_THT", "Through-hole buttons and switches", vec![push_6mm()]));
    out
}
