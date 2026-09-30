//! Sheet sketch geometry (P3C.7, D2.4, X13): lines and splines drawn on the sheet itself (not
//! attached to a view), DXF/DWG drawings inserted as blocks, and images.
//!
//! Every item lives on a sheet ([`crate::Sheet::sketch`]) in sheet millimetres and is added,
//! changed and deleted through [`crate::DrawingOp`]s, so each edit is one undo step.
//!
//! - A [`ItemKind::Line`] has two ends; a [`ItemKind::Spline`] passes through its fit points (a
//!   C2 cubic through them, [`spline_beziers`]). Their grips are their points.
//! - A [`Block`] is an imported DXF: its entities in block coordinates (mm, the file's units
//!   converted), placed at `at` and scaled by `scale`. It moves and deletes as one item.
//! - A [`SheetImage`] keeps the file's bytes (PNG or JPEG) and its size on the sheet; its corner
//!   grips resize it about the opposite corner and keep its aspect ratio ([`resize_image`]).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type P2 = [f64; 2];

/// Identifies a sheet sketch item within its sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemId(pub Uuid);

impl ItemId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for ItemId {
    fn default() -> Self {
        Self::new()
    }
}

/// One item of a sheet's sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchItem {
    pub id: ItemId,
    pub kind: ItemKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ItemKind {
    Line { a: P2, b: P2 },
    /// Passes through `points` (at least two).
    Spline { points: Vec<P2> },
    Block(Block),
    Image(SheetImage),
}

impl SketchItem {
    pub fn new(kind: ItemKind) -> Self {
        Self { id: ItemId::new(), kind }
    }

    /// What the undo label calls it.
    pub fn noun(&self) -> &'static str {
        match self.kind {
            ItemKind::Line { .. } => "line",
            ItemKind::Spline { .. } => "spline",
            ItemKind::Block(_) => "DXF",
            ItemKind::Image(_) => "image",
        }
    }
}

/// An imported drawing (DXF, or DWG through a converter).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    /// The file's name.
    pub name: String,
    /// Where the block's origin is on the sheet.
    pub at: P2,
    #[serde(default = "one")]
    pub scale: f64,
    pub entities: Vec<Entity>,
}

fn one() -> f64 {
    1.0
}

/// A 2D entity of an imported drawing (block coordinates, mm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Entity {
    Line { a: P2, b: P2 },
    /// Counter-clockwise from `start` to `end` (degrees).
    Arc { center: P2, radius: f64, start: f64, end: f64 },
    Circle { center: P2, radius: f64 },
    /// `bulges[i]` bends the segment from point `i` to the next (tan of a quarter of its angle).
    Polyline {
        points: Vec<P2>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bulges: Vec<f64>,
        closed: bool,
    },
    Spline {
        degree: usize,
        knots: Vec<f64>,
        control: Vec<P2>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        weights: Vec<f64>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fit: Vec<P2>,
    },
    /// `at` is the left end of the baseline; `height` the cap height (mm).
    Text { at: P2, height: f64, text: String, rotation: f64 },
    /// A filled polygon (DXF SOLID).
    Solid { points: Vec<P2> },
}

/// An image on the sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetImage {
    /// The file's name.
    pub name: String,
    /// The bottom-left corner.
    pub at: P2,
    pub width: f64,
    pub height: f64,
    /// Its size in pixels.
    pub pixels: (u32, u32),
    /// The file's bytes (PNG or JPEG), stored as base64.
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

impl SheetImage {
    /// Width over height, from the pixels.
    pub fn aspect(&self) -> f64 {
        self.pixels.0.max(1) as f64 / self.pixels.1.max(1) as f64
    }

    /// The corners: bottom-left, bottom-right, top-right, top-left.
    pub fn corners(&self) -> [P2; 4] {
        let [x, y] = self.at;
        [[x, y], [x + self.width, y], [x + self.width, y + self.height], [x, y + self.height]]
    }
}

/// An image `width` mm wide with its file's aspect ratio, its bottom-left corner at `at`.
pub fn image_item(name: &str, data: Vec<u8>, pixels: (u32, u32), at: P2, width: f64) -> SheetImage {
    let aspect = pixels.0.max(1) as f64 / pixels.1.max(1) as f64;
    SheetImage { name: name.into(), at, width, height: width / aspect, pixels, data }
}

/// The smallest an image gets (mm, its shorter side).
pub const MIN_IMAGE: f64 = 1.0;

/// The image after corner `corner` (see [`SheetImage::corners`]) was dragged to `to`: the
/// opposite corner stays, and the aspect ratio is kept (the larger of the two drags wins).
pub fn resize_image(img: &SheetImage, corner: usize, to: P2) -> SheetImage {
    let c = img.corners();
    let fixed = c[(corner + 2) % 4];
    let aspect = img.aspect();
    let (dx, dy) = ((to[0] - fixed[0]).abs(), (to[1] - fixed[1]).abs());
    let mut w = dx.max(dy * aspect);
    let min_w = if aspect >= 1.0 { MIN_IMAGE * aspect } else { MIN_IMAGE };
    w = w.max(min_w);
    let h = w / aspect;
    // The moved corner stays on its side of the fixed one.
    let sx = if corner == 1 || corner == 2 { 1.0 } else { -1.0 };
    let sy = if corner >= 2 { 1.0 } else { -1.0 };
    let other = [fixed[0] + sx * w, fixed[1] + sy * h];
    let mut out = img.clone();
    out.at = [fixed[0].min(other[0]), fixed[1].min(other[1])];
    out.width = w;
    out.height = h;
    out
}

// ---------------------------------------------------------------------------------------------
// Splines

/// The cubic Bézier segments `[p0, c1, c2, p1]` of the C2 spline through `points` (uniform
/// parameters, natural ends). Two points make a straight segment.
pub fn spline_beziers(points: &[P2]) -> Vec<[P2; 4]> {
    let n = points.len().saturating_sub(1);
    if n == 0 {
        return Vec::new();
    }
    let k = points;
    let lerp = |a: P2, b: P2, t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    if n == 1 {
        return vec![[k[0], lerp(k[0], k[1], 1.0 / 3.0), lerp(k[0], k[1], 2.0 / 3.0), k[1]]];
    }
    // The first control points solve a tridiagonal system (the natural spline in Bézier form).
    let mut a = vec![0.0; n];
    let mut b = vec![0.0; n];
    let mut c = vec![0.0; n];
    let mut r = vec![[0.0; 2]; n];
    for i in 0..n {
        let (ai, bi, ci, ri) = if i == 0 {
            (0.0, 2.0, 1.0, [k[0][0] + 2.0 * k[1][0], k[0][1] + 2.0 * k[1][1]])
        } else if i == n - 1 {
            (2.0, 7.0, 0.0, [8.0 * k[n - 1][0] + k[n][0], 8.0 * k[n - 1][1] + k[n][1]])
        } else {
            (1.0, 4.0, 1.0, [4.0 * k[i][0] + 2.0 * k[i + 1][0], 4.0 * k[i][1] + 2.0 * k[i + 1][1]])
        };
        a[i] = ai;
        b[i] = bi;
        c[i] = ci;
        r[i] = ri;
    }
    for i in 1..n {
        let m = a[i] / b[i - 1];
        b[i] -= m * c[i - 1];
        r[i] = [r[i][0] - m * r[i - 1][0], r[i][1] - m * r[i - 1][1]];
    }
    let mut p1 = vec![[0.0; 2]; n];
    p1[n - 1] = [r[n - 1][0] / b[n - 1], r[n - 1][1] / b[n - 1]];
    for i in (0..n - 1).rev() {
        p1[i] = [(r[i][0] - c[i] * p1[i + 1][0]) / b[i], (r[i][1] - c[i] * p1[i + 1][1]) / b[i]];
    }
    (0..n)
        .map(|i| {
            let p2 = if i < n - 1 {
                [2.0 * k[i + 1][0] - p1[i + 1][0], 2.0 * k[i + 1][1] - p1[i + 1][1]]
            } else {
                [(k[n][0] + p1[n - 1][0]) / 2.0, (k[n][1] + p1[n - 1][1]) / 2.0]
            };
            [k[i], p1[i], p2, k[i + 1]]
        })
        .collect()
}

/// A point of a cubic Bézier.
pub fn bezier_at(s: &[P2; 4], t: f64) -> P2 {
    let u = 1.0 - t;
    let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    [
        b0 * s[0][0] + b1 * s[1][0] + b2 * s[2][0] + b3 * s[3][0],
        b0 * s[0][1] + b1 * s[1][1] + b2 * s[2][1] + b3 * s[3][1],
    ]
}

/// The spline through `points` as a polyline (`per` points per segment).
pub fn spline_polyline(points: &[P2], per: usize) -> Vec<P2> {
    let segs = spline_beziers(points);
    let mut out = Vec::with_capacity(segs.len() * per + 1);
    if let Some(s) = segs.first() {
        out.push(s[0]);
    }
    for s in &segs {
        for i in 1..=per {
            out.push(bezier_at(s, i as f64 / per as f64));
        }
    }
    out
}

/// The spline through `points` as a clamped cubic B-spline (DXF SPLINE): its knots and control
/// points (the Bézier segments joined, interior knots of multiplicity 3).
pub fn spline_nurbs(points: &[P2]) -> (Vec<f64>, Vec<P2>) {
    let segs = spline_beziers(points);
    let n = segs.len();
    let mut control = Vec::with_capacity(3 * n + 1);
    for (i, s) in segs.iter().enumerate() {
        if i == 0 {
            control.push(s[0]);
        }
        control.extend([s[1], s[2], s[3]]);
    }
    let mut knots = vec![0.0; 4];
    for i in 1..n {
        knots.extend([i as f64; 3]);
    }
    knots.extend([n as f64; 4]);
    (knots, control)
}

/// Evaluates a (rational) B-spline of `degree` at `t` (de Boor).
pub fn bspline_at(degree: usize, knots: &[f64], control: &[P2], weights: &[f64], t: f64) -> Option<P2> {
    let p = degree;
    let n = control.len();
    if n == 0 || knots.len() < n + p + 1 {
        return None;
    }
    let w = |i: usize| weights.get(i).copied().unwrap_or(1.0);
    // The span: knots[k] <= t < knots[k+1], p <= k < n.
    let mut k = p;
    while k + 1 < n && knots[k + 1] <= t {
        k += 1;
    }
    let mut d: Vec<[f64; 3]> = (0..=p)
        .map(|j| {
            let i = (j + k).saturating_sub(p).min(n - 1);
            let wi = w(i);
            [control[i][0] * wi, control[i][1] * wi, wi]
        })
        .collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let den = knots[i + p + 1 - r] - knots[i];
            let alpha = if den.abs() < 1e-300 { 0.0 } else { (t - knots[i]) / den };
            let prev = d[j - 1];
            for (c, v) in d[j].iter_mut().enumerate() {
                *v = (1.0 - alpha) * prev[c] + alpha * *v;
            }
        }
    }
    let q = d[p];
    (q[2].abs() > 1e-300).then(|| [q[0] / q[2], q[1] / q[2]])
}

/// A B-spline as a polyline (`per` points per knot span).
pub fn bspline_polyline(degree: usize, knots: &[f64], control: &[P2], weights: &[f64], per: usize) -> Vec<P2> {
    let n = control.len();
    if n < 2 || knots.len() < n + degree + 1 {
        return control.to_vec();
    }
    let (t0, t1) = (knots[degree], knots[n]);
    let spans = knots[degree..=n].windows(2).filter(|w| w[1] > w[0]).count().max(1);
    let steps = spans * per;
    (0..=steps)
        .filter_map(|i| {
            let t = t0 + (t1 - t0) * i as f64 / steps as f64;
            // The last point is exactly the end.
            if i == steps { bspline_at(degree, knots, control, weights, t1 - 1e-12 * (t1 - t0).abs().max(1.0)) } else { bspline_at(degree, knots, control, weights, t) }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Geometry of items

/// An arc from `start` to `end` degrees counter-clockwise, as a polyline.
pub fn arc_polyline(center: P2, radius: f64, start: f64, end: f64) -> Vec<P2> {
    let mut sweep = (end - start).rem_euclid(360.0);
    if sweep < 1e-9 {
        sweep = 360.0;
    }
    let n = ((sweep / 6.0).ceil() as usize).max(2);
    (0..=n)
        .map(|i| {
            let a = (start + sweep * i as f64 / n as f64).to_radians();
            [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
        })
        .collect()
}

/// A polyline with bulges, as a plain polyline.
pub fn bulge_polyline(points: &[P2], bulges: &[f64], closed: bool) -> Vec<P2> {
    let mut out = Vec::new();
    let n = points.len();
    if n == 0 {
        return out;
    }
    let segs = if closed { n } else { n - 1 };
    out.push(points[0]);
    for i in 0..segs {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        if bulge.abs() < 1e-12 {
            out.push(b);
            continue;
        }
        // The arc's sweep is 4·atan(bulge), positive counter-clockwise.
        let theta = 4.0 * bulge.atan();
        let chord = [b[0] - a[0], b[1] - a[1]];
        let len = chord[0].hypot(chord[1]);
        if len < 1e-12 {
            continue;
        }
        let radius = len / (2.0 * (theta / 2.0).sin());
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let h = radius * (theta / 2.0).cos();
        let nrm = [-chord[1] / len, chord[0] / len];
        let c = [mid[0] + nrm[0] * h, mid[1] + nrm[1] * h];
        let a0 = (a[1] - c[1]).atan2(a[0] - c[0]);
        let steps = ((theta.abs().to_degrees() / 6.0).ceil() as usize).max(2);
        let r = radius.abs();
        for s in 1..=steps {
            let ang = a0 + theta * s as f64 / steps as f64;
            out.push([c[0] + r * ang.cos(), c[1] + r * ang.sin()]);
        }
    }
    out
}

/// A block point on the sheet.
pub fn block_to_sheet(b: &Block, p: P2) -> P2 {
    [b.at[0] + b.scale * p[0], b.at[1] + b.scale * p[1]]
}

/// The polylines of an entity (block coordinates).
pub fn entity_polylines(e: &Entity) -> Vec<Vec<P2>> {
    match e {
        Entity::Line { a, b } => vec![vec![*a, *b]],
        Entity::Arc { center, radius, start, end } => vec![arc_polyline(*center, *radius, *start, *end)],
        Entity::Circle { center, radius } => vec![arc_polyline(*center, *radius, 0.0, 360.0)],
        Entity::Polyline { points, bulges, closed } => vec![bulge_polyline(points, bulges, *closed)],
        Entity::Spline { degree, knots, control, weights, .. } => vec![bspline_polyline(*degree, knots, control, weights, 16)],
        Entity::Solid { points } => {
            let mut p = points.clone();
            if let Some(f) = points.first() {
                p.push(*f);
            }
            vec![p]
        }
        Entity::Text { .. } => Vec::new(),
    }
}

/// The bounding box `(min, max)` of some points.
pub fn bounds(points: impl IntoIterator<Item = P2>) -> Option<(P2, P2)> {
    let mut it = points.into_iter();
    let f = it.next()?;
    Some(it.fold((f, f), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])))
}

/// The extent of a text entity (block coordinates): its baseline box.
fn text_corners(at: P2, height: f64, text: &str, rotation: f64) -> [P2; 4] {
    let w = crate::annotation::text_width(text) * height;
    let (s, c) = rotation.to_radians().sin_cos();
    let q = |x: f64, y: f64| [at[0] + x * c - y * s, at[1] + x * s + y * c];
    [q(0.0, -0.25 * height), q(w, -0.25 * height), q(w, 1.1 * height), q(0.0, 1.1 * height)]
}

/// The entities' bounding box (block coordinates).
pub fn entities_bounds(es: &[Entity]) -> Option<(P2, P2)> {
    bounds(es.iter().flat_map(|e| match e {
        Entity::Text { at, height, text, rotation } => text_corners(*at, *height, text, *rotation).to_vec(),
        e => entity_polylines(e).into_iter().flatten().collect(),
    }))
}

/// How an item is drawn and picked: its polylines on the sheet (an image's frame).
pub fn item_polylines(item: &SketchItem) -> Vec<Vec<P2>> {
    match &item.kind {
        ItemKind::Line { a, b } => vec![vec![*a, *b]],
        ItemKind::Spline { points } => vec![spline_polyline(points, 24)],
        ItemKind::Block(b) => b
            .entities
            .iter()
            .flat_map(entity_polylines)
            .map(|pl| pl.into_iter().map(|p| block_to_sheet(b, p)).collect())
            .collect(),
        ItemKind::Image(img) => {
            let c = img.corners();
            vec![vec![c[0], c[1], c[2], c[3], c[0]]]
        }
    }
}

/// The item's box on the sheet.
pub fn item_bounds(item: &SketchItem) -> Option<(P2, P2)> {
    match &item.kind {
        ItemKind::Block(b) => {
            let (lo, hi) = entities_bounds(&b.entities)?;
            Some((block_to_sheet(b, lo), block_to_sheet(b, hi)))
        }
        _ => bounds(item_polylines(item).into_iter().flatten()),
    }
}

fn seg_distance(p: P2, a: P2, b: P2) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 < 1e-300 { 0.0 } else { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0) };
    (p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t)
}

/// Distance from a sheet point to the item (0 inside an image or a block's box).
pub fn item_distance(item: &SketchItem, p: P2) -> f64 {
    if matches!(item.kind, ItemKind::Image(_) | ItemKind::Block(_))
        && let Some((lo, hi)) = item_bounds(item)
        && p[0] >= lo[0]
        && p[0] <= hi[0]
        && p[1] >= lo[1]
        && p[1] <= hi[1]
    {
        return 0.0;
    }
    item_polylines(item)
        .iter()
        .flat_map(|pl| pl.windows(2).map(|w| seg_distance(p, w[0], w[1])).collect::<Vec<_>>())
        .fold(f64::MAX, f64::min)
}

/// A grip of a selected item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemGrip {
    /// A line's end or a spline's fit point.
    Point(usize),
    /// An image's corner (see [`SheetImage::corners`]).
    Corner(usize),
}

/// The grips of a selected item.
pub fn item_grips(item: &SketchItem) -> Vec<(P2, ItemGrip)> {
    match &item.kind {
        ItemKind::Line { a, b } => vec![(*a, ItemGrip::Point(0)), (*b, ItemGrip::Point(1))],
        ItemKind::Spline { points } => points.iter().enumerate().map(|(i, p)| (*p, ItemGrip::Point(i))).collect(),
        ItemKind::Block(_) => Vec::new(),
        ItemKind::Image(img) => img.corners().iter().enumerate().map(|(i, p)| (*p, ItemGrip::Corner(i))).collect(),
    }
}

/// The item moved by `d`.
pub fn moved(item: &SketchItem, d: P2) -> SketchItem {
    let m = |p: &P2| [p[0] + d[0], p[1] + d[1]];
    let mut out = item.clone();
    match &mut out.kind {
        ItemKind::Line { a, b } => {
            *a = m(a);
            *b = m(b);
        }
        ItemKind::Spline { points } => {
            for p in points.iter_mut() {
                *p = m(p);
            }
        }
        ItemKind::Block(b) => b.at = m(&b.at),
        ItemKind::Image(img) => img.at = m(&img.at),
    }
    out
}

/// The item after grip `grip` was dragged to `to`.
pub fn drag_grip(item: &SketchItem, grip: ItemGrip, to: P2) -> SketchItem {
    let mut out = item.clone();
    match (&mut out.kind, grip) {
        (ItemKind::Line { a, .. }, ItemGrip::Point(0)) => *a = to,
        (ItemKind::Line { b, .. }, ItemGrip::Point(_)) => *b = to,
        (ItemKind::Spline { points }, ItemGrip::Point(i)) => {
            if let Some(p) = points.get_mut(i) {
                *p = to;
            }
        }
        (ItemKind::Image(img), ItemGrip::Corner(c)) => *img = resize_image(img, c, to),
        _ => {}
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Base64 (images are kept in the document as text)

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for ch in s.bytes() {
        let v = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return Err(format!("invalid base64 character {:?}", ch as char)),
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

mod base64_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::base64_encode(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        super::base64_decode(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> SheetImage {
        SheetImage { name: "logo.png".into(), at: [10.0, 20.0], width: 40.0, height: 20.0, pixels: (200, 100), data: vec![1, 2, 3] }
    }

    #[test]
    fn corner_resize_keeps_the_aspect_ratio() {
        let i = img();
        // Top-right corner dragged up and right: the larger drag wins, the bottom-left stays.
        let r = resize_image(&i, 2, [80.0, 30.0]);
        assert_eq!(r.at, [10.0, 20.0]);
        assert!((r.width / r.height - 2.0).abs() < 1e-12, "{} x {}", r.width, r.height);
        assert!((r.width - 70.0).abs() < 1e-12);
        let r = resize_image(&i, 2, [20.0, 80.0]);
        assert!((r.width / r.height - 2.0).abs() < 1e-12);
        assert!((r.height - 60.0).abs() < 1e-12);
        // Bottom-left dragged: the top-right corner stays.
        let r = resize_image(&i, 0, [0.0, 0.0]);
        let tr = r.corners()[2];
        assert!((tr[0] - 50.0).abs() < 1e-12 && (tr[1] - 40.0).abs() < 1e-12, "{tr:?}");
        assert!((r.width / r.height - 2.0).abs() < 1e-12);
        // Dragged past the fixed corner: never smaller than the minimum.
        let r = resize_image(&i, 2, [10.0, 20.0]);
        assert!(r.height >= MIN_IMAGE - 1e-12 && (r.width / r.height - 2.0).abs() < 1e-12);
        // Through the grip API.
        let item = SketchItem::new(ItemKind::Image(i));
        let d = drag_grip(&item, ItemGrip::Corner(1), [90.0, 5.0]);
        let ItemKind::Image(r) = d.kind else { panic!() };
        assert!((r.width / r.height - 2.0).abs() < 1e-12);
        assert_eq!(r.corners()[3], [10.0, 40.0], "the top-left corner stays");
    }

    #[test]
    fn splines_pass_through_their_points_and_match_their_nurbs() {
        let pts = [[0.0, 0.0], [10.0, 5.0], [20.0, -3.0], [35.0, 8.0]];
        let segs = spline_beziers(&pts);
        assert_eq!(segs.len(), 3);
        for (i, s) in segs.iter().enumerate() {
            assert_eq!(s[0], pts[i]);
            assert_eq!(s[3], pts[i + 1]);
        }
        // C1 (and C2) at the joints.
        for w in segs.windows(2) {
            let d0 = [w[0][3][0] - w[0][2][0], w[0][3][1] - w[0][2][1]];
            let d1 = [w[1][1][0] - w[1][0][0], w[1][1][1] - w[1][0][1]];
            assert!((d0[0] - d1[0]).abs() < 1e-9 && (d0[1] - d1[1]).abs() < 1e-9);
        }
        let (knots, ctrl) = spline_nurbs(&pts);
        assert_eq!(ctrl.len(), 10);
        assert_eq!(knots.len(), ctrl.len() + 4);
        for (i, s) in segs.iter().enumerate() {
            for t in [0.0, 0.3, 0.7] {
                let a = bezier_at(s, t);
                let b = bspline_at(3, &knots, &ctrl, &[], i as f64 + t).unwrap();
                assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9, "{a:?} {b:?}");
            }
        }
        let poly = spline_polyline(&pts, 8);
        assert_eq!(poly.first(), Some(&pts[0]));
        assert_eq!(poly.last(), Some(&pts[3]));
    }

    #[test]
    fn base64_round_trips() {
        for n in 0..10 {
            let data: Vec<u8> = (0..n * 7 + 1).map(|i| (i * 37 % 256) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&data)).unwrap(), data);
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn bulges_make_arcs() {
        // A half circle of radius 5 from (0,0) to (10,0), bulge 1 (counter-clockwise: below).
        let p = bulge_polyline(&[[0.0, 0.0], [10.0, 0.0]], &[1.0], false);
        for q in &p {
            assert!(((q[0] - 5.0).hypot(q[1]) - 5.0).abs() < 1e-9);
        }
        assert!(p.iter().any(|q| q[1] < -4.9));
    }
}
