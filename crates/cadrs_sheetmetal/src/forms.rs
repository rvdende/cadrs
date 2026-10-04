//! **Forms** on sheet metal (P3I.9, SM20; `reference/onshape/sheetmetal/raw/help-sheet_metal_form.txt`):
//! where a placed form may go and how the flat pattern shows it.
//!
//! - **Placement rules** (SM20.3): "Forms cannot touch or cut into the boundaries of side walls,
//!   rolled walls, rips, joints, or corners of the sheet metal." [`check_footprint`] takes a
//!   form's footprint (its tool parts seen along the wall's normal) in the wall's own 2D and
//!   fails when the wall is rolled, when the footprint leaves the wall, or when it touches a bend
//!   (its tangent line), a rip or another joint, or a free edge of the wall (where a side wall's
//!   face is).
//! - **In the flat** (SM20.3, SM15, SM16): each form is a [`FlatForm`] of its flat-pattern part —
//!   its outline (the form's construction-only sketch from its Tag, or else its footprint) and
//!   its centre (the centermark), mapped onto the flat with its wall. Flat DXF export and flat
//!   drawing views read them from there ("Include form feature outlines / centermarks").

use serde::{Deserialize, Serialize};

use crate::flat::FlatPart;
use crate::model::{JointKind, Model, Surface, WallId};
use crate::poly::{self, P2, Polygon, Seg2};

/// A polyline of a form's outline (closed loops repeat nothing: `closed` says so).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FormLine {
    pub points: Vec<P2>,
    pub closed: bool,
}

/// A form as the flat pattern shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlatForm {
    /// A stable key of the Form feature, and its name ("Form 1").
    pub source: u64,
    pub name: String,
    /// The form's name in its library or document ("Louver").
    pub form: String,
    pub wall: WallId,
    /// The centermark.
    pub center: P2,
    /// The outline, in flat-pattern coordinates.
    pub lines: Vec<FormLine>,
    /// The form stands up towards the viewer of the flat pattern.
    pub up: bool,
}

/// What a form may not touch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FormProblem {
    /// The target face is a rolled wall.
    RolledWall,
    /// The target isn't a wall of this model.
    NoWall,
    /// The form runs off the wall.
    OffWall,
    /// It touches a bend, rip or joint (its table name), or the wall's free edge (`None`).
    Touches(Option<String>),
}

impl FormProblem {
    pub fn message(&self) -> String {
        match self {
            FormProblem::RolledWall => "Forms can't be placed on rolled walls".into(),
            FormProblem::NoWall => "The target face isn't a face of a sheet metal wall".into(),
            FormProblem::OffWall => "The form runs off its wall: forms can't touch side walls, rips, joints or corners".into(),
            FormProblem::Touches(Some(name)) => format!("The form touches {name}: forms can't touch side walls, rips, joints or corners"),
            FormProblem::Touches(None) => "The form touches the edge of its wall: forms can't touch side walls, rips, joints or corners".into(),
        }
    }
}

impl std::fmt::Display for FormProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for FormProblem {}

/// The smallest distance between a segment and a polygon's boundary (0 when it crosses it or lies
/// inside).
fn seg_distance(s: Seg2, p: &Polygon) -> f64 {
    if !poly::clip_segment(s, std::slice::from_ref(p)).is_empty() {
        return 0.0;
    }
    let pt_seg = |q: P2, a: P2, b: P2| {
        let d = b - a;
        let t = ((q - a).dot(&d) / d.norm_squared().max(1e-300)).clamp(0.0, 1.0);
        (a + d * t - q).norm()
    };
    let l = &p.outer;
    let mut best = f64::INFINITY;
    for i in 0..l.len() {
        let (a, b) = (l[i], l[(i + 1) % l.len()]);
        best = best.min(pt_seg(a, s.a, s.b)).min(pt_seg(s.a, a, b)).min(pt_seg(s.b, a, b));
    }
    best
}

/// Checks a form's footprint (in the wall's local 2D) against the rules (see the module docs).
/// `clearance` is how close it may come (a hair: touching is what's refused).
pub fn check_footprint(m: &Model, wall: WallId, footprint: &Polygon, clearance: f64) -> Result<(), FormProblem> {
    let w = m.wall(wall).ok_or(FormProblem::NoWall)?;
    if matches!(w.surface, Surface::Rolled { .. }) {
        return Err(FormProblem::RolledWall);
    }
    if footprint.is_empty() {
        return Ok(());
    }
    // The joints first: they name what is touched.
    for j in m.joints.iter().filter(|j| j.a == wall || j.b == wall) {
        let Some(s) = j.segment_on(wall) else { continue };
        if seg_distance(s, footprint) <= clearance {
            let what = match j.kind {
                JointKind::Bend(_) | JointKind::Rip { .. } | JointKind::Tangent { .. } => j.name.clone(),
            };
            return Err(FormProblem::Touches(Some(what)));
        }
    }
    let inside = poly::intersection(footprint, &w.outline).iter().map(Polygon::area).sum::<f64>();
    // (Booleans snap to their grid: allow that much along the footprint's edge.)
    if inside < footprint.area() - 4.0 * poly::GRID * poly::perimeter(footprint) - 1e-12 {
        return Err(FormProblem::OffWall);
    }
    // The free edges.
    let l = &w.outline.outer;
    for i in 0..l.len() {
        if seg_distance(Seg2::new(l[i], l[(i + 1) % l.len()]), footprint) <= clearance {
            return Err(FormProblem::Touches(None));
        }
    }
    Ok(())
}

/// A form placed on `wall`, its outline and centre given in the wall's local 2D, as the flat
/// pattern part shows it (`None` if the wall isn't in that part).
pub fn on_flat(part: &FlatPart, m: &Model, wall: WallId, center: P2, lines: &[FormLine]) -> Option<(P2, Vec<FormLine>)> {
    let place = part.placement(wall)?;
    let w = m.wall(wall)?;
    let map = |q: P2| place.apply(w.flat_local(&m.params, q));
    Some((map(center), lines.iter().map(|l| FormLine { points: l.points.iter().map(|q| map(*q)).collect(), closed: l.closed }).collect()))
}

/// The convex hull of points (counter-clockwise), for footprints.
pub fn hull(points: &[P2]) -> Polygon {
    let mut p: Vec<P2> = points.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| (*a - *b).norm() < 1e-12);
    if p.len() < 3 {
        return Polygon::default();
    }
    let cross = |o: P2, a: P2, b: P2| (a - o).perp(&(b - o));
    let mut lower: Vec<P2> = Vec::new();
    for q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *q) <= 0.0 {
            lower.pop();
        }
        lower.push(*q);
    }
    let mut upper: Vec<P2> = Vec::new();
    for q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *q) <= 0.0 {
            upper.pop();
        }
        upper.push(*q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    Polygon::new(lower)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flat::flatten;
    use crate::params::Params;
    use crate::poly::Polygon;
    use crate::samples;

    fn square(c: P2, h: f64) -> Polygon {
        Polygon::rect(P2::new(c.x - h, c.y - h), P2::new(c.x + h, c.y + h))
    }

    #[test]
    fn a_form_in_the_middle_of_a_wall_is_fine_and_one_on_a_bend_is_not() {
        let m = samples::l_bracket(Params { thickness: 2.0, bend_radius: 3.0, ..Default::default() }, true).unwrap();
        let w = m.walls[0].id;
        let (lo, hi) = m.walls[0].outline.bounds().unwrap();
        let mid = P2::from((lo.coords + hi.coords) / 2.0);
        assert_eq!(check_footprint(&m, w, &square(mid, 2.0), 1e-6), Ok(()));
        // Over the bend's tangent line.
        let bend = m.joints.iter().find(|j| j.bend().is_some()).unwrap();
        let s = bend.segment_on(w).unwrap();
        let on = P2::from((s.a.coords + s.b.coords) / 2.0);
        assert!(matches!(check_footprint(&m, w, &square(on, 2.0), 1e-6), Err(FormProblem::Touches(Some(_)))));
        // Off the wall altogether.
        assert!(check_footprint(&m, w, &square(P2::new(hi.x + 50.0, hi.y + 50.0), 2.0), 1e-6).is_err());
    }

    #[test]
    fn forms_map_onto_the_flat_with_their_wall() {
        let m = samples::l_bracket(Params::default(), true).unwrap();
        let flat = flatten(&m);
        let part = &flat.parts[0];
        let w = m.walls[1].id;
        let (c, lines) = on_flat(part, &m, w, P2::new(1.0, 2.0), &[FormLine { points: vec![P2::new(0.0, 0.0), P2::new(3.0, 0.0)], closed: false }]).unwrap();
        let place = part.placement(w).unwrap();
        assert!((c - place.apply(P2::new(1.0, 2.0))).norm() < 1e-12);
        assert!(((lines[0].points[1] - lines[0].points[0]).norm() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn hull_of_a_square_and_its_middle() {
        let h = hull(&[P2::new(0.0, 0.0), P2::new(2.0, 0.0), P2::new(1.0, 1.0), P2::new(2.0, 2.0), P2::new(0.0, 2.0)]);
        assert_eq!(h.outer.len(), 4);
        assert!((h.area() - 4.0).abs() < 1e-12);
    }
}
