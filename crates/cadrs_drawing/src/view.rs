//! Drawing views (P3C.2, D4, D7, X6, X7): a part or Part Studio seen from a direction, placed on
//! a sheet at a scale.
//!
//! # Placement
//! A view is placed by its **anchor**: the sheet point (mm) where the model origin lands. A model
//! point `p` is drawn at `anchor + R(rotation) · (scale · frame.to_2d(p))`. Views folded out of a
//! parent (projected orthographic and auxiliary views) share the fold line's coordinate with
//! their parent, so a child is **aligned** exactly when its anchor lies on the line through the
//! parent's anchor along the fold direction ([`View::fold`]). Dragging keeps that (D7.1): a
//! child only slides along the line, and moving a parent carries its children across it.
//!
//! # Folding
//! [`fold_frame`] turns a parent's frame into the frame of the view placed on side `n` of it
//! (a unit direction on the sheet): in **third angle** the view seen from side `n`, in **first
//! angle** the view seen from the opposite side (D1.5, D4.5, P6.2). The same fold makes the
//! orthographic projected views (`n` = right, up, left, down) and auxiliary views (`n` normal to
//! an edge, D4.6). Its 2D x and y stay along the sheet's, so a folded view is never rotated.

use cadrs_kernel::{Projection as Hlr, ProjClass, ProjCurve, ProjVisibility, ViewFrame};
use nalgebra::{Point3, Vector3};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::graphics::Weight;
use crate::standard::{Projection, Scale};
use crate::style::TangentEdges;
use crate::ObjectRef;

/// Identifies a view within a drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ViewId(pub Uuid);

impl ViewId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for ViewId {
    fn default() -> Self {
        Self::new()
    }
}

/// The named orientations of the Insert view dialog (D4.1). Model axes are Onshape's: Z up, the
/// Front plane XZ seen from −Y, the Right plane YZ seen from +X, the Top plane XY seen from +Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NamedView {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    Isometric,
}

impl NamedView {
    pub const ALL: [NamedView; 7] = [
        NamedView::Front,
        NamedView::Top,
        NamedView::Right,
        NamedView::Back,
        NamedView::Bottom,
        NamedView::Left,
        NamedView::Isometric,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NamedView::Front => "Front",
            NamedView::Back => "Back",
            NamedView::Top => "Top",
            NamedView::Bottom => "Bottom",
            NamedView::Left => "Left",
            NamedView::Right => "Right",
            NamedView::Isometric => "Isometric",
        }
    }

    /// The view frame: direction of sight and sheet-right.
    pub fn frame(self) -> Frame3 {
        let v = |x: f64, y: f64, z: f64| [x, y, z];
        match self {
            NamedView::Front => Frame3::new(v(0.0, 1.0, 0.0), v(1.0, 0.0, 0.0)),
            NamedView::Back => Frame3::new(v(0.0, -1.0, 0.0), v(-1.0, 0.0, 0.0)),
            NamedView::Top => Frame3::new(v(0.0, 0.0, -1.0), v(1.0, 0.0, 0.0)),
            NamedView::Bottom => Frame3::new(v(0.0, 0.0, 1.0), v(1.0, 0.0, 0.0)),
            NamedView::Right => Frame3::new(v(-1.0, 0.0, 0.0), v(0.0, 1.0, 0.0)),
            NamedView::Left => Frame3::new(v(1.0, 0.0, 0.0), v(0.0, -1.0, 0.0)),
            // Seen from (+X, −Y, +Z), the corner between Front, Right and Top, Z up.
            NamedView::Isometric => iso_from(Vector3::new(1.0, -1.0, 1.0), Vector3::z()),
        }
    }

    /// The named view with this frame, if any.
    pub fn of(frame: &Frame3) -> Option<NamedView> {
        NamedView::ALL.into_iter().find(|n| n.frame().same(frame))
    }
}

/// A view's orientation: the unit direction of sight (eye → model) and the unit sheet-right
/// direction, in model coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame3 {
    pub dir: [f64; 3],
    pub x: [f64; 3],
}

fn v3(a: [f64; 3]) -> Vector3<f64> {
    Vector3::new(a[0], a[1], a[2])
}

fn a3(v: Vector3<f64>) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The frame looking at the model from `eye` (a direction towards the viewer) with `up` up.
fn iso_from(eye: Vector3<f64>, up: Vector3<f64>) -> Frame3 {
    let dir = -eye.normalize();
    let up = (up - dir * up.dot(&dir)).normalize();
    // x × dir = up  ⇒  x = dir × up.
    let x = dir.cross(&up);
    Frame3 {
        dir: a3(dir),
        x: a3(x),
    }
}

impl Frame3 {
    pub fn new(dir: [f64; 3], x: [f64; 3]) -> Self {
        let f = ViewFrame::new(v3(dir), v3(x));
        Self {
            dir: a3(f.dir),
            x: a3(f.x),
        }
    }

    pub fn view_frame(&self) -> ViewFrame {
        ViewFrame {
            origin: Point3::origin(),
            dir: v3(self.dir),
            x: v3(self.x),
        }
    }

    pub fn up(&self) -> [f64; 3] {
        a3(self.view_frame().up())
    }

    /// The same orientation (within 1e-9).
    pub fn same(&self, o: &Frame3) -> bool {
        (v3(self.dir) - v3(o.dir)).norm() < 1e-9 && (v3(self.x) - v3(o.x)).norm() < 1e-9
    }

    /// A key for caches: the frame rounded to 1e-9.
    pub fn key(&self) -> [i64; 6] {
        let r = |v: f64| (v * 1e9).round() as i64;
        [
            r(self.dir[0]),
            r(self.dir[1]),
            r(self.dir[2]),
            r(self.x[0]),
            r(self.x[1]),
            r(self.x[2]),
        ]
    }
}

/// The frame of the view placed on side `n` (a unit direction in the parent's 2D frame) of a
/// view with frame `parent` (see the module docs).
pub fn fold_frame(parent: &Frame3, n: [f64; 2], projection: Projection) -> Frame3 {
    let f = parent.view_frame();
    let (d, x, u) = (f.dir, f.x, f.up());
    let len = (n[0] * n[0] + n[1] * n[1]).sqrt().max(1e-12);
    let n = [n[0] / len, n[1] / len];
    // The side `n` in model space, and the direction along the fold line (n turned -90°).
    let n3 = x * n[0] + u * n[1];
    let e = [n[1], -n[0]];
    let e3 = x * e[0] + u * e[1];
    // Third angle: seen from side n, the parent's direction of sight points away from the
    // parent on the sheet. First angle: seen from the other side, it points towards it.
    let (dir, away) = match projection {
        Projection::Third => (-n3, d),
        Projection::First => (n3, -d),
    };
    // Sheet x = n.x · (away) + e.x · e3; sheet y = n.y · away + e.y · e3.
    let sx = away * n[0] + e3 * e[0];
    Frame3::new(a3(dir), a3(sx))
}

/// The isometric view seen from the corner towards sheet quadrant `(sx, sy)` (±1 each) of a
/// parent view: from the parent's viewer side, up-right for (1, 1) (D4.4).
pub fn iso_frame(parent: &Frame3, sx: f64, sy: f64) -> Frame3 {
    let f = parent.view_frame();
    let eye = f.x * sx.signum() + f.up() * sy.signum() - f.dir;
    iso_from(eye, f.up())
}

/// What a projected view is, by the direction of the cursor from its parent (D4.4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Orthographic, folded out along this unit sheet direction (right, up, left or down).
    Ortho([f64; 2]),
    /// Isometric, towards this diagonal quadrant.
    Iso(f64, f64),
}

/// The projected view the cursor asks for when it is `offset` (sheet mm) from the parent's
/// centre: the nearest of the four sides, or a diagonal (isometric) between 22.5° and 67.5°
/// off an axis. `None` too close to the parent.
pub fn placement_for(offset: [f64; 2]) -> Option<Placement> {
    let (dx, dy) = (offset[0], offset[1]);
    if dx.hypot(dy) < 1.0 {
        return None;
    }
    let a = dy.atan2(dx).to_degrees().rem_euclid(360.0);
    let sector = ((a + 22.5) / 45.0).floor() as i32 % 8;
    Some(match sector {
        0 => Placement::Ortho([1.0, 0.0]),
        2 => Placement::Ortho([0.0, 1.0]),
        4 => Placement::Ortho([-1.0, 0.0]),
        6 => Placement::Ortho([0.0, -1.0]),
        _ => Placement::Iso(dx.signum(), dy.signum()),
    })
}

/// What kind of view it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ViewKind {
    /// The first view of a reference, placed from the Insert view dialog.
    Base,
    /// Projected from a parent: orthographic (aligned) or isometric.
    Projected,
    /// Folded out from an edge of the parent (D4.6).
    Auxiliary,
    /// A section of the parent along a cutting line (P3C.8, D4.12).
    Section,
    /// The parent inside a circle, at its own scale (P3C.8, D4.12).
    Detail,
}

/// A view on a sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub id: ViewId,
    /// "Front", "Top", "Isometric", "Auxiliary"… (the Sheets flyout shows it).
    pub name: String,
    pub kind: ViewKind,
    /// The view it was projected from (on this sheet or, after Move to sheet, another).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ViewId>,
    /// The fold direction (unit, in the parent's unrotated 2D frame) of an aligned view:
    /// orthographic projected and auxiliary views. `None` for base and isometric views.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fold: Option<[f64; 2]>,
    /// The part or Part Studio shown.
    pub reference: ObjectRef,
    pub frame: Frame3,
    /// Where the model origin is on the sheet (mm).
    pub anchor: [f64; 2],
    /// Counter-clockwise turn on the sheet (radians), from Align view vertical/horizontal.
    #[serde(default)]
    pub rotation: f64,
    pub scale: Scale,
    /// The scale follows the parent's (projected views, D4.7).
    #[serde(default)]
    pub scale_inherited: bool,
    pub hidden_lines: bool,
    pub tangent_edges: TangentEdges,
    #[serde(default)]
    pub shaded: bool,
    #[serde(default)]
    pub part_intersections: bool,
    /// "Suppress alignment with parent" (D7.1).
    #[serde(default)]
    pub align_suppressed: bool,
    /// The sketches drawn over the view (Show/hide sketches…, D7.4): sketch feature ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketches: Vec<Uuid>,
    /// The view's annotations (P3C.3): they move, scale and change sheets with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<crate::annotation::Annotation>,
    /// The dependency hash of the model state the view shows (P3C.6, D13.2): the hash of its
    /// referenced part (or Part Studio) in the drawing's [`crate::ModelSource`] when the view
    /// was last generated. The view is out of date when the workspace's hash differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<u64>,
    /// Material removed before projecting (section views, P3C.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<crate::view_kinds::ViewCut>,
    /// A section view's cutting line (drawn on its parent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<crate::view_kinds::SectionLine>,
    /// A detail view's circle (drawn on its parent; the detail is clipped to it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<crate::view_kinds::DetailCircle>,
    /// Crop view: only what is inside is shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<crate::view_kinds::Boundary>,
    /// Break view: bands removed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<crate::view_kinds::Break>,
    /// A broken-out section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken_out: Option<crate::view_kinds::BrokenOut>,
    /// Show threads (D4.8): tapped holes' thread marks.
    #[serde(default)]
    pub threads: bool,
    /// A flat pattern view (P3I.7, SM16): the referenced sheet metal part laid flat, with its
    /// bend lines and notes (see [`crate::flat_view`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flat: Option<crate::flat_view::FlatSettings>,
}

impl View {
    /// A base view.
    pub fn base(reference: ObjectRef, orientation: NamedView, scale: Scale, anchor: [f64; 2]) -> Self {
        Self {
            id: ViewId::new(),
            name: orientation.label().to_string(),
            kind: ViewKind::Base,
            parent: None,
            fold: None,
            reference,
            frame: orientation.frame(),
            anchor,
            rotation: 0.0,
            scale,
            scale_inherited: false,
            hidden_lines: false,
            tangent_edges: TangentEdges::Solid,
            shaded: false,
            part_intersections: false,
            align_suppressed: false,
            sketches: Vec::new(),
            annotations: Vec::new(),
            source_hash: None,
            cut: None,
            section: None,
            detail: None,
            crop: None,
            breaks: Vec::new(),
            broken_out: None,
            threads: false,
            flat: None,
        }
    }

    /// A flat pattern view of a sheet metal part (P3I.7, SM16.2): only the Insert view dialog's
    /// Flat patterns filter makes one.
    pub fn flat_pattern(reference: ObjectRef, orientation: NamedView, scale: Scale, anchor: [f64; 2]) -> Self {
        let mut v = Self::base(reference, orientation, scale, anchor);
        v.name = "Flat pattern".to_string();
        v.flat = Some(crate::flat_view::FlatSettings::default());
        // Onshape's flat views show the bends' chain lines clean, without their tangent lines
        // (`ex3-drawings/goal.png`); Tangent edges → Solid or Phantom shows them.
        v.tangent_edges = TangentEdges::Hidden;
        v
    }

    /// Whether it shows a flat pattern (or is projected from one).
    pub fn is_flat(&self) -> bool {
        self.flat.is_some()
    }

    /// A model point on the sheet (through the view's breaks, P3C.8).
    pub fn to_sheet(&self, p2: [f64; 2]) -> [f64; 2] {
        to_sheet(self.anchor, self.rotation, self.scale.factor(), crate::view_kinds::break_forward(self, p2))
    }

    /// A sheet point in the view's 2D frame (model mm).
    pub fn from_sheet(&self, s: [f64; 2]) -> [f64; 2] {
        let k = self.scale.factor();
        let (dx, dy) = (s[0] - self.anchor[0], s[1] - self.anchor[1]);
        let (c, sn) = (self.rotation.cos(), self.rotation.sin());
        crate::view_kinds::break_inverse(self, [(c * dx + sn * dy) / k, (-sn * dx + c * dy) / k])
    }

    /// The material removed before projecting: a section's cut, or a broken-out section's.
    pub fn effective_cut(&self) -> Option<crate::view_kinds::ViewCut> {
        if let Some(c) = &self.cut {
            return Some(c.clone());
        }
        self.broken_out.as_ref().map(|b| crate::view_kinds::ViewCut { depth: b.depth, polygon: b.boundary.polygon() })
    }

    /// Whether only part of the projection is shown (crop, detail, breaks): lines are then
    /// polylines, not whole arcs.
    pub fn clipped(&self) -> bool {
        self.crop.is_some() || self.detail.is_some() || !self.breaks.is_empty()
    }

    /// Drops what a view made from this one must not inherit (P3C.8).
    fn clear_special(&mut self) {
        self.cut = None;
        self.section = None;
        self.detail = None;
        self.crop = None;
        self.breaks.clear();
        self.broken_out = None;
    }

    /// The fold direction on the sheet (the parent's rotation applied), if aligned.
    pub fn fold_on_sheet(&self, parent_rotation: f64) -> Option<[f64; 2]> {
        self.fold.map(|n| rotate(n, parent_rotation))
    }

    /// Whether this view is kept aligned with its parent.
    pub fn aligned(&self) -> bool {
        self.fold.is_some() && self.parent.is_some() && !self.align_suppressed
    }
}

/// `anchor + R(rotation) · (k · p)`.
pub fn to_sheet(anchor: [f64; 2], rotation: f64, k: f64, p: [f64; 2]) -> [f64; 2] {
    let q = rotate([p[0] * k, p[1] * k], rotation);
    [anchor[0] + q[0], anchor[1] + q[1]]
}

pub fn rotate(p: [f64; 2], a: f64) -> [f64; 2] {
    let (c, s) = (a.cos(), a.sin());
    [c * p[0] - s * p[1], s * p[0] + c * p[1]]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// The name of a projected view from its frame: the named view it matches, else
/// "Projected view".
pub fn projected_name(frame: &Frame3) -> String {
    match NamedView::of(frame) {
        Some(n) => n.label().to_string(),
        None => "Projected view".to_string(),
    }
}

/// The view projected from `parent` towards `placement`, its anchor on the fold line through
/// the parent's anchor nearest `cursor` (orthographic), or with its centre at `cursor`
/// (isometric; `center2d` is the centre of the view's projection in its 2D frame, if known).
pub fn projected_view(
    parent: &View,
    placement: Placement,
    projection: Projection,
    cursor: [f64; 2],
    center2d: Option<[f64; 2]>,
) -> View {
    let mut v = parent.clone();
    v.id = ViewId::new();
    v.kind = ViewKind::Projected;
    v.parent = Some(parent.id);
    v.scale_inherited = true;
    v.align_suppressed = false;
    v.sketches.clear();
    v.annotations.clear();
    v.shaded = false;
    v.clear_special();
    match placement {
        Placement::Ortho(n) => {
            v.frame = fold_frame(&parent.frame, n, projection);
            v.fold = Some(n);
            v.anchor = aligned_anchor(parent.anchor, rotate(n, parent.rotation), cursor, None);
        }
        Placement::Iso(sx, sy) => {
            v.frame = iso_frame(&parent.frame, sx, sy);
            v.fold = None;
            let c = center2d.unwrap_or([0.0, 0.0]);
            let off = rotate([c[0] * v.scale.factor(), c[1] * v.scale.factor()], v.rotation);
            v.anchor = [cursor[0] - off[0], cursor[1] - off[1]];
        }
    }
    v.name = projected_name(&v.frame);
    v
}

/// The auxiliary view folded out of `parent` about an edge with 2D direction `edge` (in the
/// parent's frame) on the side of `cursor` (D4.6).
pub fn auxiliary_view(parent: &View, edge: [f64; 2], projection: Projection, cursor: [f64; 2]) -> View {
    let len = (edge[0] * edge[0] + edge[1] * edge[1]).sqrt().max(1e-12);
    let e = [edge[0] / len, edge[1] / len];
    // The normal on the cursor's side.
    let mut n = [-e[1], e[0]];
    let local = parent.from_sheet(cursor);
    if dot(n, local) < 0.0 {
        n = [-n[0], -n[1]];
    }
    let mut v = parent.clone();
    v.id = ViewId::new();
    v.kind = ViewKind::Auxiliary;
    v.parent = Some(parent.id);
    v.scale_inherited = true;
    v.align_suppressed = false;
    v.sketches.clear();
    v.annotations.clear();
    v.shaded = false;
    v.clear_special();
    v.frame = fold_frame(&parent.frame, n, projection);
    v.fold = Some(n);
    v.anchor = aligned_anchor(parent.anchor, rotate(n, parent.rotation), cursor, None);
    v.name = "Auxiliary view".to_string();
    v
}

/// The point on the line through `origin` along unit `n` nearest `p` (or at `t` along it).
pub fn aligned_anchor(origin: [f64; 2], n: [f64; 2], p: [f64; 2], t: Option<f64>) -> [f64; 2] {
    let t = t.unwrap_or_else(|| dot([p[0] - origin[0], p[1] - origin[1]], n));
    [origin[0] + n[0] * t, origin[1] + n[1] * t]
}

/// The "Four views" template option (X2): Front, the top and side views folded from it by the
/// projection method, and an isometric view in the free corner, at the largest common scale
/// that fits them in `area` (sheet mm) with `gap` between them. `bounds` gives the 2D bounds
/// (model mm) of the reference seen through a frame. Views take `hidden_lines` and `tangent`
/// from the drawing's properties; the isometric view is shaded.
pub fn four_views(
    reference: ObjectRef,
    projection: Projection,
    area: crate::standard::Rect,
    gap: f64,
    hidden_lines: bool,
    tangent: TangentEdges,
    bounds: impl Fn(&Frame3) -> Option<([f64; 2], [f64; 2])>,
) -> Vec<View> {
    let front_frame = NamedView::Front.frame();
    let (up, side_dir) = match projection {
        Projection::Third => ([0.0, 1.0], [1.0, 0.0]),
        Projection::First => ([0.0, -1.0], [1.0, 0.0]),
    };
    let top_frame = fold_frame(&front_frame, up, projection);
    let side_frame = fold_frame(&front_frame, side_dir, projection);
    let iso = NamedView::Isometric.frame();
    let unit = ([-10.0, -10.0], [10.0, 10.0]);
    let b = |f: &Frame3| bounds(f).unwrap_or(unit);
    let (bf, bt, bs, bi) = (b(&front_frame), b(&top_frame), b(&side_frame), b(&iso));
    let w = |b: ([f64; 2], [f64; 2])| b.1[0] - b.0[0];
    let h = |b: ([f64; 2], [f64; 2])| b.1[1] - b.0[1];
    let c = |b: ([f64; 2], [f64; 2])| [(b.0[0] + b.1[0]) / 2.0, (b.0[1] + b.1[1]) / 2.0];
    let col1 = w(bf).max(w(bt));
    let col2 = w(bs).max(w(bi));
    let row_front = h(bf).max(h(bs));
    let row_other = h(bt).max(h(bi));
    let fits = |s: f64| s * (col1 + col2) + 3.0 * gap <= area.width() && s * (row_front + row_other) + 3.0 * gap <= area.height();
    let mut candidates: Vec<Scale> = Scale::COMMON.to_vec();
    candidates.sort_by(|a, b| b.factor().total_cmp(&a.factor()));
    let scale = candidates
        .iter()
        .copied()
        .find(|s| fits(s.factor()))
        .unwrap_or(*candidates.last().unwrap_or(&Scale::new(1, 100)));
    let s = scale.factor();
    // The grid, centred in the area: columns (front | side), rows (front row | other row).
    let used_w = s * (col1 + col2) + gap;
    let used_h = s * (row_front + row_other) + gap;
    let x0 = area.min[0] + (area.width() - used_w) / 2.0;
    let y0 = area.min[1] + (area.height() - used_h) / 2.0;
    let cx1 = x0 + s * col1 / 2.0;
    let cx2 = x0 + s * col1 + gap + s * col2 / 2.0;
    let (front_row_y, other_row_y) = match projection {
        // Third angle: the front row at the bottom.
        Projection::Third => (y0 + s * row_front / 2.0, y0 + s * row_front + gap + s * row_other / 2.0),
        // First angle: the front row on top.
        Projection::First => (y0 + s * row_other + gap + s * row_front / 2.0, y0 + s * row_other / 2.0),
    };
    let place = |center: [f64; 2], b: ([f64; 2], [f64; 2])| {
        let cc = c(b);
        [center[0] - s * cc[0], center[1] - s * cc[1]]
    };
    let mut front = View::base(reference, NamedView::Front, scale, place([cx1, front_row_y], bf));
    front.hidden_lines = hidden_lines;
    front.tangent_edges = tangent;
    let child = |frame: Frame3, fold: Option<[f64; 2]>, center: [f64; 2], bb: ([f64; 2], [f64; 2])| {
        let mut v = front.clone();
        v.id = ViewId::new();
        v.kind = ViewKind::Projected;
        v.parent = Some(front.id);
        v.fold = fold;
        v.frame = frame;
        v.scale_inherited = true;
        v.name = projected_name(&frame);
        v.anchor = place(center, bb);
        v
    };
    let mut top = child(top_frame, Some(up), [cx1, other_row_y], bt);
    // Aligned: exactly on the front view's fold lines.
    top.anchor[0] = front.anchor[0];
    let mut side = child(side_frame, Some(side_dir), [cx2, front_row_y], bs);
    side.anchor[1] = front.anchor[1];
    let mut iso_view = child(iso, None, [cx2, other_row_y], bi);
    iso_view.shaded = true;
    iso_view.hidden_lines = false;
    vec![front, top, side, iso_view]
}

/// A line of a view on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineKind {
    /// Visible edges and outlines: continuous, medium.
    Visible,
    /// Hidden edges: dashed, thin (D4.10).
    Hidden,
    /// Visible tangent edges drawn solid (D4.11).
    Tangent,
    /// Visible tangent edges drawn phantom: long dash, short, short (D4.11).
    Phantom,
    /// Sketches shown over the view (D7.4).
    Sketch,
}

impl LineKind {
    pub fn weight(self) -> Weight {
        match self {
            LineKind::Visible => Weight::Medium,
            _ => Weight::Thin,
        }
    }

    /// The dash pattern on paper (mm: dash, gap, dash, gap…), `None` for continuous: hidden
    /// lines dashed (ISO 128 type 02 proportions, shortened so the short edges of small views
    /// still show a break), phantom lines a long dash and two short ones (type 05).
    pub fn pattern(self) -> Option<&'static [f64]> {
        match self {
            LineKind::Hidden => Some(&[2.0, 0.8]),
            LineKind::Phantom => Some(&[4.0, 0.8, 0.8, 0.8, 0.8, 0.8]),
            _ => None,
        }
    }
}

/// One projected edge on the sheet: its polyline (sheet mm) and how it is drawn, with the
/// index of the projection edge it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewLine {
    pub kind: LineKind,
    pub points: Vec<[f64; 2]>,
    pub edge: usize,
}

/// The lines of `view` from its projection `hlr`: visible edges solid; hidden edges dashed when
/// the view shows hidden lines, except where a drawn tangent edge lies on them (only the tangent
/// edge is drawn); visible tangent (smooth) edges hidden, solid or phantom by the view's setting
/// (hidden tangent edges are never drawn).
pub fn view_lines(view: &View, hlr: &Hlr) -> Vec<ViewLine> {
    let tangents: Vec<&cadrs_kernel::ProjEdge> = if view.tangent_edges == TangentEdges::Hidden {
        Vec::new()
    } else {
        hlr.edges
            .iter()
            .filter(|e| e.visibility == ProjVisibility::Visible && e.class == ProjClass::Smooth)
            .collect()
    };
    let clip = crate::view_kinds::view_clip(view);
    let mut out = Vec::new();
    for (i, e) in hlr.edges.iter().enumerate() {
        // A flat pattern's bend lines have their own pen (`crate::flat_view::bend_lines`).
        if crate::flat_view::is_bend_edge(e) {
            continue;
        }
        if e.visibility == ProjVisibility::Hidden && view.hidden_lines && covered(e, &tangents) {
            continue;
        }
        let kind = match (e.visibility, e.class) {
            (ProjVisibility::Visible, ProjClass::Smooth) => match view.tangent_edges {
                TangentEdges::Hidden => continue,
                TangentEdges::Solid => LineKind::Tangent,
                TangentEdges::Phantom => LineKind::Phantom,
            },
            (ProjVisibility::Visible, _) => LineKind::Visible,
            (ProjVisibility::Hidden, ProjClass::Smooth) => continue,
            (ProjVisibility::Hidden, _) if view.hidden_lines => LineKind::Hidden,
            (ProjVisibility::Hidden, _) => continue,
        };
        if clip.is_empty() {
            let points = e.points.iter().map(|p| view.to_sheet([p.x, p.y])).collect();
            out.push(ViewLine { kind, points, edge: i });
        } else {
            let pts: Vec<[f64; 2]> = e.points.iter().map(|p| [p.x, p.y]).collect();
            for piece in clip.apply(&pts) {
                let points = piece.iter().map(|p| view.to_sheet(*p)).collect();
                out.push(ViewLine { kind, points, edge: i });
            }
        }
    }
    merge_collinear(out)
}

/// Straight lines that lie on one line are drawn once: overlapping or touching pieces of one kind
/// merge (so a dashed line's pattern runs on without restarting where two hidden edges meet or
/// overlap), and a lower kind is trimmed where a higher one covers it: visible (and solid
/// tangent) over hidden over phantom. Curves are kept as they are.
fn merge_collinear(lines: Vec<ViewLine>) -> Vec<ViewLine> {
    const TOL: f64 = 1e-3; // sheet mm
    let rank = |k: LineKind| match k {
        LineKind::Visible => Some(0),
        LineKind::Tangent => Some(1),
        LineKind::Hidden => Some(2),
        LineKind::Phantom => Some(3),
        LineKind::Sketch => None,
    };
    // Groups of straight lines on one carrier: (point on it, unit direction), with each line's
    // interval along it.
    struct Group {
        o: [f64; 2],
        u: [f64; 2],
        members: Vec<(usize, f64, f64)>,
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut keep: Vec<Option<ViewLine>> = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        let straight = l.points.len() >= 2 && rank(l.kind).is_some() && {
            let (a, b) = (l.points[0], l.points[l.points.len() - 1]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let len = d[0].hypot(d[1]);
            len > TOL
                && l.points.iter().all(|p| ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]).abs() / len < TOL)
        };
        if !straight {
            keep.push(Some(l.clone()));
            continue;
        }
        keep.push(None);
        let (a, b) = (l.points[0], l.points[l.points.len() - 1]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let mut u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        if u[0] < -1e-12 || (u[0].abs() <= 1e-12 && u[1] < 0.0) {
            u = [-u[0], -u[1]];
        }
        let on = |g: &Group| {
            (g.u[0] * u[1] - g.u[1] * u[0]).abs() < 1e-6
                && ((a[0] - g.o[0]) * g.u[1] - (a[1] - g.o[1]) * g.u[0]).abs() < TOL
        };
        let gi = match groups.iter().position(on) {
            Some(g) => g,
            None => {
                groups.push(Group { o: a, u, members: Vec::new() });
                groups.len() - 1
            }
        };
        let g = &mut groups[gi];
        let t = |p: [f64; 2]| (p[0] - g.o[0]) * g.u[0] + (p[1] - g.o[1]) * g.u[1];
        let (t0, t1) = (t(a), t(b));
        g.members.push((i, t0.min(t1), t0.max(t1)));
    }
    let mut out: Vec<ViewLine> = keep.into_iter().flatten().collect();
    for g in groups {
        // Covered so far, by the higher kinds.
        let mut covered: Vec<(f64, f64)> = Vec::new();
        for r in 0..4 {
            let mut ivs: Vec<(f64, f64, usize)> = g
                .members
                .iter()
                .filter(|(i, _, _)| rank(lines[*i].kind) == Some(r))
                .map(|(i, a, b)| (*a, *b, *i))
                .collect();
            if ivs.is_empty() {
                continue;
            }
            ivs.sort_by(|a, b| a.0.total_cmp(&b.0));
            // Merge overlapping and touching intervals of this kind.
            let mut merged: Vec<(f64, f64, usize)> = Vec::new();
            for (a, b, i) in ivs {
                match merged.last_mut() {
                    Some(m) if a <= m.1 + TOL => m.1 = m.1.max(b),
                    _ => merged.push((a, b, i)),
                }
            }
            // Trim what the higher kinds cover.
            for (a, b, i) in &merged {
                let mut pieces = vec![(*a, *b)];
                for (ca, cb) in &covered {
                    pieces = pieces
                        .into_iter()
                        .flat_map(|(pa, pb)| {
                            let mut v = Vec::new();
                            if *ca > pa + TOL {
                                v.push((pa, pb.min(*ca)));
                            }
                            if *cb < pb - TOL {
                                v.push((pa.max(*cb), pb));
                            }
                            if *ca >= pb || *cb <= pa {
                                v = vec![(pa, pb)];
                            }
                            v
                        })
                        .filter(|(pa, pb)| pb - pa > TOL)
                        .collect();
                }
                // A merged run takes its first line's edge.
                let kind = lines[*i].kind;
                for (pa, pb) in pieces {
                    let p = |t: f64| [g.o[0] + g.u[0] * t, g.o[1] + g.u[1] * t];
                    out.push(ViewLine { kind, points: vec![p(pa), p(pb)], edge: lines[*i].edge });
                }
            }
            covered.extend(merged.iter().map(|(a, b, _)| (*a, *b)));
        }
    }
    out
}

/// Whether every point of `e` lies on one of `others` (within 1 µm).
fn covered(e: &cadrs_kernel::ProjEdge, others: &[&cadrs_kernel::ProjEdge]) -> bool {
    let seg = |p: &nalgebra::Point2<f64>, a: &nalgebra::Point2<f64>, b: &nalgebra::Point2<f64>| {
        let d = b - a;
        let l2 = d.norm_squared();
        if l2 < 1e-24 {
            return (p - a).norm();
        }
        let t = ((p - a).dot(&d) / l2).clamp(0.0, 1.0);
        (p - (a + d * t)).norm()
    };
    let on = |p: &nalgebra::Point2<f64>| {
        others.iter().any(|o| o.points.windows(2).any(|w| seg(p, &w[0], &w[1]) < 1e-3))
    };
    !others.is_empty() && e.points.len() >= 2 && e.points.iter().all(on) && on(&e.point_at(0.5))
}

/// The sheet-space bounding box of a view's projection, `(min, max)`.
pub fn view_bounds(view: &View, hlr: &Hlr) -> Option<([f64; 2], [f64; 2])> {
    let (lo, hi) = hlr.bounds()?;
    let corners = [[lo.x, lo.y], [hi.x, lo.y], [hi.x, hi.y], [lo.x, hi.y]].map(|p| view.to_sheet(p));
    let mut min = corners[0];
    let mut max = corners[0];
    for c in corners {
        min = [min[0].min(c[0]), min[1].min(c[1])];
        max = [max[0].max(c[0]), max[1].max(c[1])];
    }
    Some((min, max))
}

/// Splits a polyline into the dashes of `pattern` (paper mm; dash, gap, …), starting with a
/// dash. Each dash is a polyline.
pub fn dashes(points: &[[f64; 2]], pattern: &[f64]) -> Vec<Vec<[f64; 2]>> {
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    if points.len() < 2 || pattern.is_empty() || pattern.iter().any(|d| *d <= 0.0) {
        return vec![points.to_vec()];
    }
    let mut k = 0; // index into the pattern
    let mut left = pattern[0];
    let mut current: Vec<[f64; 2]> = vec![points[0]];
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let mut pos = 0.0;
        while len - pos > 1e-12 {
            let step = left.min(len - pos);
            pos += step;
            left -= step;
            let t = pos / len;
            let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            if k % 2 == 0 {
                current.push(p);
            }
            if left <= 1e-12 {
                if k % 2 == 0 {
                    out.push(std::mem::take(&mut current));
                } else {
                    current = vec![p];
                }
                k = (k + 1) % pattern.len();
                left = pattern[k];
            }
        }
    }
    if k % 2 == 0 && current.len() >= 2 {
        out.push(current);
    }
    out
}

/// The length of the straight projected line `curve`, if it is one.
pub fn line_length(curve: &ProjCurve) -> Option<f64> {
    match curve {
        ProjCurve::Line { start, end } => Some((end - start).norm()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close3(a: [f64; 3], b: [f64; 3]) -> bool {
        (v3(a) - v3(b)).norm() < 1e-9
    }

    #[test]
    fn named_frames_are_right_handed_and_match_the_planes() {
        for n in NamedView::ALL {
            let f = n.frame();
            let up = v3(f.up());
            assert!((up.norm() - 1.0).abs() < 1e-12);
            assert!(v3(f.dir).dot(&v3(f.x)).abs() < 1e-12);
        }
        assert!(close3(NamedView::Front.frame().up(), [0.0, 0.0, 1.0]));
        assert!(close3(NamedView::Top.frame().up(), [0.0, 1.0, 0.0]));
        assert!(close3(NamedView::Right.frame().up(), [0.0, 0.0, 1.0]));
    }

    #[test]
    fn third_angle_unfolds_naturally() {
        let front = NamedView::Front.frame();
        let right = fold_frame(&front, [1.0, 0.0], Projection::Third);
        assert_eq!(NamedView::of(&right), Some(NamedView::Right));
        let top = fold_frame(&front, [0.0, 1.0], Projection::Third);
        assert_eq!(NamedView::of(&top), Some(NamedView::Top));
        assert_eq!(NamedView::of(&fold_frame(&front, [-1.0, 0.0], Projection::Third)), Some(NamedView::Left));
        assert_eq!(NamedView::of(&fold_frame(&front, [0.0, -1.0], Projection::Third)), Some(NamedView::Bottom));
    }

    #[test]
    fn first_angle_places_views_opposite() {
        let front = NamedView::Front.frame();
        // The left view goes on the right, the top view below (P6.2).
        assert_eq!(NamedView::of(&fold_frame(&front, [1.0, 0.0], Projection::First)), Some(NamedView::Left));
        assert_eq!(NamedView::of(&fold_frame(&front, [0.0, -1.0], Projection::First)), Some(NamedView::Top));
        assert_eq!(NamedView::of(&fold_frame(&front, [-1.0, 0.0], Projection::First)), Some(NamedView::Right));
        assert_eq!(NamedView::of(&fold_frame(&front, [0.0, 1.0], Projection::First)), Some(NamedView::Bottom));
    }

    #[test]
    fn folds_keep_the_fold_line_coordinate() {
        // A point's coordinate along the fold line is the same in parent and child: that is
        // what makes aligned views line up.
        let front = NamedView::Front.frame();
        let p = Point3::new(3.0, -7.0, 11.0);
        for proj in Projection::ALL {
            for n in [[1.0, 0.0], [0.0, 1.0], [0.6, 0.8]] {
                let child = fold_frame(&front, n, proj);
                let e = [n[1], -n[0]];
                let pf = front.view_frame().to_2d(&p);
                let pc = child.view_frame().to_2d(&p);
                let along_parent = pf.x * e[0] + pf.y * e[1];
                let along_child = pc.x * e[0] + pc.y * e[1];
                assert!((along_parent - along_child).abs() < 1e-9, "{proj:?} {n:?}");
            }
        }
    }

    #[test]
    fn cursor_directions_pick_views() {
        assert_eq!(placement_for([50.0, 3.0]), Some(Placement::Ortho([1.0, 0.0])));
        assert_eq!(placement_for([-2.0, 40.0]), Some(Placement::Ortho([0.0, 1.0])));
        assert_eq!(placement_for([40.0, 40.0]), Some(Placement::Iso(1.0, 1.0)));
        assert_eq!(placement_for([-40.0, -35.0]), Some(Placement::Iso(-1.0, -1.0)));
        assert_eq!(placement_for([0.2, 0.2]), None);
        // The default iso from the front view is Onshape's Isometric.
        let iso = iso_frame(&NamedView::Front.frame(), 1.0, 1.0);
        assert_eq!(NamedView::of(&iso), Some(NamedView::Isometric));
    }

    #[test]
    fn projected_views_align_with_their_parent() {
        let parent = View::base(
            ObjectRef { element: Uuid::nil(), part: None },
            NamedView::Front,
            Scale::new(1, 2),
            [100.0, 60.0],
        );
        let right = projected_view(&parent, Placement::Ortho([1.0, 0.0]), Projection::Third, [180.0, 75.0], None);
        assert_eq!(right.anchor, [180.0, 60.0]);
        assert_eq!(right.name, "Right");
        assert_eq!(right.scale, Scale::new(1, 2));
        assert!(right.aligned());
        // The sheet transform: 1:2 halves lengths.
        let a = right.to_sheet([0.0, 0.0]);
        let b = right.to_sheet([10.0, 0.0]);
        assert!(((b[0] - a[0]) - 5.0).abs() < 1e-12);
        let back = right.from_sheet(b);
        assert!((back[0] - 10.0).abs() < 1e-12 && back[1].abs() < 1e-12);
    }

    #[test]
    fn four_views_fit_and_align() {
        // A 120 × 80 × 70 block (x × y × z) centred on the origin.
        let block = |f: &Frame3| {
            let vf = f.view_frame();
            let mut lo = [f64::MAX; 2];
            let mut hi = [f64::MIN; 2];
            for x in [-60.0, 60.0] {
                for y in [-40.0, 40.0] {
                    for z in [-35.0, 35.0] {
                        let q = vf.to_2d(&Point3::new(x, y, z));
                        lo = [lo[0].min(q.x), lo[1].min(q.y)];
                        hi = [hi[0].max(q.x), hi[1].max(q.y)];
                    }
                }
            }
            Some((lo, hi))
        };
        let area = crate::standard::Rect::new(12.7, 60.0, 266.7, 203.2);
        let r = ObjectRef { element: Uuid::nil(), part: None };
        let v = four_views(r, Projection::Third, area, 12.0, true, TangentEdges::Phantom, block);
        assert_eq!(v.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["Front", "Top", "Right", "Isometric"]);
        // 1:1 doesn't fit a 254 × 143 mm area (the grid is 200 + ~150 wide): 1:2 does.
        assert_eq!(v[0].scale, Scale::new(1, 2));
        // Third angle: Top above Front, Right right of it, both aligned.
        assert_eq!(v[1].anchor[0], v[0].anchor[0]);
        assert!(v[1].anchor[1] > v[0].anchor[1]);
        assert_eq!(v[2].anchor[1], v[0].anchor[1]);
        assert!(v[2].anchor[0] > v[0].anchor[0]);
        assert!(v[3].shaded && v[3].fold.is_none());
        assert!(v[..3].iter().all(|v| v.hidden_lines && v.tangent_edges == TangentEdges::Phantom));
        let first = four_views(r, Projection::First, area, 12.0, false, TangentEdges::Solid, block);
        assert_eq!(first[1].name, "Top");
        assert!(first[1].anchor[1] < first[0].anchor[1], "first angle: Top below Front");
        assert_eq!(first[2].name, "Left");
    }

    #[test]
    fn collinear_lines_merge_by_kind_and_precedence() {
        let l = |kind, a: [f64; 2], b: [f64; 2], edge| ViewLine { kind, points: vec![a, b], edge };
        let out = merge_collinear(vec![
            // Two overlapping hidden pieces of one floor line: one dashed line 0–30.
            l(LineKind::Hidden, [0.0, 5.0], [20.0, 5.0], 0),
            l(LineKind::Hidden, [30.0, 5.0], [10.0, 5.0], 1),
            // A visible edge on the same line covers 12–18 of it.
            l(LineKind::Visible, [12.0, 5.0], [18.0, 5.0], 2),
            // A phantom line under the hidden one is dropped; its part beyond stays.
            l(LineKind::Phantom, [25.0, 5.0], [40.0, 5.0], 3),
            // Another line entirely: untouched.
            l(LineKind::Hidden, [0.0, 7.0], [5.0, 7.0], 4),
        ]);
        let of = |k: LineKind| {
            let mut v: Vec<(f64, f64, f64)> = out
                .iter()
                .filter(|x| x.kind == k)
                .map(|x| (x.points[0][0].min(x.points[1][0]), x.points[0][0].max(x.points[1][0]), x.points[0][1]))
                .collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v
        };
        assert_eq!(of(LineKind::Visible), vec![(12.0, 18.0, 5.0)]);
        assert_eq!(of(LineKind::Hidden), vec![(0.0, 5.0, 7.0), (0.0, 12.0, 5.0), (18.0, 30.0, 5.0)]);
        assert_eq!(of(LineKind::Phantom), vec![(30.0, 40.0, 5.0)]);
    }

    #[test]
    fn dashes_follow_the_pattern() {
        let d = dashes(&[[0.0, 0.0], [10.0, 0.0]], &[3.0, 1.0]);
        // 3 on, 1 off: dashes at 0–3, 4–7, 8–10.
        assert_eq!(d.len(), 3);
        assert_eq!(d[0], vec![[0.0, 0.0], [3.0, 0.0]]);
        assert_eq!(d[1], vec![[4.0, 0.0], [7.0, 0.0]]);
        assert_eq!(d[2], vec![[8.0, 0.0], [10.0, 0.0]]);
        // Around a corner, a dash bends with the polyline.
        let c = dashes(&[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0]], &[3.0, 1.0]);
        assert_eq!(c[0], vec![[0.0, 0.0], [2.0, 0.0], [2.0, 1.0]]);
    }
}
