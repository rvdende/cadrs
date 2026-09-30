//! Notes (P3C.4, D9, X3, X9): rich text on the sheet, free or with leaders to view geometry.
//!
//! A [`Note`] belongs to its sheet. Its text box is placed by its top-left corner (`at`, sheet
//! mm), turned by `rotation` about that corner, with a default text height and, optionally, a
//! wrap width (the ruler's handles, D9.3). Leaders ([`Leader`], D9.1, D9.2) attach to model
//! topology through a view, like the other annotations: to a snap point of an edge
//! ([`crate::annotation::PointRef`]) or to a point along an edge (the edge's persistent name
//! and where on it, projected onto the edge's current shape), so they follow the model.
//!
//! [`note_graphics`] lays a note out on the sheet: its text (fields resolved now, D9.4), the
//! leaders (a short landing off the nearer side of the box, then a line to a filled arrowhead)
//! and, for a selected note, its grips (D9.5): the rotation handle above the box, the side
//! handles that set the wrap width and the corner handle that scales the text.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::annotation::{EdgeRef, PointRef, ViewModel, resolve, resolve_point};
use crate::rich::{self, FieldContext, P2, RichLayout, RichText, Stop};
use crate::view::{View, ViewId, rotate};

/// Identifies a note on its sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NoteId(pub Uuid);

impl NoteId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for NoteId {
    fn default() -> Self {
        Self::new()
    }
}

/// Where a leader points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LeaderEnd {
    /// An end, midpoint or centre of an edge.
    Point(PointRef),
    /// A point along an edge (view 2D, model mm): the edge's point nearest it.
    Edge { edge: EdgeRef, at: P2 },
}

/// A leader from a note to geometry in a view.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Leader {
    pub view: ViewId,
    pub end: LeaderEnd,
}

/// A note on a sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    /// The text box's top-left corner (sheet mm).
    pub at: P2,
    /// Degrees counter-clockwise about `at`.
    #[serde(default)]
    pub rotation: f64,
    /// Default cap height (sheet mm).
    pub height: f64,
    /// Wrap width (sheet mm); `None` for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    pub text: RichText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leaders: Vec<Leader>,
}

impl Note {
    pub fn new(at: P2, height: f64) -> Self {
        Self {
            id: NoteId::new(),
            at,
            rotation: 0.0,
            height,
            width: None,
            text: RichText::default(),
            leaders: Vec::new(),
        }
    }

    /// A box point (x right, y up from the top-left corner) on the sheet.
    pub fn to_sheet(&self, p: P2) -> P2 {
        let q = rotate(p, self.rotation.to_radians());
        [self.at[0] + q[0], self.at[1] + q[1]]
    }

    /// A sheet point in the box's frame.
    pub fn from_sheet(&self, s: P2) -> P2 {
        rotate([s[0] - self.at[0], s[1] - self.at[1]], -self.rotation.to_radians())
    }
}

/// A grip of a selected note (D9.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteGrip {
    /// Turns the note about its box's centre.
    Rotate,
    /// Sets the wrap width from the left side (the box's right side stays).
    Left,
    /// Sets the wrap width from the right side.
    Right,
    /// Scales the text (bottom-right corner).
    Scale,
    /// Moves leader `i`'s end onto other geometry.
    Leader(usize),
}

/// A text of a note on the sheet: a piece of its layout, turned with the note.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteText {
    pub piece: rich::Piece,
    /// Sheet position of the piece's anchor (left end, middle of the capitals).
    pub pos: P2,
    /// Degrees counter-clockwise.
    pub rotation: f64,
}

/// A note on the sheet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NoteGraphics {
    pub texts: Vec<NoteText>,
    pub strokes: Vec<Vec<P2>>,
    /// Filled triangles (arrowheads).
    pub fills: Vec<[P2; 3]>,
    /// The box's corners (top-left, top-right, bottom-right, bottom-left), with a margin.
    pub frame: [P2; 4],
    pub grips: Vec<(P2, NoteGrip)>,
    /// Caret stops on the sheet: position of the baseline, and the top of the capitals.
    pub stops: Vec<(P2, P2)>,
    /// Field boxes on the sheet (shaded while editing).
    pub fields: Vec<[P2; 4]>,
    /// The layout (box frame).
    pub layout: RichLayout,
    /// Strokes and fills of leaders whose geometry is gone (P3C.6): drawn red, where they last
    /// pointed. Indices into `strokes` and `fills`.
    pub dangling_strokes: Vec<usize>,
    pub dangling_fills: Vec<usize>,
}

impl NoteGraphics {
    /// Distance from a sheet point to the note: 0 inside its box.
    pub fn distance(&self, p: P2) -> f64 {
        if inside(&self.frame, p) {
            return 0.0;
        }
        let mut d = f64::MAX;
        for s in self.strokes.iter().chain(std::iter::once(&self.frame.to_vec())) {
            for w in s.windows(2) {
                d = d.min(seg_distance(p, w[0], w[1]));
            }
        }
        d
    }

}

fn seg_distance(p: P2, a: P2, b: P2) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 < 1e-300 { 0.0 } else { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0) };
    (p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t)
}

fn inside(q: &[P2; 4], p: P2) -> bool {
    let mut sign = 0.0;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        if c.abs() < 1e-12 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}

/// The views a note's leaders read: (view, its model) by id.
pub type ViewLookup<'a> = dyn Fn(ViewId) -> Option<(&'a View, &'a dyn ViewModel)> + 'a;

/// Whether leader `l`'s end no longer resolves in its view's model `m` (P3C.6).
pub fn leader_dangles(v: &View, m: &dyn ViewModel, l: &Leader) -> bool {
    let r = match &l.end {
        LeaderEnd::Point(p) => &p.edge,
        LeaderEnd::Edge { edge, .. } => edge,
    };
    crate::annotation::ref_dangles(v, m, r)
}

/// Where a leader ends now (sheet mm), or `None` when its view is gone. A dangling leader keeps
/// pointing where it last did.
pub fn leader_end(l: &Leader, views: &ViewLookup) -> Option<P2> {
    let (v, m) = views(l.view)?;
    let frozen = crate::annotation::ModelData::default();
    let m: &dyn ViewModel = if leader_dangles(v, m, l) { &frozen } else { m };
    let local = match &l.end {
        LeaderEnd::Point(p) => resolve_point(v, m, p)?.0,
        LeaderEnd::Edge { edge, at } => resolve(v, m, edge).shape.nearest(*at),
    };
    Some(v.to_sheet(local))
}

/// A leader arrowhead's length, in dimension arrow lengths.
pub const LEADER_ARROW: f64 = 1.3;

/// The margin around a note's text, in cap heights.
const MARGIN: f64 = 0.45;

/// A note on the sheet (see the module docs). `arrow` is the leader arrowhead's length.
pub fn note_graphics(note: &Note, ctx: &FieldContext, views: &ViewLookup, arrow: f64, grips: bool) -> NoteGraphics {
    let h = note.height;
    let l = rich::layout(&note.text, ctx, h, note.width);
    let mut g = NoteGraphics::default();
    let s = |p: P2| note.to_sheet(p);
    for p in &l.pieces {
        g.texts.push(NoteText { piece: p.clone(), pos: s(p.pos), rotation: note.rotation });
    }
    for st in &l.strokes {
        g.strokes.push(st.iter().map(|p| s(*p)).collect());
    }
    let m = MARGIN * h;
    let (w, bh) = (l.width.max(h), l.height.max(h));
    let lo_y = -bh - 0.35 * h; // room for descenders
    g.frame = [s([-m, m]), s([w + m, m]), s([w + m, lo_y - m]), s([-m, lo_y - m])];
    g.stops = l
        .stops
        .iter()
        .map(|Stop { x, baseline, height }| (s([*x, *baseline - 0.25 * height]), s([*x, *baseline + 1.15 * height])))
        .collect();
    g.fields = l.fields.iter().map(|(lo, hi)| [s(*lo), s([hi[0], lo[1]]), s(*hi), s([lo[0], hi[1]])]).collect();
    // Leaders: from the side of the box nearer the target, with a short landing.
    let mid_y = (lo_y) / 2.0;
    for (i, ld) in note.leaders.iter().enumerate() {
        let Some(tip) = leader_end(ld, views) else {
            continue;
        };
        if views(ld.view).is_some_and(|(v, m)| leader_dangles(v, m, ld)) {
            g.dangling_strokes.push(g.strokes.len());
            g.dangling_fills.push(g.fills.len());
        }
        let local = note.from_sheet(tip);
        let right = local[0] > w / 2.0;
        let side = if right { [w + m, mid_y] } else { [-m, mid_y] };
        let landing = if right { [w + m + h, mid_y] } else { [-m - h, mid_y] };
        let (a, b) = (s(side), s(landing));
        g.strokes.push(vec![a, b, tip]);
        let d = [tip[0] - b[0], tip[1] - b[1]];
        let dl = d[0].hypot(d[1]);
        if dl > 1e-9 {
            // Leader arrowheads a little larger than dimension arrows (P3C.4's delta: they read
            // small next to Onshape's).
            let arrow = arrow * LEADER_ARROW;
            let u = [d[0] / dl, d[1] / dl];
            let base = [tip[0] - u[0] * arrow, tip[1] - u[1] * arrow];
            let n = [-u[1] * arrow * 0.2, u[0] * arrow * 0.2];
            g.fills.push([tip, [base[0] + n[0], base[1] + n[1]], [base[0] - n[0], base[1] - n[1]]]);
        }
        if grips {
            g.grips.push((tip, NoteGrip::Leader(i)));
        }
    }
    if grips {
        let top_mid = [w / 2.0, m];
        g.grips.push((s([top_mid[0], m + 2.2 * h]), NoteGrip::Rotate));
        g.strokes.push(vec![s(top_mid), s([top_mid[0], m + 1.6 * h])]);
        g.grips.push((s([-m, mid_y]), NoteGrip::Left));
        g.grips.push((s([w + m, mid_y]), NoteGrip::Right));
        g.grips.push((s([w + m, lo_y - m]), NoteGrip::Scale));
    }
    g.layout = l;
    g
}

/// A note after grip `grip` was dragged from `from` to `to` (sheet mm).
pub fn drag_grip(note: &Note, ctx: &FieldContext, grip: NoteGrip, from: P2, to: P2) -> Note {
    let mut n = note.clone();
    let h = note.height;
    let l = rich::layout(&note.text, ctx, h, note.width);
    let m = MARGIN * h;
    let w = l.width.max(h);
    match grip {
        NoteGrip::Rotate => {
            // About the box's centre, snapped to 5°.
            let bh = l.height.max(h) + 0.35 * h;
            let c = note.to_sheet([w / 2.0, -bh / 2.0]);
            let a0 = (from[1] - c[1]).atan2(from[0] - c[0]);
            let a1 = (to[1] - c[1]).atan2(to[0] - c[0]);
            let mut r = note.rotation + (a1 - a0).to_degrees();
            r = (r / 5.0).round() * 5.0;
            r = r.rem_euclid(360.0);
            if r > 180.0 {
                r -= 360.0;
            }
            n.rotation = r;
            // Keep the centre where it was.
            let c_new = rotate([w / 2.0, -bh / 2.0], r.to_radians());
            n.at = [c[0] - c_new[0], c[1] - c_new[1]];
        }
        NoteGrip::Right => {
            let p = note.from_sheet(to);
            n.width = Some((p[0] - m).max(2.0 * h));
        }
        NoteGrip::Left => {
            let p = note.from_sheet(to);
            let right = w;
            let new_w = (right - (p[0] + m)).max(2.0 * h);
            let shift = right - new_w;
            n.width = Some(new_w);
            n.at = note.to_sheet([shift, 0.0]);
        }
        NoteGrip::Scale => {
            let p0 = note.from_sheet(from);
            let p1 = note.from_sheet(to);
            let d0 = p0[0].hypot(p0[1]).max(1e-9);
            let d1 = p1[0].hypot(p1[1]);
            let k = (d1 / d0).clamp(0.2, 5.0);
            let q = |v: f64| (v * k * 100.0).round() / 100.0;
            n.height = q(note.height).max(0.5);
            n.width = note.width.map(q);
            n.text.restyle(|s| s.height = s.height.map(q), |_| {});
        }
        NoteGrip::Leader(_) => {}
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::DrawingContext;
    use crate::title_block::ReferenceProps;

    fn no_views(_: ViewId) -> Option<(&'static View, &'static dyn ViewModel)> {
        None
    }

    #[test]
    fn grips_resize_rotate_and_scale() {
        let r = ReferenceProps::default();
        let d = DrawingContext::default();
        let ctx = FieldContext { reference: &r, drawing: &d };
        let mut n = Note::new([50.0, 100.0], 3.0);
        n.text = RichText::plain("GENERAL NOTES APPLY TO ALL SHEETS");
        let g = note_graphics(&n, &ctx, &no_views, 3.0, true);
        let at = |k: NoteGrip| g.grips.iter().find(|(_, x)| *x == k).unwrap().0;
        // The right handle sets a wrap width: the text wraps to more lines.
        let right = at(NoteGrip::Right);
        let narrow = drag_grip(&n, &ctx, NoteGrip::Right, right, [right[0] - 40.0, right[1]]);
        assert!(narrow.width.is_some());
        assert_eq!(narrow.at, n.at);
        let gn = note_graphics(&narrow, &ctx, &no_views, 3.0, false);
        assert!(gn.frame[2][1] < g.frame[2][1], "taller when wrapped");
        // The left handle keeps the right side.
        let left = at(NoteGrip::Left);
        let from_left = drag_grip(&n, &ctx, NoteGrip::Left, left, [left[0] + 20.0, left[1]]);
        let gl = note_graphics(&from_left, &ctx, &no_views, 3.0, false);
        assert!((from_left.at[0] - n.at[0] - 20.0).abs() < 1e-9);
        assert!(gl.layout.width < g.layout.width);
        // Rotation about the centre, in 5° steps.
        let rot = at(NoteGrip::Rotate);
        let c = [(g.frame[0][0] + g.frame[2][0]) / 2.0, (g.frame[0][1] + g.frame[2][1]) / 2.0];
        let to = [c[0] - (rot[1] - c[1]), c[1] + 0.001];
        let turned = drag_grip(&n, &ctx, NoteGrip::Rotate, rot, to);
        assert_eq!(turned.rotation, 90.0);
        // Scaling doubles the text height.
        let sc = drag_grip(&n, &ctx, NoteGrip::Scale, [60.0, 90.0], [70.0, 80.0]);
        assert_eq!(sc.height, 6.0);
        // Round trip.
        let back: Note = ron::from_str(&ron::to_string(&turned).unwrap()).unwrap();
        assert_eq!(back, turned);
    }
}
