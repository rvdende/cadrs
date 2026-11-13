//! Shared palette, geometry helpers and the icon definition type.
//!
//! Every icon is a function returning its SVG body as a list of element strings, listed with
//! [`Def`] in its module's `icons()`. `generate.rs` wraps the body in the `<svg>` root for its kind,
//! validates it, and writes the light and dark files.
//!
//! Conventions (all icons):
//! - 24x24 view box. Keep drawing inside 1.5..22.5 so strokes are not clipped.
//! - Coordinates are written with [`f`] so files stay small and diffs stay stable.
//! - Colours come only from the palette constants below. The dark variant is made by swapping them.
//!
//! Kinds:
//! - Solid:  isometric CAD geometry. Outline INK at the root stroke width (1.1). Faces lit from
//!   above-right: TOP on faces pointing up, MID on right-facing sides, SHADE on left-facing
//!   sides, SOFT on bevels and secondary faces. ACCENT marks what the tool acts on (the edge
//!   being filleted, the direction of an extrude, the moving part of a mate). Use it
//!   sparingly: one idea per icon.
//! - Sketch: flat 2D sketch geometry. INK lines at 1.5. ACCENT for the points or entity the user
//!   picks (the three points of a 3-point arc, the centre of a centre rectangle). TOP or
//!   SOFT fills are allowed for closed regions; no isometric shading.
//! - Glyph:  constraint and viewport markers shown at 10-16 px. Stroke 2, bold simple shapes, no
//!   detail smaller than 2 units.
//! - Line:   general UI icons (undo, folder, search...). Stroke 1.75, round caps and joins,
//!   optically centred, consistent with each other.
//!
//! Glyph and line icons are INK plus a little ACCENT. Apps tint the INK (Icon::tinted) and keep the
//! accent, so the accent must never carry the icon's meaning alone. Accent the modifier, not the
//! object: the plus in file-new, the arrow in upload, the slash in hidden, the contact point in
//! tangent, the shared point in coincident. One accent element (or one group) per icon. Icons that
//! are a single mark (chevrons, carets, close, check, menu, plus, plain arrows) stay INK only, and so
//! do the layered viewport markers (manipulator arrow and halo, centre-of-mass layers), which the app
//! colours per state.
//!
//! Fill and attribute arguments are strings, with `""` for none: `fill` is a palette colour or
//! `""` (the root default, no fill), and `extra` is raw attributes such as
//! `stroke-dasharray="1.4 1.2"` or `stroke="none"`.

use std::f64::consts::PI;

// Palette slots, light theme. They must match Palette::LIGHT in src/palette.rs (a test checks).
// The four face greys are neutral tints of INK: faces are context, the accent is what the tool
// acts on.
pub const INK: &str = "#262626";
pub const TOP: &str = "#ffffff";
pub const SOFT: &str = "#d4d4d4";
pub const MID: &str = "#a4a4a4";
pub const SHADE: &str = "#747474";
pub const ACCENT: &str = "#2563eb";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Solid,
    Sketch,
    Glyph,
    Line,
}

impl Kind {
    /// The root stroke width.
    pub fn stroke(self) -> &'static str {
        match self {
            Kind::Solid => "1.1",
            Kind::Sketch => "1.5",
            Kind::Glyph => "2",
            Kind::Line => "1.75",
        }
    }

    /// The colours icons of this kind may use.
    pub fn colours(self) -> &'static [&'static str] {
        match self {
            Kind::Solid | Kind::Sketch => &[INK, TOP, SOFT, MID, SHADE, ACCENT],
            Kind::Glyph | Kind::Line => &[INK, ACCENT],
        }
    }

    /// The variant name of cadrs_icons::Kind.
    pub fn rust(self) -> &'static str {
        match self {
            Kind::Solid => "Solid",
            Kind::Sketch => "Sketch",
            Kind::Glyph => "Glyph",
            Kind::Line => "Line",
        }
    }
}

/// One icon: its file stem, kind, title and drawing function.
pub struct Def {
    pub name: &'static str,
    pub kind: Kind,
    pub title: &'static str,
    pub draw: fn() -> Vec<String>,
}

pub const fn def(
    name: &'static str,
    kind: Kind,
    title: &'static str,
    draw: fn() -> Vec<String>,
) -> Def {
    Def {
        name,
        kind,
        title,
        draw,
    }
}

/// A screen point.
pub type P = (f64, f64);

// ---------------------------------------------------------------------------------------------
// Bodies

/// Things that can go in an icon body: an element, a list of them, or nothing.
pub trait Emit {
    fn emit(self, out: &mut Vec<String>);
}

impl Emit for String {
    fn emit(self, out: &mut Vec<String>) {
        out.push(self);
    }
}

impl Emit for &str {
    fn emit(self, out: &mut Vec<String>) {
        out.push(self.to_owned());
    }
}

impl<T: Emit> Emit for Vec<T> {
    fn emit(self, out: &mut Vec<String>) {
        for e in self {
            e.emit(out);
        }
    }
}

impl<T: Emit> Emit for Option<T> {
    fn emit(self, out: &mut Vec<String>) {
        if let Some(e) = self {
            e.emit(out);
        }
    }
}

/// A list of elements from any mix of elements, lists and options.
#[macro_export]
macro_rules! els {
    ($($e:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut out: Vec<String> = Vec::new();
        $( $crate::common::Emit::emit($e, &mut out); )*
        out
    }};
}

/// Elements joined into one string, as the body joins them.
pub fn join(els: Vec<String>) -> String {
    els.join("\n  ")
}

// ---------------------------------------------------------------------------------------------
// Formatting

/// A coordinate with at most two decimals and no trailing zeros.
pub fn f(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

pub fn pts(ps: &[P]) -> String {
    ps.iter()
        .map(|&(x, y)| format!("{},{}", f(x), f(y)))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn attrs(fill: &str, extra: &str) -> String {
    let mut a = if fill.is_empty() {
        String::new()
    } else {
        format!(r#" fill="{fill}""#)
    };
    let extra = extra.trim();
    if !extra.is_empty() {
        a.push(' ');
        a.push_str(extra);
    }
    a
}

// ---------------------------------------------------------------------------------------------
// Primitives

pub fn poly(ps: &[P], fill: &str, extra: &str) -> String {
    format!(r#"<polygon points="{}"{}/>"#, pts(ps), attrs(fill, extra))
}

pub fn polyline(ps: &[P], extra: &str) -> String {
    format!(r#"<polyline points="{}"{}/>"#, pts(ps), attrs("", extra))
}

pub fn line(x1: f64, y1: f64, x2: f64, y2: f64, extra: &str) -> String {
    format!(
        r#"<line x1="{}" y1="{}" x2="{}" y2="{}"{}/>"#,
        f(x1),
        f(y1),
        f(x2),
        f(y2),
        attrs("", extra)
    )
}

/// A line between two points.
pub fn seg(a: P, b: P, extra: &str) -> String {
    line(a.0, a.1, b.0, b.1, extra)
}

pub fn path(d: &str, fill: &str, extra: &str) -> String {
    format!(r#"<path d="{d}"{}/>"#, attrs(fill, extra))
}

pub fn circle(cx: f64, cy: f64, r: f64, fill: &str, extra: &str) -> String {
    format!(
        r#"<circle cx="{}" cy="{}" r="{}"{}/>"#,
        f(cx),
        f(cy),
        f(r),
        attrs(fill, extra)
    )
}

pub fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64, fill: &str, extra: &str) -> String {
    ellipse_rot(cx, cy, rx, ry, fill, extra, 0.0)
}

pub fn ellipse_rot(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    fill: &str,
    extra: &str,
    rotate: f64,
) -> String {
    let rot = if rotate != 0.0 {
        format!(r#" transform="rotate({} {} {})""#, f(rotate), f(cx), f(cy))
    } else {
        String::new()
    };
    format!(
        r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}"{rot}{}/>"#,
        f(cx),
        f(cy),
        f(rx),
        f(ry),
        attrs(fill, extra)
    )
}

pub fn rect(x: f64, y: f64, w: f64, h: f64, rx: f64, fill: &str, extra: &str) -> String {
    let r = if rx != 0.0 {
        format!(r#" rx="{}""#, f(rx))
    } else {
        String::new()
    };
    format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}"{r}{}/>"#,
        f(x),
        f(y),
        f(w),
        f(h),
        attrs(fill, extra)
    )
}

// Dashes. With round caps each dash grows by the stroke width and each gap shrinks by it, so
// a fixed dasharray looks cramped on thick strokes. dash() and dash_dot() compensate: every
// dashed line shows about DASH_ON-long dashes and DASH_GAP-wide gaps whatever its width. They
// return the stroke-width attribute too, so don't set it again.
pub const DASH_ON: f64 = 2.2;
pub const DASH_GAP: f64 = 1.6;

pub fn dash(sw: f64) -> String {
    dash_with(sw, DASH_ON, DASH_GAP)
}

pub fn dash_with(sw: f64, on: f64, gap: f64) -> String {
    format!(
        r#"stroke-width="{}" stroke-dasharray="{} {}""#,
        f(sw),
        f((on - sw).max(0.05)),
        f(gap + sw)
    )
}

/// Centre-line pattern: long dash, gap, dot, gap.
pub fn dash_dot(sw: f64) -> String {
    dash_dot_with(sw, 3.2, DASH_GAP)
}

pub fn dash_dot_with(sw: f64, on: f64, gap: f64) -> String {
    format!(
        r#"stroke-width="{}" stroke-dasharray="{} {} 0.01 {}""#,
        f(sw),
        f((on - sw).max(0.05)),
        f(gap + sw),
        f(gap + sw)
    )
}

// ---------------------------------------------------------------------------------------------
// Math helpers. The set was first generated in Python; these compute exactly as it did, so
// regenerating never shifts a coordinate.

pub fn rad(deg: f64) -> f64 {
    deg * (PI / 180.0)
}

pub fn deg(rad: f64) -> f64 {
    rad * (180.0 / PI)
}

pub fn hypot(x: f64, y: f64) -> f64 {
    x.hypot(y)
}

/// `v` rounded to 3 decimals: correctly rounded, ties to even.
pub fn round3(v: f64) -> f64 {
    format!("{v:.3}").parse().unwrap()
}

/// A sum with Neumaier compensation (as Python's `sum()` of floats).
pub fn psum(xs: impl IntoIterator<Item = f64>) -> f64 {
    let (mut total, mut c) = (0.0f64, 0.0f64);
    for x in xs {
        let t = total + x;
        if total.abs() >= x.abs() {
            c += (total - t) + x;
        } else {
            c += (x - t) + total;
        }
        total = t;
    }
    if c != 0.0 && c.is_finite() {
        total + c
    } else {
        total
    }
}

// ---------------------------------------------------------------------------------------------
// Isometric projection. x runs down-right, y down-left, z up; one unit is one SVG unit along
// each axis. The camera looks from +x +y +z, so visible faces of a box are its top (z max),
// right side (x max) and left side (y max).

pub const S30: f64 = 0.5;

pub fn c30() -> f64 {
    (PI / 6.0).cos()
}

pub fn iso(o: P, x: f64, y: f64, z: f64) -> P {
    (o.0 + (x - y) * c30(), o.1 + (x + y) * S30 - z)
}

/// Returns P(x, y, z) -> screen point, for origin `o`.
pub fn projector(o: P) -> impl Fn(f64, f64, f64) -> P + Copy {
    move |x, y, z| iso(o, x, y, z)
}

/// The three visible faces of an axis-aligned box, in TOP, MID and SHADE.
#[allow(clippy::too_many_arguments)]
pub fn iso_box(o: P, x0: f64, x1: f64, y0: f64, y1: f64, z0: f64, z1: f64) -> String {
    iso_box_with(o, x0, x1, y0, y1, z0, z1, TOP, MID, SHADE, "")
}

#[allow(clippy::too_many_arguments)]
pub fn iso_box_with(
    o: P,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    z0: f64,
    z1: f64,
    top: &str,
    right: &str,
    left: &str,
    extra: &str,
) -> String {
    let p = projector(o);
    join(vec![
        poly(
            &[p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)],
            top,
            extra,
        ),
        poly(
            &[p(x1, y0, z1), p(x1, y1, z1), p(x1, y1, z0), p(x1, y0, z0)],
            right,
            extra,
        ),
        poly(
            &[p(x0, y1, z1), p(x1, y1, z1), p(x1, y1, z0), p(x0, y1, z0)],
            left,
            extra,
        ),
    ])
}

/// A horizontal circle (in the xy plane at height z) as a projected point list.
pub fn iso_circle(o: P, cx: f64, cy: f64, z: f64, r: f64, n: usize) -> Vec<P> {
    let p = projector(o);
    (0..n)
        .map(|i| {
            let t = 2.0 * PI * i as f64 / n as f64;
            p(cx + r * t.cos(), cy + r * t.sin(), z)
        })
        .collect()
}

/// An upright cylinder in TOP and MID.
pub fn cylinder(o: P, cx: f64, cy: f64, z0: f64, z1: f64, r: f64) -> String {
    cylinder_with(o, cx, cy, z0, z1, r, TOP, MID, "")
}

/// An upright cylinder: side silhouette plus top ellipse. A horizontal circle of radius r
/// projects to an ellipse with rx = r*sqrt(1.5) and ry = r*sqrt(0.5).
#[allow(clippy::too_many_arguments)]
pub fn cylinder_with(
    o: P,
    cx: f64,
    cy: f64,
    z0: f64,
    z1: f64,
    r: f64,
    top: &str,
    side: &str,
    extra: &str,
) -> String {
    let p = projector(o);
    let (rx, ry) = (r * 1.5f64.sqrt(), r * 0.5f64.sqrt());
    let ((bx, by), (tx, ty)) = (p(cx, cy, z0), p(cx, cy, z1));
    let side_d = format!(
        "M{} {} L{} {} A{} {} 0 0 0 {} {} L{} {} Z",
        f(tx - rx),
        f(ty),
        f(bx - rx),
        f(by),
        f(rx),
        f(ry),
        f(bx + rx),
        f(by),
        f(tx + rx),
        f(ty)
    );
    join(vec![
        path(&side_d, side, extra),
        ellipse(tx, ty, rx, ry, top, extra),
    ])
}

// ---------------------------------------------------------------------------------------------
// Curves and arrows

pub fn cubic(p0: P, p1: P, p2: P, p3: P, t: f64) -> P {
    let u = 1.0 - t;
    let c = |a: f64, b: f64, c: f64, d: f64| {
        u.powf(3.0) * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t.powf(3.0) * d
    };
    (c(p0.0, p1.0, p2.0, p3.0), c(p0.1, p1.1, p2.1, p3.1))
}

pub fn sample_cubic(p0: P, p1: P, p2: P, p3: P, n: usize) -> Vec<P> {
    (0..=n)
        .map(|i| cubic(p0, p1, p2, p3, i as f64 / n as f64))
        .collect()
}

/// Points on an ellipse from deg0 to deg1 (degrees, clockwise on screen since y is down), every
/// 5 degrees.
pub fn arc_points(cx: f64, cy: f64, rx: f64, ry: f64, deg0: f64, deg1: f64) -> Vec<P> {
    arc_points_step(cx, cy, rx, ry, deg0, deg1, 5.0)
}

#[allow(clippy::too_many_arguments)]
pub fn arc_points_step(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    deg0: f64,
    deg1: f64,
    step: f64,
) -> Vec<P> {
    let n = 2.max(((deg1 - deg0).abs() / step) as i64) as usize;
    (0..=n)
        .map(|i| {
            let a = rad(deg0 + (deg1 - deg0) * i as f64 / n as f64);
            (cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

/// The polyline offset by k to its left (screen coordinates).
pub fn offset_polyline(ps: &[P], k: f64) -> Vec<P> {
    let n = ps.len();
    (0..n)
        .map(|i| {
            let (x, y) = ps[i];
            let (a, b) = (ps[i.saturating_sub(1)], ps[(i + 1).min(n - 1)]);
            let (tx, ty) = (b.0 - a.0, b.1 - a.1);
            let l = or1(hypot(tx, ty));
            (x - ty / l * k, y + tx / l * k)
        })
        .collect()
}

/// `v`, or 1 if it is zero (a safe divisor for a length).
pub fn or1(v: f64) -> f64 {
    if v == 0.0 { 1.0 } else { v }
}

/// A filled triangular head at `tip`, pointing away from `frm`.
pub fn arrow_head(tip: P, frm: P, length: f64, width: f64, fill: &str) -> String {
    let (dx, dy) = (tip.0 - frm.0, tip.1 - frm.1);
    let l = or1(hypot(dx, dy));
    let (ux, uy) = (dx / l, dy / l);
    let (bx, by) = (tip.0 - ux * length, tip.1 - uy * length);
    let (nx, ny) = (-uy * width / 2.0, ux * width / 2.0);
    poly(
        &[tip, (bx + nx, by + ny), (bx - nx, by - ny)],
        fill,
        if fill == INK {
            r#"stroke-width="0.6""#
        } else {
            ""
        },
    )
}

/// `el` with a stroke attribute added before its closing `/>`.
pub fn stroked(el: &str, color: &str) -> String {
    el.replace("/>", &format!(r#" stroke="{color}"/>"#))
}

/// A straight arrow from (x0, y0) to a filled head at (x1, y1).
#[allow(clippy::too_many_arguments)]
pub fn arrow(
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    head: f64,
    width: f64,
    color: &str,
    extra: &str,
) -> String {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let l = or1(hypot(dx, dy));
    let (bx, by) = (x1 - dx / l * head * 0.8, y1 - dy / l * head * 0.8);
    let stroke = if color != INK {
        format!(r#" stroke="{color}""#)
    } else {
        String::new()
    };
    let h = arrow_head((x1, y1), (x0, y0), head, width, color);
    join(vec![
        format!(
            r#"<line x1="{}" y1="{}" x2="{}" y2="{}"{stroke}{}/>"#,
            f(x0),
            f(y0),
            f(bx),
            f(by),
            attrs("", extra)
        ),
        if color != INK { stroked(&h, color) } else { h },
    ])
}

/// An arrow along the point list `ps`, with the head at the last point.
pub fn curved_arrow(ps: &[P], head: f64, width: f64, color: &str, extra: &str) -> String {
    let stroke = if color != INK {
        format!(r#" stroke="{color}""#)
    } else {
        String::new()
    };
    let tip = ps[ps.len() - 1];
    // Stop the shaft inside the head so the round cap doesn't poke out.
    let mut shaft = &ps[..ps.len() - 1];
    while shaft.len() > 1
        && hypot(
            shaft[shaft.len() - 1].0 - tip.0,
            shaft[shaft.len() - 1].1 - tip.1,
        ) < head * 0.7
    {
        shaft = &shaft[..shaft.len() - 1];
    }
    let mut head_el = arrow_head(tip, shaft[shaft.len() - 1], head, width, color);
    if color != INK {
        head_el = stroked(&head_el, color);
    }
    join(vec![
        format!(
            r#"<polyline points="{}"{stroke}{}/>"#,
            pts(shaft),
            attrs("", extra)
        ),
        head_el,
    ])
}

/// Pairs each item with the next, wrapping around: the edges of a polygon.
pub fn pairs<T: Copy>(ps: &[T]) -> impl Iterator<Item = (T, T)> + '_ {
    (0..ps.len()).map(move |i| (ps[i], ps[(i + 1) % ps.len()]))
}

/// Clips polygon `subject` to the convex polygon `convex` (Sutherland-Hodgman). Use this
/// instead of <clipPath>, which needs ids.
pub fn clip(subject: &[P], convex: &[P]) -> Vec<P> {
    fn inside(p: P, a: P, b: P) -> bool {
        (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) >= 0.0
    }
    fn cross(p: P, q: P, a: P, b: P) -> P {
        let ((x1, y1), (x2, y2), (x3, y3), (x4, y4)) = (p, q, a, b);
        let d = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
        if d.abs() < 1e-12 {
            // edge parallel to the clip edge (only reached through rounding)
            return q;
        }
        let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / d;
        (x1 + t * (x2 - x1), y1 + t * (y2 - y1))
    }

    // Orient the clip polygon counter-clockwise in screen space (y down).
    let area = psum(pairs(convex).map(|(a, b)| a.0 * b.1 - b.0 * a.1));
    let mut convex = convex.to_vec();
    if area < 0.0 {
        convex.reverse();
    }
    let mut out = subject.to_vec();
    for (a, b) in pairs(&convex) {
        let src = std::mem::take(&mut out);
        for (p, q) in pairs(&src) {
            if inside(q, a, b) {
                if !inside(p, a, b) {
                    out.push(cross(p, q, a, b));
                }
                out.push(q);
            } else if inside(p, a, b) {
                out.push(cross(p, q, a, b));
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// Points in reverse order.
pub fn rev(ps: &[P]) -> Vec<P> {
    ps.iter().rev().copied().collect()
}

/// Point lists joined end to end.
pub fn cat(parts: &[&[P]]) -> Vec<P> {
    parts.concat()
}
