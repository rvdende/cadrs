//! The "shown, not taught" annotations (P3C.8, D6.4, X14): baseline, ordinate, chamfer and
//! arc-length dimensions; GD&T feature control frames and datum feature symbols; surface finish
//! and weld symbols. Each is one [`crate::annotation::AnnotationKind`] of its view, attached to
//! the model through the same [`EdgeRef`]s and [`PointRef`]s as the other annotations, drawn
//! with thin lines, filled arrowheads and Inter text; the symbols are vector strokes.
//!
//! - **Baseline** ([`Baseline`]): distances from one base to several targets along one axis,
//!   each a linear dimension, stacked `spacing` apart on paper.
//! - **Ordinate** ([`Ordinate`]): each point's distance from a zero point along the view's x (or
//!   y), the values lined up at one level, leaders jogged where values would touch.
//! - **Chamfer** ([`ChamferDim`]): "1.00 x 45°" on a leader to a chamfer's edge, read from the
//!   Chamfer feature ([`crate::annotation::ChamferInfo`]); from the edge's legs otherwise.
//! - **Arc length** ([`ArcLength`]): the arc's length r·θ with the ⌒ symbol, on an arc
//!   concentric with it through the text.
//! - **Feature control frame** ([`FeatureControl`]): the characteristic (one of the 14 ASME
//!   Y14.5 symbols, [`Gdt`]), the tolerance (with Ø and a material-condition modifier) and up
//!   to three datum references, in a row of boxed cells ([`fcf_cells`]), with a leader.
//! - **Datum feature** ([`Datum`]): a boxed letter joined to a filled triangle on the feature.
//! - **Surface finish** ([`SurfaceFinish`]): the basic symbol, material removal required (the
//!   bar) or prohibited (the circle), with its value.
//! - **Weld** ([`Weld`]): an arrow to the joint, a reference line, the weld symbol below the
//!   line (arrow side) or above it (other side), its size and the all-around circle.

use serde::{Deserialize, Serialize};

use crate::annotation::{
    AnnGraphics, EdgeRef, GripKind, Orient, Pick, PlacedText, PointRef, Shape, ViewModel, layout_label, resolve, resolve_point,
    runs, text_width, TextBlock,
};
use crate::style::DrawingStyle;
use crate::view::{View, rotate};

type P2 = [f64; 2];

fn add(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: P2, k: f64) -> P2 {
    [a[0] * k, a[1] * k]
}
fn dot(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn len(a: P2) -> f64 {
    a[0].hypot(a[1])
}
fn dist(a: P2, b: P2) -> f64 {
    len(sub(a, b))
}
fn unit(a: P2) -> P2 {
    let l = len(a).max(1e-300);
    [a[0] / l, a[1] / l]
}
fn perp(a: P2) -> P2 {
    [-a[1], a[0]]
}

// ---------------------------------------------------------------------------------------------
// Types

/// Baseline dimensions: `base` to each of `targets` along `orient`, the first dimension line
/// through `text` (view 2D), the others `spacing` (paper mm) further out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub base: Pick,
    pub targets: Vec<Pick>,
    pub orient: Orient,
    pub text: P2,
    pub spacing: f64,
}

/// Ordinate dimensions: each point's distance from `origin` along the view's x (`vertical`
/// false: vertical leaders) or y (horizontal leaders), the values at `level` (the view 2D y,
/// or x, where the leaders end).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ordinate {
    pub origin: PointRef,
    pub points: Vec<PointRef>,
    pub vertical: bool,
    pub level: f64,
}

/// A chamfer dimension on a leader to the chamfer's edge; `text` is the text's left end
/// (view 2D).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamferDim {
    pub edge: EdgeRef,
    pub text: P2,
}

/// An arc length dimension; its arc runs through `text` (view 2D).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArcLength {
    pub edge: EdgeRef,
    pub text: P2,
}

/// The 14 geometric characteristics of ASME Y14.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Gdt {
    Straightness,
    Flatness,
    Circularity,
    Cylindricity,
    LineProfile,
    SurfaceProfile,
    Angularity,
    Perpendicularity,
    Parallelism,
    Position,
    Concentricity,
    Symmetry,
    CircularRunout,
    TotalRunout,
}

impl Gdt {
    pub const ALL: [Gdt; 14] = [
        Gdt::Straightness,
        Gdt::Flatness,
        Gdt::Circularity,
        Gdt::Cylindricity,
        Gdt::LineProfile,
        Gdt::SurfaceProfile,
        Gdt::Angularity,
        Gdt::Perpendicularity,
        Gdt::Parallelism,
        Gdt::Position,
        Gdt::Concentricity,
        Gdt::Symmetry,
        Gdt::CircularRunout,
        Gdt::TotalRunout,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Gdt::Straightness => "Straightness",
            Gdt::Flatness => "Flatness",
            Gdt::Circularity => "Circularity",
            Gdt::Cylindricity => "Cylindricity",
            Gdt::LineProfile => "Profile of a line",
            Gdt::SurfaceProfile => "Profile of a surface",
            Gdt::Angularity => "Angularity",
            Gdt::Perpendicularity => "Perpendicularity",
            Gdt::Parallelism => "Parallelism",
            Gdt::Position => "Position",
            Gdt::Concentricity => "Concentricity",
            Gdt::Symmetry => "Symmetry",
            Gdt::CircularRunout => "Circular runout",
            Gdt::TotalRunout => "Total runout",
        }
    }

    /// Its Unicode character (for text export).
    pub fn char(self) -> char {
        match self {
            Gdt::Straightness => '⏤',
            Gdt::Flatness => '⏥',
            Gdt::Circularity => '○',
            Gdt::Cylindricity => '⌭',
            Gdt::LineProfile => '⌒',
            Gdt::SurfaceProfile => '⌓',
            Gdt::Angularity => '∠',
            Gdt::Perpendicularity => '⟂',
            Gdt::Parallelism => '∥',
            Gdt::Position => '⌖',
            Gdt::Concentricity => '◎',
            Gdt::Symmetry => '⌯',
            Gdt::CircularRunout => '↗',
            Gdt::TotalRunout => '⌰',
        }
    }

    /// Form tolerances take no datum.
    pub fn takes_datums(self) -> bool {
        !matches!(self, Gdt::Straightness | Gdt::Flatness | Gdt::Circularity | Gdt::Cylindricity)
    }
}

/// A material-condition modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modifier {
    /// Ⓜ maximum material condition.
    Mmc,
    /// Ⓛ least material condition.
    Lmc,
    /// Ⓢ regardless of feature size.
    Rfs,
}

impl Modifier {
    pub fn letter(self) -> &'static str {
        match self {
            Modifier::Mmc => "M",
            Modifier::Lmc => "L",
            Modifier::Rfs => "S",
        }
    }
}

/// A feature control frame; `text` is the frame's left end at its middle (view 2D); a leader
/// runs to `edge` if it has one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureControl {
    pub characteristic: Gdt,
    pub tolerance: String,
    #[serde(default)]
    pub diameter: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifier: Option<Modifier>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub datums: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<EdgeRef>,
    pub text: P2,
}

/// A datum feature symbol: the letter's box centred at `text` (view 2D), the triangle on `edge`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Datum {
    pub letter: String,
    pub edge: EdgeRef,
    pub text: P2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishKind {
    Basic,
    RemovalRequired,
    RemovalProhibited,
}

impl FinishKind {
    pub const ALL: [FinishKind; 3] = [FinishKind::Basic, FinishKind::RemovalRequired, FinishKind::RemovalProhibited];

    pub fn label(self) -> &'static str {
        match self {
            FinishKind::Basic => "Basic",
            FinishKind::RemovalRequired => "Material removal required",
            FinishKind::RemovalProhibited => "Material removal prohibited",
        }
    }
}

/// A surface finish symbol with its point at `text` (view 2D) and a leader to `edge` when the
/// point is off it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceFinish {
    pub kind: FinishKind,
    #[serde(default)]
    pub value: String,
    pub edge: EdgeRef,
    pub text: P2,
}

/// A weld symbol on one side of the reference line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeldKind {
    None,
    Fillet,
    VGroove,
    Square,
}

impl WeldKind {
    pub const ALL: [WeldKind; 4] = [WeldKind::None, WeldKind::Fillet, WeldKind::VGroove, WeldKind::Square];

    pub fn label(self) -> &'static str {
        match self {
            WeldKind::None => "None",
            WeldKind::Fillet => "Fillet",
            WeldKind::VGroove => "V-groove",
            WeldKind::Square => "Square groove",
        }
    }
}

/// A weld symbol: an arrow to `edge`, the reference line from `text` (view 2D, its end at the
/// arrow) away from the joint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weld {
    pub edge: EdgeRef,
    pub text: P2,
    pub arrow_side: WeldKind,
    pub other_side: WeldKind,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub all_around: bool,
}

// ---------------------------------------------------------------------------------------------
// Values

/// An ordinate set's values (mm, from the origin), in the order of its points; `None` where a
/// point no longer resolves.
pub fn ordinate_values(view: &View, m: &dyn ViewModel, o: &Ordinate) -> Option<Vec<Option<f64>>> {
    let (p0, _) = resolve_point(view, m, &o.origin)?;
    let i = if o.vertical { 1 } else { 0 };
    Some(
        o.points
            .iter()
            .map(|p| resolve_point(view, m, p).map(|(q, _)| (q[i] - p0[i]).abs()))
            .collect(),
    )
}

/// A chamfer dimension's distance, second distance and angle: the Chamfer feature's, else the
/// edge's legs as seen (the shorter leg and the angle to the longer side).
pub fn chamfer_values(view: &View, m: &dyn ViewModel, c: &ChamferDim) -> Option<(f64, Option<f64>, f64)> {
    let faces: Vec<&cadrs_kernel::naming::FaceName> = match (&c.edge.edge, &c.edge.face) {
        (Some(n), _) => n.faces.iter().collect(),
        (None, Some(f)) => vec![f],
        _ => Vec::new(),
    };
    if let Some(ci) = faces.into_iter().find_map(|f| m.chamfer(&f.op)) {
        return Some((ci.distance, ci.distance2, ci.angle));
    }
    let (a, b) = match resolve(view, m, &c.edge).shape {
        Shape::Line { a, b } | Shape::Curve { a, b } => (a, b),
        _ => return None,
    };
    let (dx, dy) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
    let d = dx.min(dy);
    (d > 1e-9).then(|| (d, None, dx.max(dy).atan2(d).to_degrees()))
}

/// A chamfer dimension's text: "1.00 x 45°" (or "1.00 x 2.00").
pub fn chamfer_text(style: &DrawingStyle, v: (f64, Option<f64>, f64)) -> String {
    match v {
        (d, Some(d2), _) => format!("{} x {}", style.format_length(d), style.format_length(d2)),
        (d, None, a) => {
            let decimals = if (a - a.round()).abs() < 1e-6 { 0 } else { 1 };
            format!("{} x {}°", style.format_length(d), crate::style::format_number(a, decimals, true, false, style.decimal_separator))
        }
    }
}

/// An arc's centre, radius, start angle and sweep (radians, counter-clockwise), as seen.
fn arc_of(view: &View, m: &dyn ViewModel, e: &EdgeRef) -> Option<(P2, f64, f64, f64)> {
    let Shape::Circle { center, radius, arc: Some(a) } = resolve(view, m, e).shape else {
        return None;
    };
    let ang = |p: P2| (p[1] - center[1]).atan2(p[0] - center[0]);
    let (a0, am, a1) = (ang(a[0]), ang(a[1]), ang(a[2]));
    let ccw = |f: f64, t: f64| (t - f).rem_euclid(std::f64::consts::TAU);
    let (start, sweep) = if ccw(a0, am) <= ccw(a0, a1) { (a0, ccw(a0, a1)) } else { (a1, ccw(a1, a0)) };
    Some((center, radius, start, sweep))
}

/// An arc length's value (mm).
pub fn arc_length(view: &View, m: &dyn ViewModel, a: &ArcLength) -> Option<f64> {
    let (_, r, _, sweep) = arc_of(view, m, &a.edge)?;
    Some(r * sweep)
}

/// A feature control frame's cells (paper mm, min and max corners) with cap height `h`, its
/// left end's middle at `origin`: the characteristic, the tolerance, then one per datum.
pub fn fcf_cells(f: &FeatureControl, origin: P2, h: f64) -> Vec<(P2, P2)> {
    let hh = 2.0 * h;
    let pad = 0.5 * h;
    let tol_w = pad
        + if f.diameter { 0.85 * h } else { 0.0 }
        + text_width(&f.tolerance) * h
        + if f.modifier.is_some() { 1.35 * h } else { 0.0 }
        + pad;
    let mut widths = vec![hh, tol_w.max(hh)];
    for d in &f.datums {
        widths.push((text_width(d) * h + 2.0 * pad).max(hh));
    }
    let mut x = origin[0];
    widths
        .into_iter()
        .map(|w| {
            let c = ([x, origin[1] - hh / 2.0], [x + w, origin[1] + hh / 2.0]);
            x += w;
            c
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Symbols

fn circle_pts(c: P2, r: f64, from: f64, to: f64, n: usize) -> Vec<P2> {
    (0..=n)
        .map(|i| {
            let t = from + (to - from) * i as f64 / n as f64;
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        })
        .collect()
}

/// The strokes of a characteristic symbol in a square of side `s` centred at `c` (and its filled
/// arrowheads).
pub fn gdt_strokes(g: Gdt, c: P2, s: f64) -> (Vec<Vec<P2>>, Vec<[P2; 3]>) {
    let p = |x: f64, y: f64| [c[0] + x * s, c[1] + y * s];
    let tau = std::f64::consts::TAU;
    let pi = std::f64::consts::PI;
    let mut fills = Vec::new();
    let strokes = match g {
        Gdt::Straightness => vec![vec![p(-0.35, 0.0), p(0.35, 0.0)]],
        Gdt::Flatness => vec![vec![p(-0.35, -0.15), p(0.2, -0.15), p(0.35, 0.15), p(-0.2, 0.15), p(-0.35, -0.15)]],
        Gdt::Circularity => vec![circle_pts(c, 0.3 * s, 0.0, tau, 40)],
        Gdt::Cylindricity => vec![
            circle_pts(c, 0.25 * s, 0.0, tau, 40),
            vec![p(-0.38, -0.3), p(-0.13, 0.3)],
            vec![p(0.13, -0.3), p(0.38, 0.3)],
        ],
        Gdt::LineProfile => vec![circle_pts(p(0.0, -0.15), 0.32 * s, 0.0, pi, 30)],
        Gdt::SurfaceProfile => {
            let mut a = circle_pts(p(0.0, -0.15), 0.32 * s, 0.0, pi, 30);
            a.push(a[0]);
            vec![a]
        }
        Gdt::Angularity => vec![vec![p(0.35, 0.3), p(-0.35, -0.25), p(0.35, -0.25)]],
        Gdt::Perpendicularity => vec![vec![p(0.0, 0.32), p(0.0, -0.25)], vec![p(-0.35, -0.25), p(0.35, -0.25)]],
        Gdt::Parallelism => vec![vec![p(-0.3, -0.3), p(-0.05, 0.3)], vec![p(0.05, -0.3), p(0.3, 0.3)]],
        Gdt::Position => vec![
            circle_pts(c, 0.24 * s, 0.0, tau, 40),
            vec![p(-0.4, 0.0), p(0.4, 0.0)],
            vec![p(0.0, -0.4), p(0.0, 0.4)],
        ],
        Gdt::Concentricity => vec![circle_pts(c, 0.14 * s, 0.0, tau, 30), circle_pts(c, 0.32 * s, 0.0, tau, 40)],
        Gdt::Symmetry => vec![
            vec![p(-0.22, 0.2), p(0.22, 0.2)],
            vec![p(-0.38, 0.0), p(0.38, 0.0)],
            vec![p(-0.22, -0.2), p(0.22, -0.2)],
        ],
        Gdt::CircularRunout => {
            let (tip, tail) = (p(0.25, 0.3), p(-0.25, -0.3));
            let d = unit(sub(tip, tail));
            let n = mul(perp(d), 0.09 * s);
            let base = sub(tip, mul(d, 0.25 * s));
            fills.push([tip, add(base, n), sub(base, n)]);
            vec![vec![tail, base]]
        }
        Gdt::TotalRunout => {
            let mut out = vec![vec![p(-0.38, -0.32), p(0.3, -0.32)]];
            for x in [-0.3, 0.05] {
                let (tip, tail) = (p(x + 0.25, 0.32), p(x, -0.32));
                let d = unit(sub(tip, tail));
                let n = mul(perp(d), 0.08 * s);
                let base = sub(tip, mul(d, 0.22 * s));
                fills.push([tip, add(base, n), sub(base, n)]);
                out.push(vec![tail, base]);
            }
            out
        }
    };
    (strokes, fills)
}

// ---------------------------------------------------------------------------------------------
// Graphics

struct Sz {
    h: f64,
    arrow: f64,
    gap: f64,
    beyond: f64,
}

fn sizes(style: &DrawingStyle) -> Sz {
    Sz { h: style.dim_text_height, arrow: style.dim_arrow_length, gap: style.extension_gap, beyond: style.extension_beyond }
}

fn arrow(g: &mut AnnGraphics, tip: P2, dir: P2, length: f64) {
    let d = unit(dir);
    let base = sub(tip, mul(d, length));
    let n = mul(perp(d), length * 0.18);
    g.fills.push([tip, add(base, n), sub(base, n)]);
}

fn text(g: &mut AnnGraphics, block: &TextBlock, anchor: P2, h: f64, centered: bool) -> (P2, P2) {
    let l = layout_label(block, anchor, h, centered);
    let bx = (l.min, l.max);
    g.texts.extend(l.texts);
    g.strokes.extend(l.strokes);
    g.symbols.extend(l.symbols);
    g.boxes.push(bx);
    bx
}

fn plain(s: &str) -> TextBlock {
    TextBlock { lines: vec![runs(s)] }
}

/// The point of the edge (as drawn on the sheet) nearest `near` (sheet), with the edge's
/// direction there.
fn edge_foot(view: &View, m: &dyn ViewModel, e: &EdgeRef, near: P2) -> Option<(P2, P2)> {
    let r = resolve(view, m, e);
    let pts: Vec<P2> = r.shape.polyline().into_iter().map(|p| view.to_sheet(p)).collect();
    let mut best: Option<(f64, P2, P2)> = None;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = sub(b, a);
        let l2 = dot(d, d);
        if l2 < 1e-18 {
            continue;
        }
        let t = (dot(sub(near, a), d) / l2).clamp(0.0, 1.0);
        let q = add(a, mul(d, t));
        let dd = dist(q, near);
        if best.is_none_or(|(b0, _, _)| dd < b0) {
            best = Some((dd, q, unit(d)));
        }
    }
    best.map(|(_, q, d)| (q, d)).or_else(|| pts.first().map(|p| (*p, [1.0, 0.0])))
}

fn attach(g: &mut AnnGraphics, view: &View, m: &dyn ViewModel, e: &EdgeRef) {
    let r = resolve(view, m, e);
    g.attached.push(r.shape.polyline().into_iter().map(|p| view.to_sheet(p)).collect());
    g.attached_dead.push(crate::annotation::ref_dangles(view, m, e));
}

/// A baseline set's graphics: each target's linear dimension, stacked.
pub fn baseline_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, b: &Baseline) -> Option<AnnGraphics> {
    use crate::annotation::{Annotation, AnnotationKind, DimFormat, DimKind, Dimension, annotation_graphics, centre_text_in_span};
    let k = view.scale.factor();
    // The stacking direction (view 2D): away from the base, across the measured axis.
    let (sx, sy) = (rotate([1.0, 0.0], -view.rotation), rotate([0.0, 1.0], -view.rotation));
    let base_p = match b.base {
        Pick::Point(p) => resolve_point(view, m, &p)?.0,
        Pick::Edge(e) => resolve(view, m, &e).shape.polyline().first().copied()?,
    };
    let across = match b.orient {
        Orient::Horizontal => sy,
        Orient::Vertical => sx,
        Orient::Aligned => sy,
    };
    let side = if dot(sub(b.text, base_p), across) >= 0.0 { 1.0 } else { -1.0 };
    let mut out = AnnGraphics::default();
    for (i, t) in b.targets.iter().enumerate() {
        let mut d = Dimension {
            kind: DimKind::Distance { a: b.base, b: *t, orient: b.orient },
            text: add(b.text, mul(across, side * i as f64 * b.spacing / k)),
            format: DimFormat::default(),
            last: None,
        };
        centre_text_in_span(view, m, &mut d);
        let Some(g) = annotation_graphics(style, view, m, &Annotation::new(AnnotationKind::Dimension(d))) else { continue };
        out.strokes.extend(g.strokes);
        out.fills.extend(g.fills);
        out.texts.extend(g.texts);
        out.boxes.extend(g.boxes);
        if i == 0 {
            out.grips.extend(g.grips.iter().filter(|(_, k)| *k == GripKind::Text).copied());
        }
        out.attached.extend(g.attached);
        out.attached_dead.extend(g.attached_dead);
    }
    (!out.texts.is_empty()).then_some(out)
}

/// An ordinate set's graphics.
pub fn ordinate_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, o: &Ordinate) -> Option<AnnGraphics> {
    let sz = sizes(style);
    let h = sz.h;
    let (p0, _) = resolve_point(view, m, &o.origin)?;
    let mut pts: Vec<(P2, f64)> = vec![(p0, 0.0)];
    let values = ordinate_values(view, m, o)?;
    for (p, v) in o.points.iter().zip(values) {
        if let (Some((q, _)), Some(v)) = (resolve_point(view, m, p), v) {
            pts.push((q, v));
        }
    }
    let mut g = AnnGraphics::default();
    let (i, j) = if o.vertical { (1, 0) } else { (0, 1) };
    // The view's outline on the sheet (its visible edges' bounds).
    let outline = {
        let mut b: Option<(P2, P2)> = None;
        for e in m.projection().edges.iter().filter(|e| e.visibility == cadrs_kernel::ProjVisibility::Visible) {
            for q in &e.points {
                let p = view.to_sheet([q.x, q.y]);
                b = Some(match b {
                    None => (p, p),
                    Some((lo, hi)) => ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]),
                });
            }
        }
        b
    };
    // Sort along the measured axis; values stack along it at the level, each clear of the last.
    let mut order: Vec<usize> = (0..pts.len()).collect();
    order.sort_by(|a, b| pts[*a].0[i].total_cmp(&pts[*b].0[i]));
    let mut last_end = f64::MIN;
    let mut level_pt = [0.0; 2];
    level_pt[j] = o.level;
    let level_s = view.to_sheet(level_pt);
    for idx in order {
        let (p, v) = pts[idx];
        let s = view.to_sheet(p);
        let label = style.format_length(v);
        let (along, perp_len) = if o.vertical { (h, text_width(&label) * h) } else { (text_width(&label) * h, h) };
        let _ = perp_len;
        // The value's centre along the axis: the point's, pushed clear of the previous value.
        let want = s[i];
        let half = along / 2.0 + 0.4 * h;
        let at = if want - half < last_end { last_end + half } else { want };
        last_end = at + half;
        // The leader: from the point (a gap off) to the level, jogged to the value's place.
        let sgn = if level_s[j] >= s[j] { 1.0 } else { -1.0 };
        let mut start = s;
        // A clear gap off the part (P3C.8's delta): from the view's outline on the level's side
        // when the point is inside it (a hole's centre), else from the point.
        let gap = sz.gap.max(1.5);
        start[j] += sgn * gap;
        if let Some((lo, hi)) = outline {
            let edge = if sgn > 0.0 { hi[j] } else { lo[j] };
            if (edge - s[j]) * sgn > 0.0 {
                start[j] = edge + sgn * gap;
            }
        }
        let mut knee = s;
        knee[j] = level_s[j] - sgn * 2.5 * h;
        let mut jog = knee;
        jog[i] = at;
        jog[j] = level_s[j] - sgn * 1.2 * h;
        let mut end = jog;
        end[j] = level_s[j];
        if (at - want).abs() > 1e-6 {
            g.strokes.push(vec![start, knee, jog, end]);
        } else {
            g.strokes.push(vec![start, end]);
        }
        // The value beyond the leader's end.
        let c = if o.vertical {
            [end[0] + sgn * (0.6 * h + text_width(&label) * h / 2.0), end[1]]
        } else {
            [end[0], end[1] + sgn * (0.6 * h + h / 2.0)]
        };
        text(&mut g, &plain(&label), c, h, true);
        if idx == 0 {
            g.grips.push((c, GripKind::Text));
        }
    }
    attach(&mut g, view, m, &o.origin.edge);
    for p in &o.points {
        attach(&mut g, view, m, &p.edge);
    }
    Some(g)
}

/// A leader from `tip` (on the geometry) to a text whose left end (middle) is `t`, with a short
/// landing; the text goes on the side of `t` away from the tip. Returns the text's left end.
/// With `angled` the leader leaves `tip` at least 30° off the vertical,
/// the shoulder shortened to make room (the chamfer note, clear of an extension line through
/// the chamfer's end; P3C wrap-up).
fn leader_text_with(g: &mut AnnGraphics, sz: &Sz, tip: P2, t: P2, block: &TextBlock, angled: bool) -> P2 {
    let right = t[0] >= tip[0];
    let w = {
        let l = layout_label(block, [0.0, 0.0], sz.h, false);
        l.max[0] - l.min[0] - 0.7 * sz.h
    };
    let left_x = if right { t[0] } else { t[0] - w };
    let pad = 0.35 * sz.h;
    let land_end = if right { [left_x - pad, t[1]] } else { [left_x + w + pad, t[1]] };
    let mut land_start = add(land_end, [if right { -sz.arrow } else { sz.arrow }, 0.0]);
    if angled {
        let dx = 0.58 * (t[1] - tip[1]).abs();
        if right && land_start[0] - tip[0] < dx {
            land_start[0] = (tip[0] + dx).min(land_end[0]);
        } else if !right && tip[0] - land_start[0] < dx {
            land_start[0] = (tip[0] - dx).max(land_end[0]);
        }
    }
    g.strokes.push(vec![tip, land_start, land_end]);
    arrow(g, tip, sub(tip, land_start), sz.arrow);
    text(g, block, [left_x, t[1]], sz.h, false);
    [left_x, t[1]]
}

/// A chamfer dimension's graphics.
pub fn chamfer_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, c: &ChamferDim) -> Option<AnnGraphics> {
    let sz = sizes(style);
    let label = chamfer_text(style, chamfer_values(view, m, c)?);
    let t = view.to_sheet(c.text);
    // The leader lands on the chamfer's middle.
    let pts = resolve(view, m, &c.edge).shape.polyline();
    let (first, last) = (*pts.first()?, *pts.last()?);
    let centre = view.to_sheet(mul(add(first, last), 0.5));
    let mut g = AnnGraphics::default();
    leader_text_with(&mut g, &sz, centre, t, &plain(&label), true);
    g.grips.push((t, GripKind::Text));
    g.grips.push((centre, GripKind::Attach(0)));
    attach(&mut g, view, m, &c.edge);
    Some(g)
}

/// An arc length dimension's graphics.
pub fn arc_length_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, a: &ArcLength) -> Option<AnnGraphics> {
    let sz = sizes(style);
    let (c, r, start, sweep) = arc_of(view, m, &a.edge)?;
    let value = r * sweep;
    let k = view.scale.factor();
    let cs = view.to_sheet(c);
    let ts = view.to_sheet(a.text);
    // The dimension arc through the text, on the sheet (view rotation applied to the angles).
    let rho = dist(ts, cs).max(r * k + 2.0 * sz.arrow);
    let rot = view.rotation;
    let (s0, s1) = (start + rot, start + sweep + rot);
    let pts = circle_pts(cs, rho, s0, s1, 48);
    let label = format!("⌒{}", style.format_length(value));
    let block = plain(&label);
    let l = layout_label(&block, ts, sz.h, true);
    let (lo, hi) = (sub(l.min, [0.6, 0.6]), add(l.max, [0.6, 0.6]));
    let inside = |p: &P2| p[0] > lo[0] && p[0] < hi[0] && p[1] > lo[1] && p[1] < hi[1];
    let mut g = AnnGraphics::default();
    let mut cur: Vec<P2> = Vec::new();
    for p in &pts {
        if inside(p) {
            if cur.len() > 1 {
                g.strokes.push(std::mem::take(&mut cur));
            }
            cur.clear();
        } else {
            cur.push(*p);
        }
    }
    if cur.len() > 1 {
        g.strokes.push(cur);
    }
    let e0 = pts[0];
    let e1 = pts[pts.len() - 1];
    arrow(&mut g, e0, [s0.sin(), -s0.cos()], sz.arrow);
    arrow(&mut g, e1, [-s1.sin(), s1.cos()], sz.arrow);
    // Extension lines out from the arc's ends (parallel to the bisector, as ASME draws them).
    for s in [s0, s1] {
        let d = [s.cos(), s.sin()];
        let from = add(cs, mul(d, r * k + sz.gap));
        let to = add(cs, mul(d, rho + sz.beyond));
        if rho > r * k + sz.gap {
            g.strokes.push(vec![from, to]);
        }
    }
    text(&mut g, &block, ts, sz.h, true);
    g.grips.push((ts, GripKind::Text));
    attach(&mut g, view, m, &a.edge);
    Some(g)
}

/// A feature control frame's graphics.
pub fn fcf_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, f: &FeatureControl) -> Option<AnnGraphics> {
    let sz = sizes(style);
    let h = sz.h;
    let origin = view.to_sheet(f.text);
    let cells = fcf_cells(f, origin, h);
    let mut g = AnnGraphics::default();
    let (lo, hi) = (cells[0].0, cells[cells.len() - 1].1);
    g.strokes.push(vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]], lo]);
    for c in &cells[1..] {
        g.strokes.push(vec![c.0, [c.0[0], c.1[1]]]);
    }
    g.boxes.push((lo, hi));
    // The characteristic.
    let c0 = mul(add(cells[0].0, cells[0].1), 0.5);
    let (s, fl) = gdt_strokes(f.characteristic, c0, 2.0 * h * 0.8);
    g.strokes.extend(s);
    g.fills.extend(fl);
    g.symbols.push(([c0[0] - 0.4 * h, c0[1]], h, f.characteristic.char()));
    // The tolerance: Ø, value, modifier.
    let pad = 0.5 * h;
    let mut x = cells[1].0[0] + pad;
    let y = origin[1];
    if f.diameter {
        g.texts.push(PlacedText { pos: [x, y], height: h, text: "Ø".into() });
        x += 0.85 * h;
    }
    g.texts.push(PlacedText { pos: [x, y], height: h, text: f.tolerance.clone() });
    x += text_width(&f.tolerance) * h;
    if let Some(md) = f.modifier {
        let c = [x + 0.7 * h, y];
        g.strokes.push(circle_pts(c, 0.62 * h, 0.0, std::f64::consts::TAU, 32));
        let circled = match md {
            Modifier::Mmc => 'Ⓜ',
            Modifier::Lmc => 'Ⓛ',
            Modifier::Rfs => 'Ⓢ',
        };
        g.symbols.push(([c[0] - 0.4 * h, c[1]], h, circled));
        let w = text_width(md.letter()) * 0.8 * h;
        g.texts.push(PlacedText { pos: [c[0] - w / 2.0, y], height: 0.8 * h, text: md.letter().into() });
    }
    // The datums.
    for (d, c) in f.datums.iter().zip(&cells[2..]) {
        let cc = mul(add(c.0, c.1), 0.5);
        let w = text_width(d) * h;
        g.texts.push(PlacedText { pos: [cc[0] - w / 2.0, cc[1]], height: h, text: d.clone() });
    }
    g.grips.push((origin, GripKind::Text));
    // The leader from the frame's nearer end to the feature.
    if let Some(e) = &f.edge
        && let Some((foot, _)) = edge_foot(view, m, e, origin)
    {
        let from = if foot[0] < lo[0] { lo_mid(lo, hi, true) } else if foot[0] > hi[0] { lo_mid(lo, hi, false) } else if foot[1] < lo[1] { [(lo[0] + hi[0]) / 2.0, lo[1]] } else { [(lo[0] + hi[0]) / 2.0, hi[1]] };
        // An elbow: out of the frame horizontally, then to the feature.
        let elbow = if from[1] == (lo[1] + hi[1]) / 2.0 { add(from, [if foot[0] < lo[0] { -sz.arrow } else { sz.arrow }, 0.0]) } else { from };
        g.strokes.push(vec![from, elbow, foot]);
        arrow(&mut g, foot, sub(foot, elbow), sz.arrow);
        g.grips.push((foot, GripKind::Attach(0)));
        attach(&mut g, view, m, e);
    }
    Some(g)
}

fn lo_mid(lo: P2, hi: P2, left: bool) -> P2 {
    [if left { lo[0] } else { hi[0] }, (lo[1] + hi[1]) / 2.0]
}

/// A datum feature symbol's graphics.
pub fn datum_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, d: &Datum) -> Option<AnnGraphics> {
    let sz = sizes(style);
    let h = sz.h;
    let c = view.to_sheet(d.text);
    let (foot, dir) = edge_foot(view, m, &d.edge, c)?;
    let mut g = AnnGraphics::default();
    // The box.
    let half = [(text_width(&d.letter) * h + 1.2 * h).max(2.2 * h) / 2.0, 1.1 * h];
    let (lo, hi) = (sub(c, half), add(c, half));
    g.strokes.push(vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]], lo]);
    let w = text_width(&d.letter) * h;
    g.texts.push(PlacedText { pos: [c[0] - w / 2.0, c[1]], height: h, text: d.letter.clone() });
    g.boxes.push((lo, hi));
    // The triangle on the feature, pointing at the box, and the leader from its apex.
    let mut n = perp(dir);
    if dot(sub(c, foot), n) < 0.0 {
        n = mul(n, -1.0);
    }
    let t = 1.0 * h;
    let apex = add(foot, mul(n, t * 0.87));
    g.fills.push([add(foot, mul(dir, t / 2.0)), sub(foot, mul(dir, t / 2.0)), apex]);
    // The leader leaves the box's side facing the feature.
    let to_box = if (c[1] - apex[1]).abs() >= (c[0] - apex[0]).abs() {
        [c[0], if apex[1] < c[1] { lo[1] } else { hi[1] }]
    } else {
        [if apex[0] < c[0] { lo[0] } else { hi[0] }, c[1]]
    };
    g.strokes.push(vec![apex, to_box]);
    g.grips.push((c, GripKind::Text));
    g.grips.push((foot, GripKind::Attach(0)));
    attach(&mut g, view, m, &d.edge);
    Some(g)
}

/// A surface finish symbol's graphics.
pub fn finish_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, f: &SurfaceFinish) -> Option<AnnGraphics> {
    let sz = sizes(style);
    let h = sz.h;
    let v = view.to_sheet(f.text);
    let (foot, _) = edge_foot(view, m, &f.edge, v)?;
    let mut g = AnnGraphics::default();
    // The check mark: legs at 60° from the horizontal, the long one twice the short.
    let s = 1.5 * h;
    let (c60, s60) = (60f64.to_radians().cos(), 60f64.to_radians().sin());
    let short_top = add(v, [-s * c60, s * s60]);
    let long_top = add(v, [2.0 * s * c60, 2.0 * s * s60]);
    g.strokes.push(vec![short_top, v, long_top]);
    match f.kind {
        FinishKind::Basic => {}
        FinishKind::RemovalRequired => {
            // The bar closing the short leg's triangle.
            let across = add(v, [s * c60, s * s60]);
            g.strokes.push(vec![short_top, across]);
        }
        FinishKind::RemovalProhibited => {
            let r = s * 0.35;
            g.strokes.push(circle_pts(add(v, [0.0, s * s60 * 0.55]), r, 0.0, std::f64::consts::TAU, 32));
        }
    }
    // The value on a line out from the long leg's top.
    let line_len = (text_width(&f.value) * h + 0.8 * h).max(2.0 * h);
    if !f.value.is_empty() {
        g.strokes.push(vec![long_top, add(long_top, [line_len, 0.0])]);
        g.texts.push(PlacedText { pos: [long_top[0] + 0.4 * h, long_top[1] + 0.9 * h], height: h, text: f.value.clone() });
    }
    // A leader when the point is off the surface.
    if dist(foot, v) > 0.5 {
        g.strokes.push(vec![foot, v]);
        arrow(&mut g, foot, sub(foot, v), sz.arrow);
    }
    g.boxes.push(([short_top[0], v[1]], [long_top[0] + if f.value.is_empty() { 0.0 } else { line_len }, long_top[1] + 1.6 * h]));
    g.grips.push((v, GripKind::Text));
    g.grips.push((foot, GripKind::Attach(0)));
    attach(&mut g, view, m, &f.edge);
    Some(g)
}

fn weld_symbol(g: &mut AnnGraphics, kind: WeldKind, x: f64, y: f64, up: f64, h: f64) {
    let s = 1.6 * h;
    match kind {
        WeldKind::None => {}
        // The perpendicular leg on the left.
        WeldKind::Fillet => g.strokes.push(vec![[x, y], [x, y + up * s], [x + s, y], [x, y]]),
        WeldKind::VGroove => g.strokes.push(vec![[x - s * 0.5, y + up * s], [x, y], [x + s * 0.5, y + up * s]]),
        WeldKind::Square => {
            g.strokes.push(vec![[x - 0.3 * s, y], [x - 0.3 * s, y + up * s]]);
            g.strokes.push(vec![[x + 0.3 * s, y], [x + 0.3 * s, y + up * s]]);
        }
    }
}

/// A weld symbol's graphics.
pub fn weld_graphics(style: &DrawingStyle, view: &View, m: &dyn ViewModel, w: &Weld) -> Option<AnnGraphics> {
    let sz = sizes(style);
    let h = sz.h;
    let start = view.to_sheet(w.text);
    let (foot, _) = edge_foot(view, m, &w.edge, start)?;
    let mut g = AnnGraphics::default();
    // The reference line runs away from the joint, from its arrow end.
    let away = if start[0] >= foot[0] { 1.0 } else { -1.0 };
    let len_line = 7.0 * h;
    let end = add(start, [away * len_line, 0.0]);
    g.strokes.push(vec![start, end]);
    g.strokes.push(vec![foot, start]);
    arrow(&mut g, foot, sub(foot, start), sz.arrow);
    // The symbols near the arrow end: below the line on the arrow side, above on the other.
    let sx = start[0] + away * 3.2 * h;
    weld_symbol(&mut g, w.arrow_side, sx, start[1], -1.0, h);
    weld_symbol(&mut g, w.other_side, sx, start[1], 1.0, h);
    if !w.size.is_empty() {
        let tw = text_width(&w.size) * h;
        let tx = sx - 0.5 * h - tw - if matches!(w.arrow_side, WeldKind::VGroove | WeldKind::Square) { 0.8 * h } else { 0.0 };
        if w.arrow_side != WeldKind::None {
            g.texts.push(PlacedText { pos: [tx, start[1] - 0.9 * h], height: h, text: w.size.clone() });
        }
        if w.other_side != WeldKind::None {
            g.texts.push(PlacedText { pos: [tx, start[1] + 0.9 * h], height: h, text: w.size.clone() });
        }
    }
    if w.all_around {
        g.strokes.push(circle_pts(start, 0.6 * h, 0.0, std::f64::consts::TAU, 32));
    }
    let (x0, x1) = (start[0].min(end[0]), start[0].max(end[0]));
    g.boxes.push(([x0, start[1] - 2.0 * h], [x1, start[1] + 2.0 * h]));
    g.grips.push((start, GripKind::Text));
    g.grips.push((foot, GripKind::Attach(0)));
    attach(&mut g, view, m, &w.edge);
    Some(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_has_a_cell_per_part() {
        let f = FeatureControl {
            characteristic: Gdt::Position,
            tolerance: "0.05".into(),
            diameter: true,
            modifier: Some(Modifier::Mmc),
            datums: vec!["A".into(), "B".into(), "C".into()],
            edge: None,
            text: [0.0, 0.0],
        };
        let cells = fcf_cells(&f, [10.0, 20.0], 3.0);
        assert_eq!(cells.len(), 5);
        // Contiguous, one row, the frame's height twice the text.
        for w in cells.windows(2) {
            assert!((w[0].1[0] - w[1].0[0]).abs() < 1e-12);
        }
        assert!(cells.iter().all(|c| (c.1[1] - c.0[1] - 6.0).abs() < 1e-12));
        let no_datum = FeatureControl { characteristic: Gdt::Flatness, datums: Vec::new(), diameter: false, modifier: None, ..f };
        assert_eq!(fcf_cells(&no_datum, [0.0, 0.0], 3.0).len(), 2);
    }

    #[test]
    fn the_fourteen_characteristics_are_distinct_symbols() {
        assert_eq!(Gdt::ALL.len(), 14);
        let chars: std::collections::HashSet<char> = Gdt::ALL.iter().map(|g| g.char()).collect();
        assert_eq!(chars.len(), 14);
        let mut drawn = std::collections::HashSet::new();
        for g in Gdt::ALL {
            let (s, f) = gdt_strokes(g, [0.0, 0.0], 10.0);
            assert!(!s.is_empty(), "{g:?}");
            // Every stroke inside its cell.
            assert!(s.iter().flatten().all(|p| p[0].abs() <= 5.0 && p[1].abs() <= 5.0), "{g:?}");
            drawn.insert(format!("{s:?}{f:?}"));
        }
        assert_eq!(drawn.len(), 14);
        assert_eq!(Gdt::ALL.iter().filter(|g| !g.takes_datums()).count(), 4);
    }

    #[test]
    fn chamfer_texts() {
        let st = DrawingStyle::default();
        assert_eq!(chamfer_text(&st, (1.0, None, 45.0)), "1.00 x 45°");
        assert_eq!(chamfer_text(&st, (1.5, None, 30.0)), "1.50 x 30°");
        assert_eq!(chamfer_text(&st, (1.0, Some(2.0), 63.4)), "1.00 x 2.00");
    }
}
