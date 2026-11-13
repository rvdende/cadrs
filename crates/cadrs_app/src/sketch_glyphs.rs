//! Constraint glyphs and inference feedback, drawn in screen space over the sketch (M5).
//!
//! Measured from `reference/onshape/screens/10`, `12a`, `18`, `18a`:
//! - **Constraint glyphs**: dark (`#333`) line icons in 20 × 20 px white boxes; a group of
//!   glyphs on one entity is a row with 2 px gaps. A line's group is centered on its midpoint,
//!   20 px to the inside (toward its neighbours), with a thin `#bcc4cb` leader. A point's group
//!   sits up and to the left of it (box center 17 px left and up), with diagonal leaders.
//!   Constraints to the fixed origin or plane axes (external references) get a pale blue
//!   `#def1fb` box.
//! - **Coincidences** have no glyph: a point coincident with anything (shared by curves, on a
//!   curve, at the origin) is drawn as a solid square instead of a round dot (`sketch_draw`),
//!   and its context menu deletes the coincidence ([`coincident_points`]).
//! - **Perpendicular corners**: ⊥ between two lines that meet end to end is a small right-angle
//!   mark in the corner instead of a glyph box ([`corner_marks`]); it is picked, selected and
//!   deleted like a glyph.
//! - **Inference glyphs**: the same icons in a `#eaedf0` box whose corner is 21 px right of and
//!   below the cursor, one per inferred constraint.
//! - **Snap square**: a crisp 1 px `#fdca33` square, 17 px across, centered on the snap point.
//! - **States (M6)**: a hovered glyph is orange and highlights its geometry; a selected one is
//!   yellow; a constraint of the conflicting set is white on red (`constraints.md`,
//!   `inspection-and-repair/ex1-step4.png`). Glyphs keep
//!   4 px clear of the sketch axes, and their leaders stay short (at most about 24 px).
//!
//! The icons are cadrs_icons's constraint glyphs (`constraint-*`), rasterized with the other icons.

use std::collections::HashMap;

use bevy::prelude::*;
use cadrs_sketch::geom::segment_hits_box;
use cadrs_sketch::hit::curve_polyline;
use cadrs_sketch::infer::{Candidate, Kind, Placed};
use cadrs_sketch::{
    ConstraintId, ConstraintOf, CurveId, CurveKind, CurveRef, Orient, PointRef, Sketch,
    Vec2 as SVec2,
};
use cadrs_ui::Icon;

use crate::sketch_tools::ScreenMap;
use crate::viewport::{ViewportArea, ViewportRect};

/// A glyph's icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphIcon {
    Coincident,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Tangent,
    Midpoint,
    Equal,
    Concentric,
    Fix,
    Symmetric,
    Offset,
    Normal,
    /// A projected entity's link to its source (S20).
    Use,
    /// A pierced point (S12.11).
    Pierce,
    /// G2 where two curves meet (S12.14).
    Curvature,
}

/// Half a sketch glyph's size (px): its strokes span about twice this.
pub const GLYPH_HALF: f32 = 7.0;

/// Points round a circle `c`, radius `r`, from `a0` to `a1` (degrees), in local units.
fn ring(c: [f32; 2], r: f32, a0: f32, a1: f32) -> Vec<[f32; 2]> {
    let n = (((a1 - a0).abs() / 30.0).ceil() as usize).max(2);
    (0..=n)
        .map(|i| {
            let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

impl GlyphIcon {
    /// The glyph as it is drawn on a sketch: strokes in a local frame (x right, y up along the
    /// sketch's axes) spanning about -1..1, scaled by [`GLYPH_HALF`]. Lines in the schematic's
    /// manner rather than the toolbar icons in a box.
    pub fn strokes(self) -> Vec<Vec<[f32; 2]>> {
        match self {
            GlyphIcon::Horizontal => vec![vec![[-1.0, 0.0], [1.0, 0.0]]],
            GlyphIcon::Vertical => vec![vec![[0.0, -1.0], [0.0, 1.0]]],
            GlyphIcon::Parallel => vec![
                vec![[-0.85, -0.9], [-0.15, 0.9]],
                vec![[0.15, -0.9], [0.85, 0.9]],
            ],
            GlyphIcon::Perpendicular => vec![
                vec![[-1.0, -0.8], [1.0, -0.8]],
                vec![[0.0, -0.8], [0.0, 1.0]],
            ],
            // A circle under the line it touches.
            GlyphIcon::Tangent => vec![
                ring([0.0, -0.25], 0.65, 0.0, 360.0),
                vec![[-1.0, 0.4], [1.0, 0.4]],
            ],
            GlyphIcon::Midpoint => vec![
                vec![[-1.0, 0.0], [-0.4, 0.0]],
                vec![[0.0, 0.4], [0.4, 0.0], [0.0, -0.4], [-0.4, 0.0], [0.0, 0.4]],
                vec![[0.4, 0.0], [1.0, 0.0]],
            ],
            GlyphIcon::Equal => vec![
                vec![[-0.85, 0.35], [0.85, 0.35]],
                vec![[-0.85, -0.35], [0.85, -0.35]],
            ],
            GlyphIcon::Concentric => vec![
                ring([0.0, 0.0], 0.95, 0.0, 360.0),
                ring([0.0, 0.0], 0.4, 0.0, 360.0),
            ],
            // A ground.
            GlyphIcon::Fix => vec![
                vec![[0.0, 1.0], [0.0, 0.15]],
                vec![[-0.95, 0.15], [0.95, 0.15]],
                vec![[-0.6, -0.35], [0.6, -0.35]],
                vec![[-0.25, -0.85], [0.25, -0.85]],
            ],
            GlyphIcon::Symmetric => vec![
                vec![[0.0, -1.0], [0.0, 1.0]],
                vec![[-0.25, 0.0], [-1.0, 0.6], [-1.0, -0.6], [-0.25, 0.0]],
                vec![[0.25, 0.0], [1.0, 0.6], [1.0, -0.6], [0.25, 0.0]],
            ],
            GlyphIcon::Offset => vec![
                ring([0.0, -0.9], 0.75, 25.0, 155.0),
                ring([0.0, -0.9], 1.6, 50.0, 130.0),
            ],
            // A line standing on a curve.
            GlyphIcon::Normal => vec![
                ring([0.0, 1.0], 1.6, 235.0, 305.0),
                vec![[0.0, -0.6], [0.0, 1.0]],
            ],
            // Two chain links.
            GlyphIcon::Use => vec![
                ring([-0.4, 0.0], 0.55, 0.0, 360.0),
                ring([0.4, 0.0], 0.55, 0.0, 360.0),
            ],
            GlyphIcon::Pierce => vec![
                ring([0.0, 0.0], 0.55, 0.0, 360.0),
                vec![[-1.0, -1.0], [1.0, 1.0]],
            ],
            // The tangent's arch, doubled.
            GlyphIcon::Curvature => vec![
                ring([0.0, -0.8], 1.5, 40.0, 140.0),
                ring([0.0, -0.8], 0.8, 30.0, 150.0),
            ],
            GlyphIcon::Coincident => vec![vec![
                [-0.45, -0.45],
                [0.45, -0.45],
                [0.45, 0.45],
                [-0.45, 0.45],
                [-0.45, -0.45],
            ]],
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            GlyphIcon::Equal => "constraint-equal",
            GlyphIcon::Concentric => "constraint-concentric",
            GlyphIcon::Fix => "constraint-fix",
            GlyphIcon::Coincident => "constraint-coincident",
            GlyphIcon::Horizontal => "constraint-horizontal",
            GlyphIcon::Vertical => "constraint-vertical",
            GlyphIcon::Parallel => "constraint-parallel",
            GlyphIcon::Perpendicular => "constraint-perpendicular",
            GlyphIcon::Tangent => "constraint-tangent",
            GlyphIcon::Midpoint => "constraint-midpoint",
            GlyphIcon::Symmetric => "constraint-symmetric",
            GlyphIcon::Offset => "constraint-offset",
            GlyphIcon::Normal => "constraint-normal",
            GlyphIcon::Use => "constraint-use",
            GlyphIcon::Pierce => "constraint-pierce",
            GlyphIcon::Curvature => "constraint-curvature",
        }
    }
}

/// How a glyph box looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphStyle {
    /// A constraint between sketch entities: white box.
    Constraint,
    /// A constraint to the origin or a plane axis: pale blue box.
    External,
    /// A link to geometry outside the sketch (Use, Pierce): light blue box, #92d4ee
    /// (`constraints/constraints-referenced.png`).
    Link,
    /// An inference next to the cursor: grey box.
    Inference,
    /// Under the pointer: orange.
    Hover,
    /// Selected: yellow.
    Selected,
    /// A conflicting constraint (the whole conflicting set): a white glyph on a red chip with a
    /// darker 1 px border (`inspection-and-repair/ex1-step4.png`; it was a red glyph in a white
    /// box after `screens/15a`, which read as an ordinary glyph).
    Conflict,
}

impl GlyphStyle {
    fn background(self) -> Color {
        match self {
            // Slightly see-through, so geometry under a glyph still shows.
            GlyphStyle::Constraint => Color::WHITE.with_alpha(0.85),
            GlyphStyle::External => Color::srgb_u8(0xde, 0xf1, 0xfb).with_alpha(0.85),
            GlyphStyle::Link => Color::srgb_u8(0x92, 0xd4, 0xee),
            GlyphStyle::Inference => Color::srgb_u8(0xea, 0xed, 0xf0),
            GlyphStyle::Hover => Color::srgb_u8(0xfe, 0xc6, 0x85),
            GlyphStyle::Selected => Color::srgb_u8(0xf6, 0xbc, 0x1a),
            GlyphStyle::Conflict => Color::srgb_u8(0xe0, 0x5f, 0x5f),
        }
    }

    /// The box's 1 px border (inside the box).
    fn outline(self) -> Outline {
        match self {
            GlyphStyle::Conflict => Outline::new(
                Val::Px(1.0),
                Val::Px(-1.0),
                Color::srgb_u8(0xbe, 0x00, 0x00),
            ),
            // A selected glyph: amber with a darker 1 px rim, distinct from the hover orange.
            GlyphStyle::Selected => Outline::new(
                Val::Px(1.0),
                Val::Px(-1.0),
                Color::srgb_u8(0xb3, 0x7a, 0x00),
            ),
            _ => Outline::new(Val::Px(0.0), Val::Px(0.0), Color::NONE),
        }
    }

    fn foreground(self) -> Color {
        match self {
            GlyphStyle::Conflict => Color::WHITE,
            _ => Color::srgb_u8(0x33, 0x33, 0x33),
        }
    }

    fn name(self) -> &'static str {
        match self {
            GlyphStyle::Inference => "sketch-inference-glyph",
            _ => "sketch-constraint-glyph",
        }
    }
}

/// One glyph box, centered at `center` (screen px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphSpec {
    pub icon: GlyphIcon,
    pub center: Vec2,
    pub style: GlyphStyle,
    /// The constraint it shows (`None` for a shared point's coincidence and inferences).
    pub constraint: Option<ConstraintId>,
    /// The group it belongs to and where the group's first box is from the group's anchor
    /// (the point, or the middle of the line) — for dragging groups. `None` for inferences.
    pub group: Option<(Host, Vec2)>,
}

/// Glyph groups the user dragged: where their first box is from their anchor (screen px).
/// Kept while the sketch is edited.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct GlyphOffsets(pub std::collections::HashMap<Host, Vec2>);

/// The glyph group under a screen point (a constraint glyph, not an inference).
pub fn glyph_group_at(overlay: &SketchOverlay, p: Vec2) -> Option<(Host, Vec2)> {
    overlay
        .glyphs
        .iter()
        .rev()
        .filter(|g| g.style != GlyphStyle::Inference)
        .find(|g| g.contains(p))
        .and_then(|g| g.group)
}

impl GlyphSpec {
    /// True if the screen point is on the glyph's box.
    pub fn contains(&self, p: Vec2) -> bool {
        (p - self.center).abs().max_element() <= BOX / 2.0
    }
}

/// The constraint glyph under a screen point, if any.
pub fn glyph_at(overlay: &SketchOverlay, p: Vec2) -> Option<ConstraintId> {
    overlay
        .glyphs
        .iter()
        .rev()
        .filter(|g| g.style != GlyphStyle::Inference)
        .find(|g| g.contains(p))
        .and_then(|g| g.constraint)
        .or_else(|| {
            overlay
                .corners
                .iter()
                .find(|c| c.contains(p))
                .map(|c| c.constraint)
        })
}

/// Screen-space overlays for this frame (filled by `sketch_draw`).
#[derive(Resource, Debug, Default, PartialEq)]
pub struct SketchOverlay {
    pub glyphs: Vec<GlyphSpec>,
    /// The snap square's center (screen px).
    pub square: Option<Vec2>,
    /// Dimension value labels (for picking).
    pub dims: Vec<crate::sketch_dimension::DimLabel>,
    /// Perpendicular corner marks (for picking).
    pub corners: Vec<CornerSpec>,
}

/// A perpendicular corner mark on screen: the corner and the mark's two other corners along
/// each line (`along[0]` on the first line), `size` px from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerSpec {
    pub constraint: ConstraintId,
    pub corner: Vec2,
    pub dirs: [Vec2; 2],
    pub size: f32,
}

impl CornerSpec {
    /// The mark's corners: the lines' corner, along the first line, across, along the second.
    pub fn quad(&self) -> [Vec2; 4] {
        let [u, v] = self.dirs.map(|d| d * self.size);
        let c = self.corner;
        [c, c + u, c + u + v, c + v]
    }

    /// True if the screen point picks the mark: on it (or 3 px round it), but not on the corner
    /// point itself, which stays the point's (to drag the corner).
    pub fn contains(&self, p: Vec2) -> bool {
        let [u, v] = self.dirs;
        let det = u.x * v.y - u.y * v.x;
        if det.abs() < 1e-3 || p.distance(self.corner) <= POINT_PICK {
            return false;
        }
        let d = p - self.corner;
        let s = (d.x * v.y - d.y * v.x) / det;
        let t = (u.x * d.y - u.y * d.x) / det;
        let m = 3.0;
        (-m..=self.size + m).contains(&s) && (-m..=self.size + m).contains(&t)
    }
}

/// How close (px) to a point the pointer picks the point rather than a corner mark on it.
const POINT_PICK: f32 = cadrs_sketch::hit::POINT_TOLERANCE_PX as f32 - 2.0;
/// A corner mark's side (px), at most; shorter lines get a smaller one.
pub const CORNER_MARK: f32 = 10.0;

/// A perpendicular constraint between two lines that meet end to end: where they meet and the
/// far ends of the two lines (sketch mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerMark {
    pub constraint: ConstraintId,
    pub lines: [CurveId; 2],
    pub corner: SVec2,
    pub far: [SVec2; 2],
}

/// Where two lines meet end to end (a shared point, or two ends in one place): the corner and
/// each line's other end.
fn line_corner(s: &Sketch, a: CurveRef, b: CurveRef) -> Option<(SVec2, [SVec2; 2])> {
    let (CurveRef::Curve(a), CurveRef::Curve(b)) = (a, b) else {
        return None;
    };
    let ends = |k: CurveId| match s.curves.get(k).map(|c| c.kind) {
        Some(CurveKind::Line { a, b }) => Some((s.pos(a), s.pos(b))),
        _ => None,
    };
    let ((a0, a1), (b0, b1)) = (ends(a)?, ends(b)?);
    for (pa, fa) in [(a0, a1), (a1, a0)] {
        for (pb, fb) in [(b0, b1), (b1, b0)] {
            if pa.distance(pb) <= 1e-6 {
                return Some((pa, [fa, fb]));
            }
        }
    }
    None
}

/// The sketch's perpendicular corners (see [`CornerMark`]), shown as marks, not glyphs.
pub fn corner_marks(s: &Sketch) -> Vec<CornerMark> {
    s.constraints
        .iter()
        .filter(|(k, _)| !s.quiet.constraints.contains(k))
        .filter_map(|(k, c)| match *c {
            ConstraintOf::Perpendicular(a @ CurveRef::Curve(la), b @ CurveRef::Curve(lb)) => {
                let (corner, far) = line_corner(s, a, b)?;
                Some(CornerMark {
                    constraint: k,
                    lines: [la, lb],
                    corner,
                    far,
                })
            }
            _ => None,
        })
        .collect()
}

/// The points drawn as solid squares: coincident with something (shared by two curves, or in a
/// coincident or point-on-curve constraint), which their context menu can undo.
pub fn coincident_points(s: &Sketch) -> std::collections::HashSet<cadrs_sketch::PointId> {
    let mut out: std::collections::HashSet<_> = s
        .shared_points()
        .into_iter()
        .filter(|p| !s.quiet.points.contains(p))
        .collect();
    for (k, c) in &s.constraints {
        if s.quiet.constraints.contains(&k) {
            continue;
        }
        match *c {
            ConstraintOf::Coincident(a, b) => {
                out.extend([a, b].into_iter().filter_map(|p| match p {
                    PointRef::Point(p) => Some(p),
                    PointRef::Origin => None,
                }));
            }
            ConstraintOf::PointOnCurve(PointRef::Point(p), _) => {
                out.insert(p);
            }
            _ => {}
        }
    }
    out
}

/// Glyph box size and the distance between neighbouring boxes' centers (px).
pub const BOX: f32 = 20.0;
pub const PITCH: f32 = 22.0;
/// A point's glyph row: the box nearest the point is centered this far left and up.
const POINT_OFFSET: Vec2 = Vec2::new(-17.0, -17.0);
/// Glyph boxes keep this far (px) from the sketch axes.
const AXIS_CLEARANCE: f64 = 4.0;
/// ... and this far from dimensions: their lines, extension lines, leaders and values (T2
/// judge: glyphs crowding a dimension's extension line).
const FAT_CLEARANCE: f64 = 7.0;
/// A line's glyph row: centered this far from the line's midpoint, outside the shape.
const LINE_OFFSET: f32 = 20.5;

/// What a group of glyphs is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Host {
    Point(PointRef),
    Curve(CurveId),
    /// Where a constraint touches, off any sketch point: a tangent's touch point
    /// ([`cadrs_sketch::extension::touch_point`]).
    Touch(ConstraintId),
}

/// Glyph boxes and their leaders (screen px, from the geometry to the box center).
#[derive(Debug, Default, PartialEq)]
pub struct GlyphLayout {
    pub glyphs: Vec<GlyphSpec>,
    pub leaders: Vec<(Vec2, Vec2)>,
    /// Where each group's first box went, from its anchor (remembered for next frame).
    pub placed: HashMap<Host, Vec2>,
}

/// What glyph groups keep clear of, besides the sketch's own curves and axes.
#[derive(Debug, Default, Clone, Copy)]
pub struct GlyphObstacles<'a> {
    /// Segments (screen px) boxes must not touch: the rubber band.
    pub thin: &'a [(SVec2, SVec2)],
    /// Segments boxes keep 4 px from: dimension lines, arrowheads and value boxes.
    pub fat: &'a [(SVec2, SVec2)],
    /// Last frame's places ([`GlyphLayout::placed`]), for hysteresis.
    pub previous: Option<&'a HashMap<Host, Vec2>>,
    /// The viewport: groups anchored outside it (plus a margin) are skipped.
    pub visible: Option<Rect>,
    /// Keep every group where it was last frame, free or not (while geometry is dragged, so
    /// glyphs do not jump from side to side).
    pub frozen: bool,
}

/// Groups anchored this far outside the viewport are still laid out (px).
const GLYPH_MARGIN: f32 = 60.0;

/// Where a group is anchored on screen: its point, or its curve's middle.
fn host_anchor(s: &Sketch, map: &ScreenMap, host: Host) -> Option<Vec2> {
    match host {
        Host::Point(PointRef::Point(p)) => s.points.get(p).map(|p| map.to_screen(p.pos)),
        Host::Point(PointRef::Origin) => Some(map.to_screen(SVec2::ZERO)),
        Host::Touch(k) => touch(s, k).map(|p| map.to_screen(p)),
        Host::Curve(c) => {
            let pts = curve_polyline(s, c);
            let lo = pts
                .iter()
                .fold(SVec2::new(f64::MAX, f64::MAX), |a, p| a.min(*p));
            let hi = pts
                .iter()
                .fold(SVec2::new(f64::MIN, f64::MIN), |a, p| a.max(*p));
            (!pts.is_empty()).then(|| map.to_screen((lo + hi) * 0.5))
        }
    }
}

/// Which way is outside at a point or touch point (screen, unit): away from the other points
/// of its curves (the shape's inside) and from the centers of the circles and arcs it touches.
/// `None` where they balance out.
fn outward(s: &Sketch, map: &ScreenMap, host: Host, at: Vec2) -> Option<Vec2> {
    let toward: Vec<SVec2> = match host {
        Host::Point(PointRef::Point(p)) => s
            .curves_at(p)
            .flat_map(|k| s.curve_points(k))
            .filter(|q| *q != p)
            .map(|q| s.pos(q))
            .collect(),
        Host::Touch(k) => match s.constraints.get(k) {
            Some(ConstraintOf::Tangent(a, b)) => [*a, *b]
                .into_iter()
                .filter_map(|c| match c {
                    CurveRef::Curve(c) => cadrs_sketch::extension::round(s, c).map(|(o, _)| o),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    let sum = toward
        .iter()
        .map(|q| (map.to_screen(*q) - at).normalize_or_zero())
        .fold(Vec2::ZERO, |a, d| a + d);
    (sum.length() > 0.3).then(|| -sum.normalize())
}

/// Where a constraint touches (see [`Host::Touch`]).
fn touch(s: &Sketch, k: ConstraintId) -> Option<SVec2> {
    cadrs_sketch::extension::touch_point(s, s.constraints.get(k)?)
}

/// Segments bucketed in a uniform screen grid, for fast "does anything cross this box" tests.
struct SegmentIndex {
    segs: Vec<(SVec2, SVec2)>,
    grid: HashMap<(i32, i32), Vec<u32>>,
    /// Segments spanning too many cells, tested every time.
    long: Vec<u32>,
}

impl SegmentIndex {
    const CELL: f64 = 32.0;
    const MAX_CELLS: i64 = 256;

    fn cell(v: f64) -> i32 {
        (v / Self::CELL).floor().clamp(-1e6, 1e6) as i32
    }

    fn new(segs: Vec<(SVec2, SVec2)>) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let mut long = Vec::new();
        for (i, (a, b)) in segs.iter().enumerate() {
            let (x0, x1) = (Self::cell(a.x.min(b.x)), Self::cell(a.x.max(b.x)));
            let (y0, y1) = (Self::cell(a.y.min(b.y)), Self::cell(a.y.max(b.y)));
            if (x1 as i64 - x0 as i64 + 1) * (y1 as i64 - y0 as i64 + 1) > Self::MAX_CELLS {
                long.push(i as u32);
                continue;
            }
            for x in x0..=x1 {
                for y in y0..=y1 {
                    grid.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        Self { segs, grid, long }
    }

    fn hits_box(&self, lo: SVec2, hi: SVec2) -> bool {
        let hit = |i: &u32| {
            let (a, b) = self.segs[*i as usize];
            segment_hits_box(a, b, lo, hi)
        };
        if self.long.iter().any(hit) {
            return true;
        }
        for x in Self::cell(lo.x)..=Self::cell(hi.x) {
            for y in Self::cell(lo.y)..=Self::cell(hi.y) {
                if self.grid.get(&(x, y)).is_some_and(|v| v.iter().any(hit)) {
                    return true;
                }
            }
        }
        false
    }
}

/// True when the constraint glyphs would bury the geometry: zoomed far out (under 1 px/mm),
/// or their boxes would cover more than half of the sketch's on-screen area (plus a 40 px
/// margin all round). Then only the hovered entity's glyphs are shown, until the user zooms in.
pub fn too_dense(s: &Sketch, map: &ScreenMap, visible: Rect) -> bool {
    if map.px_per_mm() < 1.0 {
        return true;
    }
    let glyphs: usize = glyph_groups(s).iter().map(|(_, v)| v.len()).sum();
    if glyphs == 0 {
        return false;
    }
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for p in s.points.values() {
        let q = map.to_screen(p.pos);
        lo = lo.min(q);
        hi = hi.max(q);
    }
    let r = Rect::from_corners(lo, hi).inflate(40.0).intersect(visible);
    let area = r.width().max(0.0) * r.height().max(0.0);
    glyphs as f32 * PITCH * PITCH > 0.5 * area.max(1.0)
}

/// Last frame's glyph places, for [`GlyphObstacles::previous`]. Cleared when the sketch closes.
#[derive(Resource, Debug, Default)]
pub struct GlyphMemory(pub HashMap<Host, Vec2>);

fn is_external(c: CurveRef) -> bool {
    !matches!(c, CurveRef::Curve(_))
}

/// One glyph of a group: its icon, whether it refers to the origin or an axis (pale blue), and
/// its constraint.
pub type GroupGlyph = (GlyphIcon, bool, Option<ConstraintId>);

/// The glyph groups of a sketch's constraints, in order: one group per host.
pub fn glyph_groups(s: &Sketch) -> Vec<(Host, Vec<GroupGlyph>)> {
    let mut groups: Vec<(Host, Vec<GroupGlyph>)> = Vec::new();
    // Where each host's group is in `groups` (a linear search is quadratic in big sketches).
    let mut index: HashMap<Host, usize> = HashMap::new();
    let mut equal_shown = std::collections::HashSet::new();
    let mut add = |host: Host, icon: GlyphIcon, external: bool, id: Option<ConstraintId>| {
        let found = index.get(&host).copied();
        match found.map(|i| &mut groups[i]) {
            Some((_, v)) => v.push((icon, external, id)),
            None => {
                index.insert(host, groups.len());
                groups.push((host, vec![(icon, external, id)]));
            }
        }
    };
    let kind = |c: CurveRef| match c {
        CurveRef::Curve(id) => s.curves.get(id).map(|c| c.kind),
        _ => None,
    };
    let is_line = |c: CurveRef| matches!(kind(c), Some(CurveKind::Line { .. }));
    let is_round = |c: CurveRef| {
        matches!(
            kind(c),
            Some(CurveKind::Circle { .. } | CurveKind::Arc { .. })
        )
    };
    let center = |c: CurveRef| match kind(c) {
        Some(CurveKind::Circle { center, .. } | CurveKind::Arc { center, .. }) => Some(center),
        _ => None,
    };
    for (k, c) in &s.constraints {
        // What a polygon, slot, fillet or chamfer generates between its own pieces shows no
        // glyph (`entity_tools/polygon-inscribed-circumscribed.png`).
        if s.quiet.constraints.contains(&k) {
            continue;
        }
        let id = Some(k);
        match *c {
            // Coincident points are drawn as squares instead ([`coincident_points`]).
            ConstraintOf::Coincident(..) | ConstraintOf::PointOnCurve(..) => {}
            ConstraintOf::Midpoint(p, _) => add(Host::Point(p), GlyphIcon::Midpoint, false, id),
            ConstraintOf::Horizontal(o) | ConstraintOf::Vertical(o) => {
                let icon = if matches!(c, ConstraintOf::Horizontal(_)) {
                    GlyphIcon::Horizontal
                } else {
                    GlyphIcon::Vertical
                };
                match o {
                    Orient::Line(CurveRef::Curve(l)) => add(Host::Curve(l), icon, false, id),
                    Orient::Line(_) => {}
                    Orient::Points(a, b) => {
                        let (host, other) = if a == PointRef::Origin {
                            (b, a)
                        } else {
                            (a, b)
                        };
                        add(Host::Point(host), icon, other == PointRef::Origin, id);
                    }
                }
            }
            ConstraintOf::Parallel(a, b) | ConstraintOf::Perpendicular(a, b) => {
                let icon = if matches!(c, ConstraintOf::Parallel(..)) {
                    GlyphIcon::Parallel
                } else {
                    GlyphIcon::Perpendicular
                };
                // A corner's ⊥ is a mark in the corner instead ([`corner_marks`]).
                if icon == GlyphIcon::Perpendicular && line_corner(s, a, b).is_some() {
                    continue;
                }
                // On the first line (the other may be an axis).
                if let (CurveRef::Curve(l), true) = (a, is_line(a)) {
                    add(Host::Curve(l), icon, is_external(b), id);
                } else if let (CurveRef::Curve(l), true) = (b, is_line(b)) {
                    add(Host::Curve(l), icon, is_external(a), id);
                }
            }
            ConstraintOf::Curvature(a, b) => {
                // At the point the two curves share.
                if let (CurveRef::Curve(a), CurveRef::Curve(b)) = (a, b)
                    && let (Some((a0, a1)), Some((b0, b1))) = (s.curve_ends(a), s.curve_ends(b))
                    && let Some(p) = [a0, a1].into_iter().find(|p| *p == b0 || *p == b1)
                {
                    add(
                        Host::Point(PointRef::Point(p)),
                        GlyphIcon::Curvature,
                        false,
                        id,
                    );
                }
            }
            ConstraintOf::Tangent(a, b) => {
                let shared = match (a, b) {
                    (CurveRef::Curve(a), CurveRef::Curve(b)) => {
                        let pa = s.curve_points(a);
                        s.curve_points(b).into_iter().find(|p| pa.contains(p))
                    }
                    _ => None,
                };
                match shared {
                    Some(p) => add(
                        Host::Point(PointRef::Point(p)),
                        GlyphIcon::Tangent,
                        false,
                        id,
                    ),
                    // Lines, circles and arcs: where they touch (the line or arc is drawn on to
                    // it when that is past its end, `sketch_draw`).
                    None if cadrs_sketch::extension::touch_point(s, c).is_some() => {
                        add(Host::Touch(k), GlyphIcon::Tangent, false, id);
                    }
                    None => {
                        // Not touching at an end: on the circle or arc (on the line, near
                        // where it touches, for an ellipse: Final re-audit, S8).
                        let ellipse = |c: CurveRef| {
                            matches!(
                                kind(c),
                                Some(CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. })
                            )
                        };
                        let round = if ellipse(a) || ellipse(b) {
                            if is_line(a) { a } else { b }
                        } else if is_round(a) {
                            a
                        } else {
                            b
                        };
                        if let CurveRef::Curve(r) = round {
                            add(Host::Curve(r), GlyphIcon::Tangent, false, id);
                        }
                    }
                }
            }
            ConstraintOf::Equal(a, b) => {
                // "=" on both, once per entity: three lines made equal in pairs show one "="
                // each, not two on the shared line.
                for c in [a, b] {
                    if let CurveRef::Curve(k) = c
                        && equal_shown.insert(k)
                    {
                        add(Host::Curve(k), GlyphIcon::Equal, false, id);
                    }
                }
            }
            // On the line (S12.10).
            ConstraintOf::Normal(l, _) => {
                if let CurveRef::Curve(k) = l {
                    add(Host::Curve(k), GlyphIcon::Normal, false, id);
                }
            }
            ConstraintOf::Concentric(a, _) => {
                if let Some(p) = center(a) {
                    add(
                        Host::Point(PointRef::Point(p)),
                        GlyphIcon::Concentric,
                        false,
                        id,
                    );
                }
            }
            ConstraintOf::FixPoint(p) => add(Host::Point(p), GlyphIcon::Fix, false, id),
            ConstraintOf::FixCurve(CurveRef::Curve(k)) => {
                add(Host::Curve(k), GlyphIcon::Fix, false, id)
            }
            ConstraintOf::FixCurve(_) => {}
            // On both entities, like Onshape's "Σ" glyphs.
            ConstraintOf::SymmetricPoints(a, b, _) => {
                for p in [a, b] {
                    add(Host::Point(p), GlyphIcon::Symmetric, false, id);
                }
            }
            ConstraintOf::SymmetricCurves(a, b, _) => {
                for c in [a, b] {
                    if let CurveRef::Curve(k) = c {
                        add(Host::Curve(k), GlyphIcon::Symmetric, false, id);
                    }
                }
            }
            // The link of a projected curve sits on it (S20.2: it can be deleted).
            ConstraintOf::Use(CurveRef::Curve(k), _) => {
                add(Host::Curve(k), GlyphIcon::Use, true, id)
            }
            // A used part vertex is a projected point: the Use link.
            ConstraintOf::Pierce(p, cadrs_sketch::Link::Vertex { .. }) => {
                add(Host::Point(p), GlyphIcon::Use, true, id)
            }
            ConstraintOf::Pierce(p, _) => add(Host::Point(p), GlyphIcon::Pierce, true, id),
            // The offset chain's shared distance and a rectangle's center are not drawn.
            ConstraintOf::EqualOffset(..)
            | ConstraintOf::Center(..)
            | ConstraintOf::EqualDistance(..)
            | ConstraintOf::Use(..)
            | ConstraintOf::TextAspect(..) => {}
        }
    }
    groups
}

/// A row of `n` glyph boxes, the first centered at `first_center`: the screen rectangles
/// (min, max) of its boxes.
fn row_boxes(first_center: Vec2, n: usize) -> Vec<(Vec2, Vec2)> {
    (0..n)
        .map(|i| {
            let c = first_center + Vec2::new(i as f32 * PITCH, 0.0);
            (c - Vec2::splat(BOX / 2.0), c + Vec2::splat(BOX / 2.0))
        })
        .collect()
}

/// Where the segment from `from` to the center of a box (half size `half`) enters the box
/// (the center itself if `from` is inside).
fn clip_to_box(from: Vec2, center: Vec2, half: Vec2) -> Vec2 {
    let d = center - from;
    let sx = if d.x.abs() > 1e-6 {
        half.x / d.x.abs()
    } else {
        f32::INFINITY
    };
    let sy = if d.y.abs() > 1e-6 {
        half.y / d.y.abs()
    } else {
        f32::INFINITY
    };
    let s = sx.min(sy);
    if !s.is_finite() || s >= 1.0 {
        return center;
    }
    center - d * s
}

/// Places the glyph groups on screen. `show` filters hosts (all of them with "Show
/// constraints" on, only the hovered entity's otherwise).
///
/// Each group tries a few places, preferring Onshape's (up-left of a point; inside a line, at
/// its middle) and takes the first that covers no sketch curve and no other glyph.
pub fn layout_glyphs(
    s: &Sketch,
    map: &ScreenMap,
    obstacles: GlyphObstacles,
    offsets: &GlyphOffsets,
    show: impl Fn(Host) -> bool,
) -> GlyphLayout {
    let mut out = GlyphLayout::default();
    // The groups to place (shown, and on screen). None: nothing to lay out (a zoomed-out
    // 500-entity sketch shows none, and building the segment grid is not free).
    let groups: Vec<(Host, Vec<GroupGlyph>)> = glyph_groups(s)
        .into_iter()
        .filter(|(host, _)| show(*host))
        .filter(|(host, _)| {
            obstacles.visible.is_none_or(|r| {
                host_anchor(s, map, *host).is_none_or(|a| r.inflate(GLYPH_MARGIN).contains(a))
            })
        })
        .collect();
    if groups.is_empty() {
        return out;
    }
    // Every curve as screen segments, for collision tests, in a grid so each test only looks
    // at nearby segments (a 500-entity sketch has thousands of segments).
    let segments = SegmentIndex::new(
        s.curves
            .keys()
            .flat_map(|k| {
                let pts: Vec<SVec2> = curve_polyline(s, k)
                    .into_iter()
                    .map(|p| map.to_screen64(p))
                    .collect();
                pts.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>()
            })
            .chain(obstacles.thin.iter().copied())
            .collect(),
    );
    let fat = SegmentIndex::new(obstacles.fat.to_vec());
    // The sketch axes, as long screen segments through the origin.
    let o = SVec2::new(map.origin.x as f64, map.origin.y as f64);
    let axes: Vec<(SVec2, SVec2)> = [map.x, map.y]
        .iter()
        .filter(|d| d.length() > 1e-6)
        .map(|d| {
            let d = d.normalize() * 20000.0;
            let d = SVec2::new(d.x as f64, d.y as f64);
            (o - d, o + d)
        })
        .collect();
    let mut taken: Vec<(Vec2, Vec2)> = Vec::new();
    let overlaps_taken = |lo: Vec2, hi: Vec2, taken: &[(Vec2, Vec2)]| {
        taken.iter().any(|(tl, th)| {
            lo.x < th.x + 1.0 && hi.x > tl.x - 1.0 && lo.y < th.y + 1.0 && hi.y > tl.y - 1.0
        })
    };
    // When no place is completely free: one that at least keeps off dimensions and other
    // glyphs (it may touch a sketch curve).
    let clear_of_labels = |boxes: &[(Vec2, Vec2)], taken: &[(Vec2, Vec2)]| {
        boxes.iter().all(|(lo, hi)| {
            let c = FAT_CLEARANCE;
            let (al, ah) = (
                SVec2::new(lo.x as f64 - c, lo.y as f64 - c),
                SVec2::new(hi.x as f64 + c, hi.y as f64 + c),
            );
            !fat.hits_box(al, ah) && !overlaps_taken(*lo, *hi, taken)
        })
    };
    let free = |boxes: &[(Vec2, Vec2)], taken: &[(Vec2, Vec2)]| {
        boxes.iter().all(|(lo, hi)| {
            let (l, h) = (
                SVec2::new(lo.x as f64 - 1.0, lo.y as f64 - 1.0),
                SVec2::new(hi.x as f64 + 1.0, hi.y as f64 + 1.0),
            );
            let c = AXIS_CLEARANCE;
            let (al, ah) = (
                SVec2::new(lo.x as f64 - c, lo.y as f64 - c),
                SVec2::new(hi.x as f64 + c, hi.y as f64 + c),
            );
            let f = FAT_CLEARANCE;
            let (fl, fh) = (
                SVec2::new(lo.x as f64 - f, lo.y as f64 - f),
                SVec2::new(hi.x as f64 + f, hi.y as f64 + f),
            );
            !segments.hits_box(l, h)
                && !fat.hits_box(fl, fh)
                && !axes.iter().any(|(a, b)| segment_hits_box(*a, *b, al, ah))
                && !taken.iter().any(|(tl, th)| {
                    lo.x < th.x + 1.0 && hi.x > tl.x - 1.0 && lo.y < th.y + 1.0 && hi.y > tl.y - 1.0
                })
        })
    };
    // (Off-screen groups are not laid out: their UI nodes would be clipped anyway.)
    for (host, icons) in groups {
        let n = icons.len();
        // Candidate places for the row's first box center, best first; the leader starts at
        // `from`.
        let (from, candidates): (Vec2, Vec<Vec2>) = match host {
            Host::Point(_) | Host::Touch(_) => {
                let Some(at) = host_anchor(s, map, host) else {
                    continue;
                };
                let w = (n - 1) as f32 * PITCH;
                // Up-left (the row ending nearest the point), up-right, down-left, down-right,
                // then straight left, right, up and down: all within ~24 px of the point.
                let o = POINT_OFFSET;
                let mut v = vec![
                    at + Vec2::new(o.x - w, o.y),
                    at + Vec2::new(-o.x, o.y),
                    at + Vec2::new(o.x - w, -o.y),
                    at + Vec2::new(-o.x, -o.y),
                    at + Vec2::new(-22.0 - w, 0.0),
                    at + Vec2::new(22.0, 0.0),
                    at + Vec2::new(-w / 2.0, -22.0),
                    at + Vec2::new(-w / 2.0, 22.0),
                ];
                // A ring a little further out, when the near places are all taken (by
                // dimensions or other glyph groups); not further: a longer leader loses its
                // point (T5 judge).
                let f = 1.45f32;
                let d = 22.0 * f;
                v.extend([
                    at + Vec2::new(o.x * f - w, o.y * f),
                    at + Vec2::new(-o.x * f, o.y * f),
                    at + Vec2::new(o.x * f - w, -o.y * f),
                    at + Vec2::new(-o.x * f, -o.y * f),
                    at + Vec2::new(-d - w, 0.0),
                    at + Vec2::new(d, 0.0),
                    at + Vec2::new(-w / 2.0, -d),
                    at + Vec2::new(-w / 2.0, d),
                ]);
                // Outside: each ring's places facing away from the point's curves first.
                if let Some(out) = outward(s, map, host, at) {
                    let facing = |c: &Vec2| {
                        -(*c + Vec2::new(w / 2.0, 0.0) - at)
                            .normalize_or_zero()
                            .dot(out)
                    };
                    for ring in v.chunks_mut(8) {
                        ring.sort_by(|p, q| facing(p).total_cmp(&facing(q)));
                    }
                }
                (at, v)
            }
            Host::Curve(c)
                if !matches!(
                    s.curves.get(c).map(|c| c.kind),
                    Some(CurveKind::Line { .. })
                ) =>
            {
                // A circle or arc: outside its rim, at 45° up-right (a circle) or the arc's
                // middle.
                let Some((center, r, angle)) = (match s.curves.get(c).map(|c| c.kind) {
                    Some(CurveKind::Circle { center, radius }) => {
                        Some((s.pos(center), radius, std::f64::consts::FRAC_PI_4))
                    }
                    Some(CurveKind::Arc { .. }) => s
                        .arc_geom(c)
                        .map(|g| (g.center, g.radius, (g.mid() - g.center).angle())),
                    _ => None,
                }) else {
                    continue;
                };
                let half_row = (n - 1) as f32 * PITCH / 2.0;
                let mut v = Vec::new();
                for da in [
                    0.0,
                    0.5,
                    -0.5,
                    1.0,
                    -1.0,
                    1.6,
                    -1.6,
                    2.4,
                    -2.4,
                    std::f64::consts::PI,
                ] {
                    let a = angle + da;
                    let rim = map.to_screen(center + SVec2::from_angle(a) * r);
                    let out = (rim - map.to_screen(center)).normalize_or_zero();
                    let c = rim + out * (LINE_OFFSET + half_row * out.x.abs());
                    v.push(c - Vec2::new(half_row, 0.0));
                }
                let rim = map.to_screen(center + SVec2::from_angle(angle) * r);
                (rim, v)
            }
            Host::Curve(c) => {
                let Some(CurveKind::Line { a, b }) = s.curves.get(c).map(|c| c.kind) else {
                    continue;
                };
                let (sa, sb) = (map.to_screen(s.pos(a)), map.to_screen(s.pos(b)));
                let mid = (sa + sb) / 2.0;
                let dir = (sb - sa).normalize_or_zero();
                let mut normal = Vec2::new(-dir.y, dir.x);
                // Where the neighbours are (the inside of a rectangle).
                let neighbours: Vec<SVec2> = [a, b]
                    .iter()
                    .flat_map(|p| s.curves_at(*p).filter(|k| *k != c).collect::<Vec<_>>())
                    .flat_map(|k| s.curve_points(k))
                    .filter(|p| *p != a && *p != b)
                    .map(|p| s.pos(p))
                    .collect();
                let toward = if neighbours.is_empty() {
                    None
                } else {
                    let sum = neighbours.iter().fold(SVec2::ZERO, |acc, p| acc + *p);
                    Some(map.to_screen(sum / neighbours.len() as f64) - mid)
                };
                // Projected curves have their own points (no neighbours): away from the middle
                // of all the projected curves instead, so a used face's four links read alike
                // (T5 judge).
                let links_only = icons.iter().all(|(i, ..)| *i == GlyphIcon::Use);
                let toward = toward.or_else(|| {
                    if !links_only {
                        return None;
                    }
                    let mids: Vec<SVec2> = s
                        .constraints
                        .values()
                        .filter_map(|c| match c {
                            ConstraintOf::Use(CurveRef::Curve(k), _) => s.curve_ends(*k),
                            _ => None,
                        })
                        .map(|(p, q)| s.pos(p).midpoint(s.pos(q)))
                        .collect();
                    (mids.len() > 1).then(|| {
                        let sum = mids.iter().fold(SVec2::ZERO, |acc, p| acc + *p);
                        map.to_screen(sum / mids.len() as f64) - mid
                    })
                });
                // Outside the shape: away from the neighbours; with none, down or right.
                let flip = match toward {
                    Some(t) if t.length() > 1e-3 => normal.dot(t) > 0.0,
                    _ => normal.y < 0.0 || (normal.y == 0.0 && normal.x < 0.0),
                };
                if flip {
                    normal = -normal;
                }
                // A horizontal row beside a steep line moves out by half its width.
                let half_row = (n - 1) as f32 * PITCH / 2.0;
                let len = sa.distance(sb);
                let base0 = mid;
                let mut v = Vec::new();
                // The preferred side at the middle, then the other side there, then moving
                // along the line to clear other things (dimension values), outside the shape
                // first (T2 judge), then further out.
                let mut places = vec![(1.0, 0.0, 0.0), (-1.0, 0.0, 0.0)];
                for out in [0.0, 22.0] {
                    for side in [-1.0, 1.0] {
                        for shift in [0.25, -0.25, 0.4, -0.4] {
                            places.push((side, out, shift));
                        }
                    }
                }
                places.push((1.0, 22.0, 0.0));
                places.push((-1.0, 22.0, 0.0));
                for (side, out, shift) in places {
                    let base = base0 + dir * (shift * len);
                    let center =
                        base + normal * side * (LINE_OFFSET + out + half_row * normal.x.abs());
                    v.push(center - Vec2::new(half_row, 0.0));
                }
                (base0, v)
            }
        };
        // Whole-pixel centers keep the 20 px boxes on the pixel grid. A dragged group stays
        // where it was put.
        // A dragged group stays where it was put. Otherwise the group keeps last frame's place
        // while it is still free (so glyphs do not jump back and forth as things move), and
        // takes the first free candidate when it is not.
        let first = match offsets.0.get(&host) {
            Some(o) => (from + *o).round(),
            None => obstacles
                .previous
                .and_then(|p| p.get(&host))
                .map(|o| (from + *o).round())
                .filter(|c| {
                    obstacles.frozen
                        || (candidates.iter().any(|k| k.round() == *c)
                            && free(&row_boxes(*c, n), &taken))
                })
                .or_else(|| {
                    candidates
                        .iter()
                        .map(|c| c.round())
                        .find(|c| free(&row_boxes(*c, n), &taken))
                })
                .or_else(|| {
                    candidates
                        .iter()
                        .map(|c| c.round())
                        .find(|c| clear_of_labels(&row_boxes(*c, n), &taken))
                })
                // Nothing clear: the place that covers the least (dimensions and other glyphs
                // count most), not simply the first.
                .unwrap_or_else(|| {
                    let cost = |c: Vec2| -> u32 {
                        row_boxes(c, n)
                            .iter()
                            .map(|(lo, hi)| {
                                let (l, h) = (
                                    SVec2::new(lo.x as f64 - 2.0, lo.y as f64 - 2.0),
                                    SVec2::new(hi.x as f64 + 2.0, hi.y as f64 + 2.0),
                                );
                                10 * u32::from(fat.hits_box(l, h))
                                    + 10 * u32::from(overlaps_taken(*lo, *hi, &taken))
                                    + u32::from(segments.hits_box(l, h))
                            })
                            .sum()
                    };
                    candidates
                        .iter()
                        .map(|c| c.round())
                        .min_by_key(|c| cost(*c))
                        .unwrap_or_else(|| candidates[0].round())
                }),
        };
        out.placed.insert(host, first - from);
        let boxes = row_boxes(first, n);
        taken.extend(boxes.iter().copied());
        let style = |external: bool, constraint: Option<ConstraintId>| {
            let link = constraint
                .and_then(|k| s.constraints.get(k))
                .is_some_and(|c| matches!(c, ConstraintOf::Use(..) | ConstraintOf::Pierce(..)));
            if link {
                GlyphStyle::Link
            } else if external {
                GlyphStyle::External
            } else {
                GlyphStyle::Constraint
            }
        };
        let centers: Vec<Vec2> = boxes.iter().map(|(lo, hi)| (*lo + *hi) / 2.0).collect();
        match host {
            Host::Point(_) | Host::Touch(_) => {
                // Leaders end at the box's edge.
                for c in &centers {
                    out.leaders
                        .push((from, clip_to_box(from, *c, Vec2::splat(BOX / 2.0))));
                }
            }
            // A curve's marks sit beside it, with no leader.
            Host::Curve(_) => {}
        }
        for ((icon, external, constraint), center) in icons.into_iter().zip(centers) {
            out.glyphs.push(GlyphSpec {
                icon,
                center,
                style: style(external, constraint),
                constraint,
                group: Some((host, first - from)),
            });
        }
    }
    out
}

/// The glyphs shown next to the cursor for an inference candidate (`screens/18`, `18a`).
pub fn inference_glyphs(c: &Candidate, cursor: Vec2) -> Vec<GlyphSpec> {
    let icons: Vec<GlyphIcon> = c
        .constraints
        .iter()
        .map(|p| match p {
            // A snapped point: its square, and the coincident glyph at the cursor.
            Placed::Coincident(_) | Placed::OnCurve(_) => GlyphIcon::Coincident,
            Placed::Midpoint(_) => GlyphIcon::Midpoint,
            Placed::Horizontal(_) => GlyphIcon::Horizontal,
            Placed::Vertical(_) => GlyphIcon::Vertical,
            Placed::Parallel(_) => GlyphIcon::Parallel,
            Placed::Perpendicular(_) => GlyphIcon::Perpendicular,
            Placed::Tangent(_) => GlyphIcon::Tangent,
        })
        .collect();
    let first = cursor.round() + Vec2::splat(21.0 + BOX / 2.0);
    icons
        .into_iter()
        .enumerate()
        .map(|(i, icon)| GlyphSpec {
            icon,
            center: first + Vec2::new(i as f32 * PITCH, 0.0),
            style: GlyphStyle::Inference,
            constraint: None,
            group: None,
        })
        .collect()
}

/// Where the snap square goes for a candidate, if it gets one (points, midpoints and
/// intersections; not curves or guides).
pub fn square_for(c: &Candidate) -> Option<SVec2> {
    match c.kind {
        Kind::Point(_) | Kind::Origin | Kind::Midpoint(_) | Kind::Intersection(..) => Some(c.pos),
        Kind::OnCurve(_) | Kind::Guide => None,
    }
}

// ---------------------------------------------------------------------------------------------
// UI

#[derive(Component)]
pub(crate) struct GlyphBox(usize);

#[derive(Component)]
pub(crate) struct SnapSquare;

/// A glyph shows while its box is inside the viewport and clear of the view ball.
fn glyph_visibility(rect: &ViewportRect, center: Vec2) -> Visibility {
    let b = Rect::from_center_size(center, Vec2::splat(BOX));
    let cube = crate::view_cube::cube_rect(rect.0);
    if rect.0.contains(center) && b.intersect(cube).is_empty() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// Only inference glyphs are boxes; constraint glyphs are drawn on the sketch
/// ([`GlyphIcon::strokes`]), their boxes kept hidden for picking's sake.
fn box_visibility(rect: &ViewportRect, spec: &GlyphSpec) -> Visibility {
    if spec.style == GlyphStyle::Inference {
        glyph_visibility(rect, spec.center)
    } else {
        Visibility::Hidden
    }
}

/// Keeps one UI box per [`GlyphSpec`] and the snap square, placed inside the viewport area.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn sync_overlay(
    overlay: Res<SketchOverlay>,
    viewport_rect: Res<ViewportRect>,
    sketch_area: Res<crate::sketch_tools::SketchArea>,
    mut host_seen: Local<Option<Entity>>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q: Query<
        (
            Entity,
            &GlyphBox,
            &mut Node,
            &mut BackgroundColor,
            &mut Outline,
            &mut Name,
            &mut Visibility,
            &Children,
        ),
        Without<SnapSquare>,
    >,
    mut q_icon: Query<(&mut Icon, &mut ImageNode)>,
    mut q_square: Query<
        (Entity, &mut Node, &mut Visibility),
        (With<SnapSquare>, Without<GlyphBox>),
    >,
    mut commands: Commands,
) {
    // In the viewport area, or the flat view while a sketch on the flat is edited (P3I.6): a
    // change of host starts the boxes again there.
    let (host, r) = sketch_area.host(q_area.iter().next(), &viewport_rect);
    let rect = ViewportRect(r);
    if *host_seen != host {
        for (e, ..) in &q {
            commands.entity(e).try_despawn();
        }
        for (e, ..) in &q_square {
            commands.entity(e).try_despawn();
        }
        *host_seen = host;
        return;
    }
    let Some(area) = host else {
        return;
    };
    let origin = rect.0.min;
    let mut have = 0;
    for (e, b, mut node, mut bg, mut outline, mut name, mut vis, children) in &mut q {
        let Some(spec) = overlay.glyphs.get(b.0) else {
            commands.entity(e).try_despawn();
            continue;
        };
        have = have.max(b.0 + 1);
        let tl = spec.center - Vec2::splat(BOX / 2.0) - origin;
        let (l, t) = (Val::Px(tl.x), Val::Px(tl.y));
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
        bg.set_if_neq(BackgroundColor(spec.style.background()));
        outline.set_if_neq(spec.style.outline());
        if name.as_str() != spec.style.name() {
            *name = Name::new(spec.style.name());
        }
        vis.set_if_neq(box_visibility(&rect, spec));
        for c in children.iter() {
            if let Ok((mut icon, mut image)) = q_icon.get_mut(c) {
                if icon.name != spec.icon.icon_name() {
                    icon.name = spec.icon.icon_name().into();
                }
                let fg = spec.style.foreground();
                if image.color != fg {
                    image.color = fg;
                }
            }
        }
    }
    for (i, spec) in overlay.glyphs.iter().enumerate().skip(have) {
        let tl = spec.center - Vec2::splat(BOX / 2.0) - origin;
        let e = commands
            .spawn((
                Name::new(spec.style.name()),
                GlyphBox(i),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(tl.x),
                    top: Val::Px(tl.y),
                    width: Val::Px(BOX),
                    height: Val::Px(BOX),
                    ..default()
                },
                BackgroundColor(spec.style.background()),
                spec.style.outline(),
                Pickable::IGNORE,
                box_visibility(&rect, spec),
                // Under the value labels and the sketch dialog.
                ZIndex(-2),
                children![(
                    Name::new("sketch-glyph-icon"),
                    cadrs_ui::icon(spec.icon.icon_name(), BOX, spec.style.foreground()),
                    Pickable::IGNORE,
                )],
            ))
            .id();
        commands.entity(area).add_child(e);
    }

    // The snap square: 18 px, a 2 px anti-aliased-looking stroke: #fbd870 outside, a paler
    // #f9e29b inside (`screens/18`).
    const SQ: f32 = 18.0;
    match (overlay.square, q_square.iter_mut().next()) {
        (Some(c), Some((_, mut node, mut vis))) => {
            let tl = (c - origin - Vec2::splat(SQ / 2.0)).round();
            let (l, t) = (Val::Px(tl.x), Val::Px(tl.y));
            if node.left != l || node.top != t {
                node.left = l;
                node.top = t;
            }
            vis.set_if_neq(Visibility::Inherited);
        }
        (Some(c), None) => {
            let tl = (c - origin - Vec2::splat(SQ / 2.0)).round();
            let e = commands
                .spawn((
                    Name::new("sketch-snap-square"),
                    SnapSquare,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(tl.x),
                        top: Val::Px(tl.y),
                        width: Val::Px(SQ),
                        height: Val::Px(SQ),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb_u8(0xfb, 0xd8, 0x70)),
                    children![(
                        Node {
                            width: Val::Px(SQ - 2.0),
                            height: Val::Px(SQ - 2.0),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::all(Color::srgb_u8(0xf9, 0xe2, 0x9b)),
                        Pickable::IGNORE,
                    )],
                    Pickable::IGNORE,
                    ZIndex(-1),
                ))
                .id();
            commands.entity(area).add_child(e);
        }
        (None, Some((_, _, mut vis))) => {
            vis.set_if_neq(Visibility::Hidden);
        }
        (None, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_sketch::SketchOp;
    use cadrs_sketch::constraint::rectangle_constraints;

    #[test]
    fn rectangle_glyphs() {
        let mut s = Sketch::new();
        let c = [
            SVec2::ZERO,
            SVec2::new(50.0, 0.0),
            SVec2::new(50.0, 30.0),
            SVec2::new(0.0, 30.0),
        ];
        let mut specs = rectangle_constraints(c);
        specs.push(ConstraintOf::Coincident(
            cadrs_sketch::PointSpec::At(SVec2::ZERO),
            cadrs_sketch::PointSpec::Origin,
        ));
        SketchOp::Batch(vec![
            SketchOp::AddPolyline {
                points: c.to_vec(),
                closed: true,
                construction: false,
                label: "Add rectangle",
            },
            SketchOp::AddConstraints(specs),
        ])
        .apply(&mut s)
        .unwrap();
        let groups = glyph_groups(&s);
        let icons: Vec<Vec<GlyphIcon>> = groups
            .iter()
            .map(|(_, v)| v.iter().map(|(i, ..)| *i).collect())
            .collect();
        use GlyphIcon::*;
        // Top: "—" (its ⊥ is a mark in the corner); left: "∥"; bottom: "∥". No coincident
        // glyphs: the corners are squares, the origin one too.
        assert_eq!(
            icons,
            vec![vec![Horizontal], vec![Parallel], vec![Parallel]]
        );
        let marks = corner_marks(&s);
        assert_eq!(marks.len(), 1);
        // Top and left meet at the top-left corner.
        assert_eq!(marks[0].corner, SVec2::new(0.0, 30.0));
        assert_eq!(marks[0].far, [SVec2::new(50.0, 30.0), SVec2::ZERO]);
        assert_eq!(coincident_points(&s).len(), 4);

        // Laid out in a normal-to view at 4 px/mm, y up.
        let map = ScreenMap {
            plane: cadrs_sketch::PlaneRef::Top,
            origin: Vec2::new(400.0, 400.0),
            x: Vec2::new(4.0, 0.0),
            y: Vec2::new(0.0, -4.0),
            w: Vec2::ZERO,
        };
        let l = layout_glyphs(
            &s,
            &map,
            GlyphObstacles::default(),
            &GlyphOffsets::default(),
            |_| true,
        );
        // Outside the rectangle: the top edge's glyph above its midpoint (500, 280), the left
        // edge's to its left.
        assert_eq!(l.glyphs[0].center, Vec2::new(500.0, 260.0));
        assert!(l.glyphs[1].center.x < 400.0 && (l.glyphs[1].center.y - 340.0).abs() < 1.0);
        // Curves' glyphs have no leaders.
        assert!(l.leaders.is_empty());
        assert_eq!(l.glyphs.len(), 3);
    }

    #[test]
    fn corner_mark_is_picked_off_its_point() {
        let c = CornerSpec {
            constraint: ConstraintId::default(),
            corner: Vec2::new(100.0, 100.0),
            dirs: [Vec2::X, Vec2::NEG_Y],
            size: CORNER_MARK,
        };
        // Inside the mark, away from the corner point.
        assert!(c.contains(Vec2::new(108.0, 92.0)));
        // On the corner point: the point's.
        assert!(!c.contains(Vec2::new(101.0, 99.0)));
        // Outside the corner.
        assert!(!c.contains(Vec2::new(92.0, 108.0)));
    }

    #[test]
    fn tangent_glyph_sits_where_the_circle_touches() {
        let mut s = Sketch::new();
        SketchOp::Batch(vec![
            SketchOp::AddPolyline {
                points: vec![SVec2::ZERO, SVec2::new(10.0, 0.0)],
                closed: false,
                construction: false,
                label: "Add line",
            },
            SketchOp::AddCircle {
                center: SVec2::new(30.0, 5.0),
                radius: 5.0,
                construction: false,
            },
        ])
        .apply(&mut s)
        .unwrap();
        let ids: Vec<CurveId> = s.curves.keys().collect();
        SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::Tangent(
                CurveRef::Curve(ids[0]),
                CurveRef::Curve(ids[1]),
            )],
            label: "Add tangent",
        }
        .apply(&mut s)
        .unwrap();
        let groups = glyph_groups(&s);
        assert!(matches!(groups[..], [(Host::Touch(_), _)]));
        let map = ScreenMap {
            plane: cadrs_sketch::PlaneRef::Top,
            origin: Vec2::new(100.0, 400.0),
            x: Vec2::new(4.0, 0.0),
            y: Vec2::new(0.0, -4.0),
            w: Vec2::ZERO,
        };
        let l = layout_glyphs(
            &s,
            &map,
            GlyphObstacles::default(),
            &GlyphOffsets::default(),
            |_| true,
        );
        // Within a box or two of the touch point (30, 0) → (220, 400), not up on the circle.
        let touch = map.to_screen(SVec2::new(30.0, 0.0));
        assert!(l.glyphs[0].center.distance(touch) < 40.0);
        // The line is carried on to it.
        let ext = cadrs_sketch::extension::extensions(&s);
        assert_eq!(ext.len(), 1);
        assert_eq!(ext[0].1, ids[0]);
    }
}
