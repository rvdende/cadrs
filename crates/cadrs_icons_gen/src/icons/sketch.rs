//! Sketch tools (kind Sketch): flat 2D geometry on the sketch plane.
//!
//! Geometry is INK at 1.5. The points the user clicks, in order, are ACCENT dots of one size (DOT),
//! so every variant explains its own input method. Construction geometry is dashed and thinner.
//! Closed shapes may carry a light TOP fill.

use crate::common::*;
use Kind::Sketch;
use std::f64::consts::PI;

pub fn icons() -> Vec<Def> {
    vec![
        def("line", Sketch, "Line", line_tool),
        def("midpoint-line", Sketch, "Midpoint line", midpoint_line),
        def(
            "corner-rectangle",
            Sketch,
            "Corner rectangle",
            corner_rectangle,
        ),
        def(
            "center-rectangle",
            Sketch,
            "Center rectangle",
            center_rectangle,
        ),
        def(
            "aligned-rectangle",
            Sketch,
            "Aligned rectangle",
            aligned_rectangle,
        ),
        def("center-circle", Sketch, "Center circle", center_circle),
        def(
            "three-point-circle",
            Sketch,
            "3 point circle",
            three_point_circle,
        ),
        def("ellipse", Sketch, "Ellipse", ellipse_tool),
        def("three-point-arc", Sketch, "3 point arc", three_point_arc),
        def("tangent-arc", Sketch, "Tangent arc", tangent_arc),
        def("center-arc", Sketch, "Center point arc", center_arc),
        def(
            "inscribed-polygon",
            Sketch,
            "Inscribed polygon",
            inscribed_polygon,
        ),
        def(
            "circumscribed-polygon",
            Sketch,
            "Circumscribed polygon",
            circumscribed_polygon,
        ),
        def("spline", Sketch, "Spline", spline),
        def("point", Sketch, "Point", point),
        def("slot", Sketch, "Slot", slot),
        def("text", Sketch, "Text", text),
        def("construction", Sketch, "Construction", construction),
        def("image", Sketch, "Image", image),
        def("use", Sketch, "Use", use_),
        def("intersection", Sketch, "Intersection", intersection),
        def("sketch-fillet", Sketch, "Sketch fillet", sketch_fillet),
        def("sketch-chamfer", Sketch, "Sketch chamfer", sketch_chamfer),
        def("trim", Sketch, "Trim", trim),
        def("extend", Sketch, "Extend", extend),
        def("sketch-split", Sketch, "Split", sketch_split),
        def("offset", Sketch, "Offset", offset),
        def("mirror", Sketch, "Mirror", mirror),
        def("sketch-pattern", Sketch, "Sketch pattern", sketch_pattern),
        def("dxf-import", Sketch, "Import DXF", dxf_import),
        def("dimension", Sketch, "Dimension", dimension),
        def("diagnostics", Sketch, "Sketch diagnostics", diagnostics),
    ]
}

const DOT: f64 = 1.45; // the one accent dot radius for the whole family
const CON: &str = r#"stroke-width="1" stroke-dasharray="1.2 2.6""#; // construction / reference geometry: dash(1)
const ACC: &str = r##"stroke="#2563eb""##;
const FILL: &str = SOFT; // light region fill for closed shapes, always with REGION
const REGION: &str = r#"fill-opacity="0.45""#;

fn dot(x: f64, y: f64) -> String {
    circle(x, y, DOT, ACCENT, r#"stroke="none""#)
}

fn dots(ps: &[P]) -> Vec<String> {
    ps.iter().map(|&(x, y)| dot(x, y)).collect()
}

fn on(c: P, r: f64, deg: f64) -> P {
    let a = rad(deg);
    (c.0 + r * a.cos(), c.1 + r * a.sin())
}

/// SVG path data for a circular arc from angle a0 to a1 (degrees, clockwise on screen when
/// a1 > a0).
fn arc_d(c: P, r: f64, a0: f64, a1: f64) -> String {
    let ((x0, y0), (x1, y1)) = (on(c, r, a0), on(c, r, a1));
    let large = if (a1 - a0).abs() > 180.0 { 1 } else { 0 };
    let sweep = if a1 > a0 { 1 } else { 0 };
    format!(
        "M{} {} A{} {} 0 {large} {sweep} {} {}",
        f(x0),
        f(y0),
        f(r),
        f(r),
        f(x1),
        f(y1)
    )
}

fn ngon(c: P, r: f64, n: usize, start: f64) -> Vec<P> {
    (0..n)
        .map(|i| on(c, r, start + (360 * i) as f64 / n as f64))
        .collect()
}

/// A Catmull-Rom spline through the points, sampled as a polyline.
fn catmull(ps: &[P], n: usize) -> Vec<P> {
    let ext = cat(&[&[ps[0]], ps, &[ps[ps.len() - 1]]]);
    let mut out: Vec<P> = Vec::new();
    for i in 1..ext.len() - 2 {
        let (p0, p1, p2, p3) = (ext[i - 1], ext[i], ext[i + 1], ext[i + 2]);
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        let seg = sample_cubic(p1, c1, c2, p2, n);
        out.extend_from_slice(if out.is_empty() { &seg } else { &seg[1..] });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Lines and rectangles

fn line_tool() -> Vec<String> {
    let (a, b) = ((5.0, 19.0), (19.0, 5.0));
    els![seg(a, b, ""), dots(&[a, b])]
}

fn midpoint_line() -> Vec<String> {
    let (a, b, m) = ((4.0, 20.0), (20.0, 4.0), (12.0, 12.0));
    els![seg(a, b, ""), dots(&[m, b])]
}

fn corner_rectangle() -> Vec<String> {
    els![
        rect(4.0, 6.0, 16.0, 12.0, 0.0, FILL, REGION),
        dots(&[(4.0, 6.0), (20.0, 18.0)])
    ]
}

fn center_rectangle() -> Vec<String> {
    els![
        rect(4.0, 6.0, 16.0, 12.0, 0.0, FILL, REGION),
        line(12.0, 12.0, 20.0, 18.0, CON),
        dots(&[(12.0, 12.0), (20.0, 18.0)]),
    ]
}

fn aligned_rectangle() -> Vec<String> {
    // Edge first (two clicks along the base), then the width (third click on the far side).
    let ang = rad(-30.0);
    let u = (ang.cos(), ang.sin());
    let v = (u.1, -u.0); // perpendicular, pointing up-left
    let (l, w) = (14.5, 7.5);
    let q = [
        (0.0, 0.0),
        (u.0 * l, u.1 * l),
        (u.0 * l + v.0 * w, u.1 * l + v.1 * w),
        (v.0 * w, v.1 * w),
    ];
    let min = |k: fn(&P) -> f64| q.iter().map(k).fold(f64::INFINITY, f64::min);
    let max = |k: fn(&P) -> f64| q.iter().map(k).fold(f64::NEG_INFINITY, f64::max);
    let cx = (min(|p| p.0) + max(|p| p.0)) / 2.0;
    let cy = (min(|p| p.1) + max(|p| p.1)) / 2.0;
    let [p0, p1, p2, p3] = q.map(|(x, y)| (x - cx + 12.0, y - cy + 12.0));
    let mid = ((p2.0 + p3.0) / 2.0, (p2.1 + p3.1) / 2.0);
    els![poly(&[p0, p1, p2, p3], FILL, REGION), dots(&[p0, p1, mid])]
}

// ---------------------------------------------------------------------------------------------
// Circles and ellipse

fn center_circle() -> Vec<String> {
    let (c, r) = ((12.0, 12.0), 8.5);
    let p = on(c, r, -40.0);
    els![
        circle(c.0, c.1, r, FILL, REGION),
        seg(c, p, CON),
        dots(&[c, p])
    ]
}

fn three_point_circle() -> Vec<String> {
    let (c, r) = ((12.0, 12.0), 8.5);
    els![
        circle(c.0, c.1, r, FILL, REGION),
        dots(&[160.0, 275.0, 25.0].map(|a| on(c, r, a)))
    ]
}

fn ellipse_tool() -> Vec<String> {
    let (c, rx, ry, rot) = ((12.0, 12.0), 9.3, 5.6, -30.0);
    let ma = on(c, rx, rot);
    let mi = on(c, ry, rot - 90.0);
    els![
        ellipse_rot(c.0, c.1, rx, ry, FILL, REGION, rot),
        seg(c, ma, CON),
        seg(c, mi, CON),
        dots(&[c, ma, mi])
    ]
}

// ---------------------------------------------------------------------------------------------
// Arcs

fn three_point_arc() -> Vec<String> {
    let (c, r) = ((12.0, 17.5), 9.0);
    let (a0, a1) = (195.0, 345.0);
    els![
        path(&arc_d(c, r, a0, a1), "", ""),
        dots(&[on(c, r, a0), on(c, r, a1), on(c, r, 250.0)])
    ]
}

fn tangent_arc() -> Vec<String> {
    // An existing line runs right into (10.5, 19); the arc leaves it tangentially, turning up.
    let (c, r) = ((10.5, 11.0), 8.0);
    let (a0, a1) = (90.0, -60.0);
    els![
        line(3.0, 19.0, 10.5, 19.0, r#"stroke-opacity="0.5""#),
        path(&arc_d(c, r, a0, a1), "", ""),
        dots(&[on(c, r, a0), on(c, r, a1)]),
    ]
}

fn center_arc() -> Vec<String> {
    let (c, r) = ((5.5, 18.5), 13.5);
    let (a0, a1) = (-80.0, 0.0);
    let (s, e) = (on(c, r, a0), on(c, r, a1));
    els![
        seg(c, s, CON),
        seg(c, e, CON),
        path(&arc_d(c, r, a0, a1), "", ""),
        dots(&[c, s, e])
    ]
}

// ---------------------------------------------------------------------------------------------
// Polygons, spline, point, slot, text

const INNER: &str = r#"fill-opacity="0.6""#; // tint of the enclosed shape in the two polygon icons

fn inscribed_polygon() -> Vec<String> {
    // The two polygon variants share one rule: the enclosed shape is tinted, the enclosing one is
    // only an outline, so which encloses which reads from the fill in both themes. Here a tinted
    // pentagon has its corners on the dashed construction circle.
    let (c, r) = ((12.0, 12.0), 9.0);
    let vs = ngon(c, r, 5, -90.0);
    els![
        circle(c.0, c.1, r, "", CON),
        poly(&vs, MID, INNER),
        dots(&[c, vs[1]])
    ]
}

fn circumscribed_polygon() -> Vec<String> {
    // A tinted construction disc inside a pentagon, touching each side's midpoint. Centred
    // optically, since a pentagon reaches further above its centre (R) than below it (apothem).
    let ap = 7.0;
    let r = ap / (PI / 5.0).cos();
    let c = (12.0, 12.0 + (r - ap) / 2.0);
    let vs = ngon(c, r, 5, -90.0);
    els![
        circle(c.0, c.1, ap - 0.3, MID, &format!("{INNER} {CON}")),
        poly(&vs, "", ""),
        dots(&[c, on(c, ap, 90.0)])
    ]
}

fn spline() -> Vec<String> {
    let ps = [(3.8, 17.5), (9.0, 7.5), (15.0, 16.5), (20.2, 6.5)];
    els![polyline(&catmull(&ps, 12), ""), dots(&ps)]
}

fn point() -> Vec<String> {
    let c = (12.0, 12.0);
    let ticks: Vec<String> = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)]
        .iter()
        .map(|&(dx, dy)| {
            line(
                c.0 + dx * 3.6,
                c.1 + dy * 3.6,
                c.0 + dx * 8.5,
                c.1 + dy * 8.5,
                "",
            )
        })
        .collect();
    els![ticks, dot(c.0, c.1)]
}

fn slot() -> Vec<String> {
    let (a, b, r): (P, P, f64) = ((8.0, 16.0), (16.0, 8.0), 4.6);
    let ang = deg((b.1 - a.1).atan2(b.0 - a.0));
    let (p1, p2) = (on(a, r, ang + 90.0), on(b, r, ang + 90.0));
    let (p3, p4) = (on(b, r, ang - 90.0), on(a, r, ang - 90.0));
    let dd = format!(
        "M{} {} L{} {} A{} {} 0 0 0 {} {} L{} {} A{} {} 0 0 0 {} {} Z",
        f(p1.0),
        f(p1.1),
        f(p2.0),
        f(p2.1),
        f(r),
        f(r),
        f(p3.0),
        f(p3.1),
        f(p4.0),
        f(p4.1),
        f(r),
        f(r),
        f(p1.0),
        f(p1.1)
    );
    els![path(&dd, FILL, REGION), seg(a, b, CON), dots(&[a, b])]
}

fn text() -> Vec<String> {
    // The text box is placed by two opposite corners (like corner-rectangle); a T sits inside.
    let (x0, y0, x1, y1) = (3.5, 3.5, 20.5, 20.5);
    let t = [
        (7.5, 7.0),
        (16.5, 7.0),
        (16.5, 10.3),
        (13.7, 10.3),
        (13.7, 17.5),
        (10.3, 17.5),
        (10.3, 10.3),
        (7.5, 10.3),
    ];
    els![
        rect(x0, y0, x1 - x0, y1 - y0, 0.0, "", CON),
        poly(&t, FILL, REGION),
        dots(&[(x0, y0), (x1, y1)])
    ]
}

fn construction() -> Vec<String> {
    els![
        circle(12.0, 12.0, 8.5, "", &dash(1.5)),
        line(5.5, 18.5, 18.5, 5.5, &format!("{ACC} {}", dash(1.5)))
    ]
}

fn image() -> Vec<String> {
    let (x0, y0, x1, y1) = (3.5, 5.0, 20.5, 19.0);
    els![
        rect(x0, y0, x1 - x0, y1 - y0, 0.0, FILL, REGION),
        poly(
            &[
                (3.5, 19.0),
                (9.5, 11.5),
                (13.5, 16.0),
                (15.5, 13.8),
                (20.5, 19.0)
            ],
            SOFT,
            r#"stroke-width="1.2""#
        ),
        circle(15.5, 9.2, 1.7, "", r#"stroke-width="1.2""#),
        dots(&[(x0, y1), (x1, y0)]),
    ]
}

// ---------------------------------------------------------------------------------------------
// Model references

fn use_() -> Vec<String> {
    // A model body (muted, flat silhouette) whose picked edges become sketch geometry (accent).
    let (c, r) = ((12.0, 12.5), 9.0);
    let hexv = ngon(c, r, 6, -90.0);
    let [_top, rt, br, bot, bl, lt] = hexv[..] else {
        unreachable!()
    };
    let muted = r#"stroke-width="1" stroke-opacity="0.45""#;
    els![
        poly(&hexv, FILL, &format!("{REGION} {muted}")),
        polyline(&[lt, c, rt], muted),
        seg(c, bot, muted),
        polyline(&[bl, bot, br], ACC),
        dots(&[bl, bot, br]),
    ]
}

fn intersection() -> Vec<String> {
    let (c, r): (P, f64) = ((12.0, 13.0), 7.5);
    let y = 11.0;
    let dx = (r * r - (y - c.1).powf(2.0)).sqrt();
    els![
        circle(
            c.0,
            c.1,
            r,
            FILL,
            &format!(r#"{REGION} stroke-width="1" stroke-opacity="0.5""#)
        ),
        line(2.5, y, 21.5, y, CON),
        line(c.0 - dx, y, c.0 + dx, y, ACC),
        dots(&[(c.0 - dx, y), (c.0 + dx, y)]),
    ]
}

// ---------------------------------------------------------------------------------------------
// Modify

fn sketch_fillet() -> Vec<String> {
    els![
        polyline(&[(5.0, 13.0), (5.0, 5.0), (13.0, 5.0)], CON),
        line(5.0, 20.0, 5.0, 13.0, ""),
        line(13.0, 5.0, 20.0, 5.0, ""),
        path("M5 13 A8 8 0 0 1 13 5", "", ACC),
    ]
}

fn sketch_chamfer() -> Vec<String> {
    els![
        polyline(&[(5.0, 13.0), (5.0, 5.0), (13.0, 5.0)], CON),
        line(5.0, 20.0, 5.0, 13.0, ""),
        line(13.0, 5.0, 20.0, 5.0, ""),
        line(5.0, 13.0, 13.0, 5.0, ACC),
    ]
}

fn trim() -> Vec<String> {
    els![
        line(8.0, 4.0, 8.0, 20.0, ""),
        line(16.0, 4.0, 16.0, 20.0, ""),
        line(3.5, 12.0, 8.0, 12.0, ""),
        line(16.0, 12.0, 20.5, 12.0, ""),
        line(8.0, 12.0, 16.0, 12.0, &format!("{ACC} {}", dash(1.5))),
    ]
}

fn extend() -> Vec<String> {
    els![
        line(19.0, 4.0, 19.0, 20.0, ""),
        line(3.5, 12.0, 10.0, 12.0, ""),
        arrow(10.0, 12.0, 18.5, 12.0, 3.4, 3.2, ACCENT, &dash(1.5)),
    ]
}

fn sketch_split() -> Vec<String> {
    let (c, r) = ((12.0, 20.0), 12.0);
    let p = on(c, r, 270.0);
    els![
        path(&arc_d(c, r, 196.0, 248.0), "", ""),
        path(&arc_d(c, r, 292.0, 344.0), "", ""),
        dot(p.0, p.1)
    ]
}

fn offset() -> Vec<String> {
    els![
        rect(3.5, 3.5, 17.0, 17.0, 5.0, "", ""),
        rect(8.0, 8.0, 8.0, 8.0, 1.5, "", ACC)
    ]
}

fn mirror() -> Vec<String> {
    let left = [(9.5, 5.0), (9.5, 19.0), (3.5, 19.0)];
    let right = left.map(|(x, y)| (24.0 - x, y));
    els![
        poly(&left, FILL, REGION),
        poly(&right, FILL, REGION),
        line(12.0, 2.5, 12.0, 21.5, &format!("{ACC} {}", dash_dot(1.2)))
    ]
}

fn sketch_pattern() -> Vec<String> {
    let r = 3.3;
    [(7.0, 7.0), (17.0, 7.0), (7.0, 17.0), (17.0, 17.0)]
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            circle(
                x,
                y,
                r,
                FILL,
                &if i == 0 {
                    format!("{REGION} {ACC}")
                } else {
                    REGION.to_owned()
                },
            )
        })
        .collect()
}

fn dxf_import() -> Vec<String> {
    els![
        path("M3.5 2.5 H10.5 L14 6 V15 H3.5 Z", FILL, REGION),
        path("M10.5 2.5 V6 H14", "", r#"stroke-width="1""#),
        circle(8.2, 10.0, 2.1, "", r#"stroke-width="1""#),
        line(2.5, 20.5, 21.5, 20.5, CON),
        curved_arrow(
            &arc_points(11.0, 17.5, 7.5, 12.0, -80.0, -5.0),
            3.2,
            3.0,
            ACCENT,
            ""
        ),
    ]
}

fn dimension() -> Vec<String> {
    let (y, yd) = (18.5, 8.0);
    let thin = r#"stroke-width="1""#;
    els![
        line(4.0, y, 20.0, y, ""),
        line(4.0, y - 2.5, 4.0, yd - 3.0, thin),
        line(20.0, y - 2.5, 20.0, yd - 3.0, thin),
        arrow(12.0, yd, 4.2, yd, 3.0, 3.0, INK, thin),
        arrow(12.0, yd, 19.8, yd, 3.0, 3.0, INK, thin),
        dots(&[(4.0, y), (20.0, y)]),
    ]
}

fn diagnostics() -> Vec<String> {
    let v = (14.5, 13.5);
    els![
        poly(&[(3.5, 19.0), (8.5, 4.5), v], FILL, REGION),
        circle(v.0, v.1, 4.6, "", ""),
        line(
            v.0 + 3.3,
            v.1 + 3.3,
            21.5,
            20.5 + 1.0,
            r#"stroke-width="2""#
        ),
        dot(v.0, v.1),
    ]
}
