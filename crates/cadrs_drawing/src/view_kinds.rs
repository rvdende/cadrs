//! The "shown, not taught" view types (P3C.8, D4.12, X14): section, detail, crop, broken-out and
//! break views, and thread display (D4.8).
//!
//! - **Section view** ([`section_view`]): a cutting line drawn on a parent view. The section is
//!   folded out of the parent like an auxiliary view (aligned, on the side it was placed) and the
//!   material on the viewer's side of the cutting plane is removed before the hidden-line
//!   removal ([`ViewCut`]; `cadrs_core` cuts the bodies with the kernel). The faces the cut
//!   leaves on the plane come back as a hatch region ([`crate::annotation::ViewModel::hatch`]),
//!   drawn with ANSI31 lines (45°, 3.175 mm apart on paper). The parent shows the cutting line
//!   (a chain line) with arrows in the direction of sight and the letters; the view is labelled
//!   "SECTION A-A".
//! - **Detail view** ([`detail_view`]): a circle on a parent; the detail shows the parent's
//!   projection (the same frame, cut and all) clipped to the circle, at its own scale (twice the
//!   parent's by default), labelled "DETAIL B" with its scale; the parent shows the circle and
//!   letter.
//! - **Crop view**: a view clipped to a rectangle or a closed spline ([`Boundary`]).
//! - **Broken-out section**: a closed spline on a view and a depth; inside it the material in
//!   front of the depth is removed (a [`ViewCut`] with a polygon) and the cut faces hatched.
//! - **Break view** ([`Break`]): two parallel break lines; the band between them is removed and
//!   the far side moves up to a fixed gap on paper. The break is part of the view's placement
//!   ([`crate::View::to_sheet`]), so annotations follow it, and dimensions still measure the
//!   model: a dimension across a break reads the true length.
//! - **Show threads**: a tapped hole's major diameter as a thin 3/4 circle where the hole is
//!   seen end on (ANSI), and as two thin lines along the thread where it is seen from the side
//!   (dashed where hidden, solid in a section).
//!
//! Clipping happens in the view's 2D frame (model mm) before the sheet transform ([`Clip`]).

use serde::{Deserialize, Serialize};

use crate::annotation::{PlacedText, ThreadInfo, ViewModel};
use crate::standard::{Projection, Scale};
use crate::style::DrawingStyle;
use crate::view::{Frame3, View, ViewId, ViewKind, aligned_anchor, dashes, fold_frame, rotate};

pub type P2 = [f64; 2];

/// The gap a break leaves on paper (mm).
pub const BREAK_GAP: f64 = 8.0;
/// ANSI31 hatch spacing on paper (mm): 1/8 in.
pub const HATCH_SPACING: f64 = 3.175;
/// ANSI31 hatch angle on the sheet (degrees).
pub const HATCH_ANGLE: f64 = 45.0;

/// Material removed before a view is projected (P3C.8): everything nearer the eye than `depth`
/// (along the view's direction of sight, measured from the model origin) whose projection lies
/// inside `polygon` (the view's 2D frame; the whole view when empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewCut {
    pub depth: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polygon: Vec<P2>,
}

/// A section view's cutting line, in its parent's 2D frame (model mm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionLine {
    pub letter: String,
    pub a: P2,
    pub b: P2,
}

/// A detail view's circle, in its parent's 2D frame (the detail's own: they share the frame).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailCircle {
    pub letter: String,
    pub center: P2,
    pub radius: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundaryKind {
    /// Two opposite corners.
    Rectangle,
    /// A closed curve through the points.
    Spline,
}

/// A closed boundary in a view's 2D frame (crop views, broken-out sections).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Boundary {
    pub kind: BoundaryKind,
    pub points: Vec<P2>,
}

impl Boundary {
    pub fn rectangle(a: P2, b: P2) -> Self {
        Self { kind: BoundaryKind::Rectangle, points: vec![a, b] }
    }

    pub fn spline(points: Vec<P2>) -> Self {
        Self { kind: BoundaryKind::Spline, points }
    }

    /// The boundary as a closed polygon (the last point not repeated).
    pub fn polygon(&self) -> Vec<P2> {
        match self.kind {
            BoundaryKind::Rectangle => {
                let [a, b] = match self.points.as_slice() {
                    [a, b, ..] => [*a, *b],
                    _ => return Vec::new(),
                };
                let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
                let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
                vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
            }
            BoundaryKind::Spline => closed_spline(&self.points, 16),
        }
    }
}

/// A closed uniform Catmull-Rom curve through `points`, `per` samples per span.
pub fn closed_spline(points: &[P2], per: usize) -> Vec<P2> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(n * per);
    for i in 0..n {
        let p0 = points[(i + n - 1) % n];
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        let p3 = points[(i + 2) % n];
        for s in 0..per {
            let t = s as f64 / per as f64;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    out
}

/// A break (two parallel break lines): the band `lo..hi` of the view's 2D x (vertical break
/// lines) or y (horizontal ones) is removed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Break {
    pub vertical: bool,
    pub lo: f64,
    pub hi: f64,
}

impl Break {
    fn axis(&self) -> usize {
        if self.vertical { 0 } else { 1 }
    }

    fn span(&self) -> (f64, f64) {
        (self.lo.min(self.hi), self.lo.max(self.hi))
    }
}

/// A broken-out section: the material in front of `depth` inside `boundary` is removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokenOut {
    pub boundary: Boundary,
    pub depth: f64,
}

// ---------------------------------------------------------------------------------------------
// Placement through breaks

/// A model 2D point moved through `view`'s breaks: the far side of each band slides up to the
/// paper gap, a point inside a band is squeezed into the gap (so the map is continuous and
/// invertible).
pub fn break_forward(view: &View, p: P2) -> P2 {
    if view.breaks.is_empty() {
        return p;
    }
    let gap = BREAK_GAP / view.scale.factor().max(1e-9);
    let mut bs = view.breaks.clone();
    bs.sort_by(|a, b| b.span().0.total_cmp(&a.span().0));
    let mut q = p;
    for b in bs {
        let (lo, hi) = b.span();
        let w = hi - lo;
        if w <= gap {
            continue;
        }
        let i = b.axis();
        let c = q[i];
        q[i] = if c <= lo {
            c
        } else if c >= hi {
            c - (w - gap)
        } else {
            lo + (c - lo) * gap / w
        };
    }
    q
}

/// The inverse of [`break_forward`].
pub fn break_inverse(view: &View, q: P2) -> P2 {
    if view.breaks.is_empty() {
        return q;
    }
    let gap = BREAK_GAP / view.scale.factor().max(1e-9);
    let mut bs = view.breaks.clone();
    bs.sort_by(|a, b| a.span().0.total_cmp(&b.span().0));
    let mut p = q;
    for b in bs {
        let (lo, hi) = b.span();
        let w = hi - lo;
        if w <= gap {
            continue;
        }
        let i = b.axis();
        let c = p[i];
        p[i] = if c <= lo {
            c
        } else if c >= lo + gap {
            c + (w - gap)
        } else {
            lo + (c - lo) * w / gap
        };
    }
    p
}

// ---------------------------------------------------------------------------------------------
// Clipping

/// What of a view is shown, in its 2D frame: inside every polygon (crop boundary, detail
/// circle) and outside every break band.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Clip {
    pub polygons: Vec<Vec<P2>>,
    /// (axis, lo, hi).
    pub bands: Vec<(usize, f64, f64)>,
}

/// Whether `p` is inside the closed polygon (even-odd).
pub fn inside(poly: &[P2], p: P2) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut c = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if p[0] < x {
                c = !c;
            }
        }
        j = i;
    }
    c
}

/// A circle as a polygon.
pub fn circle_polygon(center: P2, r: f64, n: usize) -> Vec<P2> {
    (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            [center[0] + r * t.cos(), center[1] + r * t.sin()]
        })
        .collect()
}

/// Where segment p–q crosses segment a–b, as a parameter along p–q.
fn seg_cross(p: P2, q: P2, a: P2, b: P2) -> Option<f64> {
    let r = [q[0] - p[0], q[1] - p[1]];
    let s = [b[0] - a[0], b[1] - a[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-18 {
        return None;
    }
    let ap = [a[0] - p[0], a[1] - p[1]];
    let t = (ap[0] * s[1] - ap[1] * s[0]) / den;
    let u = (ap[0] * r[1] - ap[1] * r[0]) / den;
    ((0.0..=1.0).contains(&t) && (-1e-12..=1.0 + 1e-12).contains(&u)).then_some(t)
}

impl Clip {
    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty() && self.bands.is_empty()
    }

    pub fn keeps(&self, p: P2) -> bool {
        self.polygons.iter().all(|poly| inside(poly, p)) && self.bands.iter().all(|&(i, lo, hi)| p[i] <= lo || p[i] >= hi)
    }

    /// The pieces of the polyline that are shown.
    pub fn apply(&self, pts: &[P2]) -> Vec<Vec<P2>> {
        if self.is_empty() {
            return vec![pts.to_vec()];
        }
        let mut out: Vec<Vec<P2>> = Vec::new();
        let mut cur: Vec<P2> = Vec::new();
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            let mut ts = vec![0.0, 1.0];
            for poly in &self.polygons {
                let n = poly.len();
                for i in 0..n {
                    if let Some(t) = seg_cross(p, q, poly[i], poly[(i + 1) % n]) {
                        ts.push(t);
                    }
                }
            }
            for &(i, lo, hi) in &self.bands {
                let d = q[i] - p[i];
                if d.abs() > 1e-15 {
                    for c in [lo, hi] {
                        let t = (c - p[i]) / d;
                        if t > 0.0 && t < 1.0 {
                            ts.push(t);
                        }
                    }
                }
            }
            ts.sort_by(|a, b| a.total_cmp(b));
            ts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
            let at = |t: f64| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
            for k in ts.windows(2) {
                let (t0, t1) = (k[0], k[1]);
                if self.keeps(at((t0 + t1) / 2.0)) {
                    let a = at(t0);
                    if cur.last().is_none_or(|l| (l[0] - a[0]).abs() > 1e-12 || (l[1] - a[1]).abs() > 1e-12) {
                        if cur.len() >= 2 {
                            out.push(std::mem::take(&mut cur));
                        }
                        cur.clear();
                        cur.push(a);
                    }
                    cur.push(at(t1));
                } else if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
        }
        if cur.len() >= 2 {
            out.push(cur);
        }
        out
    }
}

/// What of `view` is shown (crop, detail circle, breaks), in its 2D frame.
pub fn view_clip(view: &View) -> Clip {
    let mut c = Clip::default();
    if let Some(crop) = &view.crop {
        let p = crop.polygon();
        if p.len() >= 3 {
            c.polygons.push(p);
        }
    }
    if let Some(d) = &view.detail {
        c.polygons.push(circle_polygon(d.center, d.radius, 128));
    }
    let gap = BREAK_GAP / view.scale.factor().max(1e-9);
    for b in &view.breaks {
        let (lo, hi) = b.span();
        if hi - lo > gap {
            c.bands.push((b.axis(), lo, hi));
        }
    }
    c
}

// ---------------------------------------------------------------------------------------------
// Hatching

/// The signed area of a closed polygon (counter-clockwise positive).
pub fn signed_area(poly: &[P2]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

/// The area of a hatch region: its loops' signed areas summed (outer loops counter-clockwise,
/// holes clockwise, as `cadrs_core` builds them).
pub fn region_area(loops: &[Vec<P2>]) -> f64 {
    loops.iter().map(|l| signed_area(l)).sum::<f64>().abs()
}

/// Parallel hatch segments filling `loops` (even-odd), `spacing` apart at `angle` (radians),
/// the lines through multiples of `spacing` from the origin so neighbouring regions line up.
pub fn hatch_segments(loops: &[Vec<P2>], spacing: f64, angle: f64) -> Vec<(P2, P2)> {
    let (c, s) = (angle.cos(), angle.sin());
    // Rotate by −angle: the hatch lines become horizontal.
    let to = |p: P2| [c * p[0] + s * p[1], -s * p[0] + c * p[1]];
    let back = |p: P2| [c * p[0] - s * p[1], s * p[0] + c * p[1]];
    let rl: Vec<Vec<P2>> = loops.iter().map(|l| l.iter().map(|p| to(*p)).collect()).collect();
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for l in &rl {
        for p in l {
            lo = lo.min(p[1]);
            hi = hi.max(p[1]);
        }
    }
    let mut out = Vec::new();
    if spacing.is_nan() || spacing <= 0.0 || lo > hi {
        return out;
    }
    let mut k = (lo / spacing).ceil() as i64;
    while (k as f64) * spacing <= hi {
        let y = k as f64 * spacing;
        let mut xs: Vec<f64> = Vec::new();
        for l in &rl {
            let n = l.len();
            for i in 0..n {
                let (a, b) = (l[i], l[(i + 1) % n]);
                if (a[1] > y) != (b[1] > y) {
                    xs.push(a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                }
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks(2) {
            if let [x0, x1] = pair
                && x1 - x0 > 1e-9
            {
                out.push((back([*x0, y]), back([*x1, y])));
            }
        }
        k += 1;
    }
    out
}

/// A view's hatch lines on the sheet (paper mm): the model's hatch region, ANSI31 at 45° on the
/// sheet and 3.175 mm apart on paper, clipped like the view's lines.
pub fn hatch_lines(view: &View, m: &dyn ViewModel) -> Vec<Vec<P2>> {
    let loops = m.hatch();
    if loops.is_empty() {
        return Vec::new();
    }
    let k = view.scale.factor().max(1e-9);
    // 45° on the sheet is 45° − rotation in the view's frame.
    let angle = HATCH_ANGLE.to_radians() - view.rotation;
    let clip = view_clip(view);
    let mut out = Vec::new();
    for (a, b) in hatch_segments(loops, HATCH_SPACING / k, angle) {
        for piece in clip.apply(&[a, b]) {
            out.push(piece.iter().map(|p| view.to_sheet(*p)).collect());
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// New views

/// The letters section and detail views use: A–Z without I, O and Q, then AA, AB…
pub fn letter(i: usize) -> String {
    const L: &[u8] = b"ABCDEFGHJKLMNPRSTUVWXYZ";
    let n = L.len();
    if i < n {
        (L[i] as char).to_string()
    } else {
        let a = L[(i / n - 1) % n] as char;
        let b = L[i % n] as char;
        format!("{a}{b}")
    }
}

/// The first letter no section or detail view of `views` uses.
pub fn next_letter<'a>(views: impl Iterator<Item = &'a View>) -> String {
    let used: Vec<String> = views
        .flat_map(|v| {
            v.section
                .as_ref()
                .map(|s| s.letter.clone())
                .into_iter()
                .chain(v.detail.as_ref().map(|d| d.letter.clone()))
        })
        .collect();
    (0..).map(letter).find(|l| !used.contains(l)).unwrap_or_else(|| "A".into())
}

fn clear_special(v: &mut View) {
    v.annotations.clear();
    v.sketches.clear();
    v.crop = None;
    v.breaks.clear();
    v.section = None;
    v.detail = None;
}

/// The unit normal of the cutting line a→b (in the parent's 2D frame) on the side of `side` (a
/// point of the parent's 2D frame).
fn line_normal(a: P2, b: P2, side: P2) -> Option<P2> {
    let e = [b[0] - a[0], b[1] - a[1]];
    let l = e[0].hypot(e[1]);
    if l < 1e-9 {
        return None;
    }
    let mut n = [-e[1] / l, e[0] / l];
    if (side[0] - a[0]) * n[0] + (side[1] - a[1]) * n[1] < 0.0 {
        n = [-n[0], -n[1]];
    }
    Some(n)
}

/// The section view of `parent` cut along the line a→b (the parent's 2D frame), placed on the
/// side of `cursor` (sheet mm): folded out like an auxiliary view and aligned with the parent,
/// the material between the eye and the cutting plane removed, hidden lines off.
pub fn section_view(parent: &View, a: P2, b: P2, cursor: P2, projection: Projection, letter: &str) -> Option<View> {
    let n = line_normal(a, b, parent.from_sheet(cursor))?;
    let frame = fold_frame(&parent.frame, n, projection);
    // The cutting plane holds the line and the parent's direction of sight: its depth in the
    // new view is that of the line's first point.
    let pf = parent.frame.view_frame();
    let pa = pf.x * a[0] + pf.up() * a[1];
    let dir = nalgebra::Vector3::new(frame.dir[0], frame.dir[1], frame.dir[2]);
    let depth = pa.dot(&dir);
    let mut v = parent.clone();
    v.id = ViewId::new();
    v.kind = ViewKind::Section;
    v.parent = Some(parent.id);
    v.fold = Some(n);
    v.frame = frame;
    v.scale_inherited = true;
    v.align_suppressed = false;
    v.shaded = false;
    v.hidden_lines = false;
    v.broken_out = None;
    clear_special(&mut v);
    v.cut = Some(ViewCut { depth, polygon: Vec::new() });
    v.section = Some(SectionLine { letter: letter.to_string(), a, b });
    v.anchor = aligned_anchor(parent.anchor, rotate(n, parent.rotation), cursor, None);
    v.name = format!("Section {letter}-{letter}");
    v.source_hash = parent.source_hash;
    Some(v)
}

/// Twice `s` (the default detail scale), reduced.
pub fn double(s: Scale) -> Scale {
    let (n, d) = (s.num * 2, s.den.max(1));
    let g = gcd(n, d);
    Scale::new(n / g, d / g)
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

/// The detail view of `parent` inside the circle (`center`, `radius`; the parent's 2D frame)
/// at `scale`, its circle's centre at `cursor` (sheet mm).
pub fn detail_view(parent: &View, center: P2, radius: f64, scale: Scale, cursor: P2, letter: &str) -> View {
    let mut v = parent.clone();
    v.id = ViewId::new();
    v.kind = ViewKind::Detail;
    v.parent = Some(parent.id);
    v.fold = None;
    v.scale = scale;
    v.scale_inherited = false;
    v.align_suppressed = false;
    v.shaded = false;
    clear_special(&mut v);
    v.detail = Some(DetailCircle { letter: letter.to_string(), center, radius });
    let k = scale.factor();
    let off = rotate([center[0] * k, center[1] * k], v.rotation);
    v.anchor = [cursor[0] - off[0], cursor[1] - off[1]];
    v.name = format!("Detail {letter}");
    v
}

/// The frame of a view (for tests).
pub fn frame_of(v: &View) -> Frame3 {
    v.frame
}

// ---------------------------------------------------------------------------------------------
// Decorations

/// What a view draws besides its projected edges and annotations (paper mm).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decor {
    /// Thin continuous lines (hatching, threads, break lines, boundaries).
    pub thin: Vec<Vec<P2>>,
    /// Medium lines (cutting lines).
    pub medium: Vec<Vec<P2>>,
    pub fills: Vec<[P2; 3]>,
    pub texts: Vec<PlacedText>,
}

fn add(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn mul(a: P2, k: f64) -> P2 {
    [a[0] * k, a[1] * k]
}

fn unit(a: P2) -> P2 {
    let l = a[0].hypot(a[1]).max(1e-300);
    [a[0] / l, a[1] / l]
}

/// A text of cap height `h` centred on `c`.
fn centred_text(c: P2, h: f64, text: &str) -> PlacedText {
    let w = crate::annotation::text_width(text) * h;
    PlacedText { pos: [c[0] - w / 2.0, c[1]], height: h, text: text.to_string() }
}

fn arrow(d: &mut Decor, tip: P2, dir: P2, length: f64) {
    let u = unit(dir);
    let base = sub(tip, mul(u, length));
    let n = mul([-u[1], u[0]], length * 0.18);
    d.fills.push([tip, add(base, n), sub(base, n)]);
}

/// The chain pattern of cutting-plane lines (paper mm): long, gap, short, gap, short, gap.
pub const CUTTING: [f64; 6] = [10.0, 1.5, 2.0, 1.5, 2.0, 1.5];

/// The view's sheet bounds from its model (clipped and through its breaks), if it has lines.
pub fn shown_bounds(view: &View, m: &dyn ViewModel) -> Option<(P2, P2)> {
    let clip = view_clip(view);
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    for e in &m.projection().edges {
        let pts: Vec<P2> = e.points.iter().map(|p| [p.x, p.y]).collect();
        for piece in clip.apply(&pts) {
            for p in piece {
                let q = view.to_sheet(p);
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

/// The distance from `p` to segment `a`–`b`.
fn seg_dist(p: P2, a: P2, b: P2) -> f64 {
    let d = sub(b, a);
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 > 1e-18 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let q = add(a, mul(d, t));
    (p[0] - q[0]).hypot(p[1] - q[1])
}

/// Polyline `pts` split into steps of at most `step`.
fn densify(pts: &[P2], step: f64) -> Vec<P2> {
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = (((b[0] - a[0]).hypot(b[1] - a[1])) / step.max(1e-9)).ceil().max(1.0) as usize;
        for i in 0..n {
            out.push(add(a, mul(sub(b, a), i as f64 / n as f64)));
        }
    }
    if let Some(l) = pts.last() {
        out.push(*l);
    }
    out
}

/// The runs of polyline `pts` that lie on or inside the loops `material` (within `tol`).
pub fn on_material(pts: &[P2], material: &[Vec<P2>], tol: f64, step: f64) -> Vec<Vec<P2>> {
    let near = |p: P2| {
        material.iter().any(|l| inside(l, p) || l.windows(2).chain(std::iter::once(&[*l.last().unwrap_or(&p), *l.first().unwrap_or(&p)][..])).any(|w| seg_dist(p, w[0], w[1]) <= tol))
    };
    let mut out: Vec<Vec<P2>> = Vec::new();
    let mut cur: Vec<P2> = Vec::new();
    for p in densify(pts, step) {
        if near(p) {
            cur.push(p);
        } else if cur.len() > 1 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.clear();
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// The runs of polyline `pts` inside the circle (`c`, `r`), sampled every `step`.
fn inside_circle(pts: &[P2], c: P2, r: f64, step: f64) -> Vec<Vec<P2>> {
    let mut out: Vec<Vec<P2>> = Vec::new();
    let mut cur: Vec<P2> = Vec::new();
    for p in densify(pts, step) {
        if (p[0] - c[0]).hypot(p[1] - c[1]) <= r {
            cur.push(p);
        } else if cur.len() > 1 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.clear();
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// Where view labels shouldn't go on `sheet`: its title block.
pub fn label_avoid(sheet: &crate::Sheet) -> Vec<(P2, P2)> {
    if !sheet.title_block {
        return Vec::new();
    }
    let r = crate::title_block::placement(crate::standard::frame(sheet.format).inner, sheet.format.size);
    vec![(r.min, r.max)]
}

/// The decorations of `view` (one of `sheet_views`, the views of its sheet), with its model
/// `m` if it has been generated. A section or detail label goes under its view, or above it
/// where it would cover one of `avoid` (the title block).
pub fn view_decor(style: &DrawingStyle, sheet_views: &[View], view: &View, m: Option<&dyn ViewModel>, avoid: &[(P2, P2)]) -> Decor {
    let mut d = Decor::default();
    let h = style.dim_text_height;
    if let Some(m) = m {
        d.thin.extend(hatch_lines(view, m));
        thread_marks(&mut d, view, m);
        // Break lines: a thin line with a zig-zag in its middle at each edge of the gap,
        // across the view.
        let k = view.scale.factor().max(1e-9);
        let gap = BREAK_GAP / k;
        for b in &view.breaks {
            let (b0, b1) = b.span();
            if b1 - b0 <= gap {
                continue;
            }
            let (i, j) = (b.axis(), 1 - b.axis());
            let (mut t0, mut t1) = (f64::MAX, f64::MIN);
            for e in &m.projection().edges {
                for p in &e.points {
                    let v = [p.x, p.y][j];
                    t0 = t0.min(v);
                    t1 = t1.max(v);
                }
            }
            if t0 > t1 {
                continue;
            }
            let margin = 2.5 / k;
            for c in [b0, b1] {
                let at = |t: f64| {
                    let mut p = [0.0; 2];
                    p[i] = c;
                    p[j] = t;
                    view.to_sheet(p)
                };
                let (p0, p1) = (at(t0 - margin), at(t1 + margin));
                let mid = mul(add(p0, p1), 0.5);
                let along = unit(sub(p1, p0));
                let across = [-along[1], along[0]];
                let z = 1.6;
                d.thin.push(vec![
                    p0,
                    sub(mid, mul(along, z)),
                    add(mid, mul(across, z)),
                    sub(mid, mul(across, z)),
                    add(mid, mul(along, z)),
                    p1,
                ]);
            }
        }
        let bounds = shown_bounds(view, m);
        // Labels under section and detail views.
        let label_lines: Vec<String> = match (&view.section, &view.detail) {
            (Some(s), _) => vec![format!("SECTION {0}-{0}", s.letter)],
            (_, Some(dt)) => vec![format!("DETAIL {}", dt.letter), format!("SCALE {}", view.scale.label())],
            _ => Vec::new(),
        };
        if !label_lines.is_empty()
            && let Some((lo, hi)) = bounds.or_else(|| {
                view.detail.as_ref().map(|dt| {
                    let c = view.to_sheet(dt.center);
                    let r = dt.radius * view.scale.factor();
                    ([c[0] - r, c[1] - r], [c[0] + r, c[1] + r])
                })
            })
        {
            let cx = (lo[0] + hi[0]) / 2.0;
            let n = label_lines.len() as f64;
            let block = 1.25 * h + 1.9 * h * (n - 1.0);
            let w = label_lines.iter().map(|l| crate::annotation::text_width(l) * 1.25 * h).fold(0.0, f64::max);
            let below_top = lo[1] - 2.2 * h + 0.7 * h;
            let below = ([cx - w / 2.0, below_top - block - 0.5 * h], [cx + w / 2.0, below_top]);
            let overlaps = |(a, b): (P2, P2), (c, e): &(P2, P2)| a[0] < e[0] && b[0] > c[0] && a[1] < e[1] && b[1] > c[1];
            let mut y = if avoid.iter().any(|r| overlaps(below, r)) { hi[1] + 2.0 * h + block - 1.25 * h } else { lo[1] - 2.2 * h };
            for (k, l) in label_lines.iter().enumerate() {
                let hh = if k == 0 { h * 1.25 } else { h };
                d.texts.push(centred_text([cx, y], hh, l));
                y -= 1.9 * h;
            }
        }
    }
    // A detail view's boundary: a thin circle.
    if let Some(dt) = &view.detail {
        let c = view.to_sheet(dt.center);
        let mut pts = circle_polygon(c, dt.radius * view.scale.factor(), 128);
        if let Some(f) = pts.first().copied() {
            pts.push(f);
        }
        d.thin.push(pts);
    }
    // Broken-out boundaries: a thin freehand line, only where it crosses the part (P3C.8's
    // delta: it was drawn round the air too): the pieces on the cut faces' outline.
    if let Some(bo) = &view.broken_out {
        let mut poly = bo.boundary.polygon();
        if let Some(f) = poly.first().copied() {
            poly.push(f);
        }
        let clip = view_clip(view);
        let hatch: &[Vec<P2>] = m.map(|m| m.hatch()).unwrap_or(&[]);
        let k = view.scale.factor().max(1e-9);
        for piece in clip.apply(&poly) {
            let runs = if hatch.is_empty() { vec![piece] } else { on_material(&piece, hatch, 0.3 / k, 0.5 / k) };
            for run in runs {
                d.thin.push(run.iter().map(|p| view.to_sheet(*p)).collect());
            }
        }
    }
    // A detail view carries its parent's centermarks and centerlines inside its circle (P3C.8's
    // delta).
    if let (Some(dt), Some(m)) = (&view.detail, m)
        && let Some(parent) = view.parent.and_then(|p| sheet_views.iter().find(|v| v.id == p))
    {
        use crate::annotation::AnnotationKind as K;
        let c = view.to_sheet(dt.center);
        let r = dt.radius * view.scale.factor();
        for a in parent.annotations.iter().filter(|a| matches!(a.kind, K::Centermark(_) | K::Centerline(_) | K::CircleCenterline(_))) {
            let Some(g) = crate::annotation::annotation_graphics(style, view, m, a) else { continue };
            for stroke in &g.strokes {
                for run in inside_circle(stroke, c, r, 0.3) {
                    d.thin.push(run);
                }
            }
        }
    }
    // The cutting lines and detail circles of this view's children.
    let parent_bounds = m.and_then(|m| shown_bounds(view, m));
    for c in sheet_views.iter().filter(|c| c.parent == Some(view.id)) {
        if let Some(s) = &c.section {
            cutting_line(&mut d, style, view, c, s, parent_bounds);
        }
        if let Some(dt) = &c.detail {
            let center = view.to_sheet(dt.center);
            let r = dt.radius * view.scale.factor();
            let mut pts = circle_polygon(center, r, 96);
            if let Some(f) = pts.first().copied() {
                pts.push(f);
            }
            d.thin.push(pts);
            let q = add(center, mul([std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2], r + 1.6 * h));
            d.texts.push(centred_text(q, h * 1.25, &dt.letter));
        }
    }
    d
}

/// How far past the parent's outline a cutting line's thick ends start (sheet mm).
const CUT_CLEAR: f64 = 3.0;

/// The section line of child `c` on its parent `view`: a chain line with thick ends, arrows
/// pointing in the child's direction of sight, and the letters beside the arrows' tips (on the
/// outside). The
/// line runs across the parent's outline `bounds` and a gap beyond it (P3C.8's delta: its ends
/// and letters sat on the view and its dimensions).
fn cutting_line(d: &mut Decor, style: &DrawingStyle, view: &View, c: &View, s: &SectionLine, bounds: Option<(P2, P2)>) {
    let h = style.dim_text_height;
    let (mut a, mut b) = (view.to_sheet(s.a), view.to_sheet(s.b));
    let along = unit(sub(b, a));
    if let Some((lo, hi)) = bounds {
        let corners = [lo, hi, [lo[0], hi[1]], [hi[0], lo[1]]];
        let t = |p: P2| (p[0] - a[0]) * along[0] + (p[1] - a[1]) * along[1];
        let tmin = corners.iter().map(|p| t(*p)).fold(f64::MAX, f64::min);
        let tmax = corners.iter().map(|p| t(*p)).fold(f64::MIN, f64::max);
        let len = t(b);
        let (t0, t1) = ((tmin - CUT_CLEAR).min(0.0), (tmax + CUT_CLEAR).max(len));
        let a0 = a;
        a = add(a0, mul(along, t0));
        b = add(a0, mul(along, t1));
    }
    // The child's direction of sight seen in the parent, on the sheet.
    let pf = view.frame.view_frame();
    let dir = nalgebra::Vector3::new(c.frame.dir[0], c.frame.dir[1], c.frame.dir[2]);
    let s2 = rotate([dir.dot(&pf.x), dir.dot(&pf.up())], view.rotation);
    let sight = unit(s2);
    for dash in dashes(&[a, b], &CUTTING) {
        d.medium.push(dash);
    }
    let leg = 5.0;
    let arrow_len = style.dim_arrow_length * 1.4;
    for (end, out) in [(a, mul(along, -1.0)), (b, along)] {
        // A thick end stroke, the leg perpendicular to it and the arrow at its end.
        d.medium.push(vec![end, add(end, mul(out, 3.0))]);
        let root = add(end, mul(out, 3.0));
        let tip = add(root, mul(sight, leg));
        d.medium.push(vec![root, tip]);
        arrow(d, tip, sight, arrow_len);
        // The letter beside the arrow's tip, on the outside.
        let lp = add(tip, mul(out, 1.25 * h));
        d.texts.push(centred_text(lp, h * 1.25, &s.letter));
    }
}

/// Thread marks of the model's tapped holes in `view` (when the view shows threads).
fn thread_marks(d: &mut Decor, view: &View, m: &dyn ViewModel) {
    if !view.threads {
        return;
    }
    let f = view.frame.view_frame();
    let clip = view_clip(view);
    let sectioned = !m.hatch().is_empty();
    let emit = |pts: Vec<P2>, dashed: bool, d: &mut Decor| {
        for piece in clip.apply(&pts) {
            let sheet: Vec<P2> = piece.iter().map(|p| view.to_sheet(*p)).collect();
            if dashed {
                d.thin.extend(dashes(&sheet, &[2.0, 0.8]));
            } else {
                d.thin.push(sheet);
            }
        }
    };
    for t in m.threads() {
        let ThreadInfo { center, axis, major, length, .. } = *t;
        let ax = nalgebra::Vector3::new(axis[0], axis[1], axis[2]);
        let c3 = nalgebra::Point3::new(center[0], center[1], center[2]);
        let c2 = f.to_2d(&c3);
        let along = ax.dot(&f.dir);
        let r = major / 2.0;
        if along.abs() > 0.99 {
            // End on: a 3/4 circle of the major diameter (the gap in the upper right quadrant).
            let pts: Vec<P2> = (0..=54)
                .map(|i| {
                    let t = std::f64::consts::FRAC_PI_2 + 1.5 * std::f64::consts::PI * i as f64 / 54.0;
                    [c2.x + r * t.cos(), c2.y + r * t.sin()]
                })
                .collect();
            // Seen from its entry (the axis runs away from the eye) or through: visible.
            emit(pts, along < 0.0 && !t.through, d);
        } else if along.abs() < 0.01 {
            let a2 = f.dir_2d(&ax);
            let u = unit([a2.x, a2.y]);
            let n = [-u[1], u[0]];
            let c = [c2.x, c2.y];
            let e = add(c, mul(u, length));
            // Visible where a cut opens it up (a section, or a broken-out section around it).
            let opened = sectioned && view.effective_cut().is_some_and(|c| c.polygon.len() < 3 || inside(&c.polygon, [c2.x, c2.y]));
            let hidden = !opened;
            if hidden && !view.hidden_lines {
                continue;
            }
            for sgn in [1.0, -1.0] {
                let o = mul(n, sgn * r);
                emit(vec![add(c, o), add(e, o)], hidden, d);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectRef;
    use crate::view::NamedView;

    fn base() -> View {
        View::base(ObjectRef { element: uuid::Uuid::nil(), part: None }, NamedView::Front, Scale::new(1, 2), [100.0, 80.0])
    }

    #[test]
    fn hatch_of_a_square_ring_has_its_area_and_lines() {
        let outer = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let hole = vec![[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]];
        let loops = vec![outer, hole];
        assert!((region_area(&loops) - 84.0).abs() < 1e-9);
        let segs = hatch_segments(&loops, 1.0, 0.0);
        // Lines at y = 0..10: those through the hole are split in two.
        assert!(segs.iter().all(|(a, b)| (a[1] - b[1]).abs() < 1e-9));
        let through_hole = segs.iter().filter(|(a, _)| (a[1] - 5.0).abs() < 1e-9).count();
        assert_eq!(through_hole, 2);
    }

    #[test]
    fn breaks_map_and_invert() {
        let mut v = base();
        v.breaks.push(Break { vertical: true, lo: 20.0, hi: 120.0 });
        let gap = BREAK_GAP * 2.0; // 1:2
        let far = break_forward(&v, [150.0, 3.0]);
        assert!((far[0] - (150.0 - (100.0 - gap))).abs() < 1e-9);
        for x in [-5.0, 20.0, 50.0, 119.0, 150.0] {
            let q = break_forward(&v, [x, 1.0]);
            let p = break_inverse(&v, q);
            assert!((p[0] - x).abs() < 1e-9, "{x}");
        }
        // Through the sheet transform too.
        let s = v.to_sheet([150.0, 3.0]);
        let back = v.from_sheet(s);
        assert!((back[0] - 150.0).abs() < 1e-9 && (back[1] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn clipping_keeps_the_inside_of_a_circle_and_drops_bands() {
        let c = Clip { polygons: vec![circle_polygon([0.0, 0.0], 10.0, 256)], bands: Vec::new() };
        let pieces = c.apply(&[[-20.0, 0.0], [20.0, 0.0]]);
        assert_eq!(pieces.len(), 1);
        assert!((pieces[0][0][0] + 10.0).abs() < 1e-3 && (pieces[0].last().unwrap()[0] - 10.0).abs() < 1e-3);
        let b = Clip { polygons: Vec::new(), bands: vec![(0, -5.0, 5.0)] };
        let pieces = b.apply(&[[-20.0, 1.0], [20.0, 1.0]]);
        assert_eq!(pieces.len(), 2);
    }

    #[test]
    fn letters_skip_i_o_q() {
        let l: Vec<String> = (0..10).map(letter).collect();
        assert_eq!(l.join(""), "ABCDEFGHJK");
        assert!(!(0..23).map(letter).any(|s| s == "I" || s == "O" || s == "Q"));
        assert_eq!(letter(23), "AA");
    }

    #[test]
    fn section_views_fold_out_and_cut_at_the_line() {
        let p = base();
        // A vertical cutting line at x = 10 on the Front view, the section placed to the right.
        let s = section_view(&p, [10.0, -50.0], [10.0, 50.0], [200.0, 80.0], Projection::Third, "A").unwrap();
        assert_eq!(NamedView::of(&s.frame), Some(NamedView::Right));
        // Seen from +x looking along −x: depth of the plane x = 10 is −10.
        assert!((s.cut.as_ref().unwrap().depth + 10.0).abs() < 1e-9);
        assert_eq!(s.name, "Section A-A");
        assert!(s.aligned() && !s.hidden_lines);
        let d = detail_view(&p, [5.0, 5.0], 8.0, double(p.scale), [60.0, 60.0], "B");
        assert_eq!(d.scale, Scale::new(1, 1));
        let c = d.to_sheet([5.0, 5.0]);
        assert!((c[0] - 60.0).abs() < 1e-9 && (c[1] - 60.0).abs() < 1e-9);
    }
}
