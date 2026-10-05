//! Modelling in the flat (SM14; `reference/onshape/sheetmetal/raw/14-modeling-in-the-flat-view.txt`):
//! a sketch on the flat pattern, extruded **Add** or **Remove**, adds material to the sheet or
//! cuts it away *in the flat*, so a cut keeps its exact flat size even where it wraps across
//! bends (the lesson's 0.5 × 6.0 in slot), and tabs drawn in the flat appear folded.
//!
//! - **Remove** keeps the region as a [`FlatCut`] in the model, in its part's **anchor** wall's
//!   own flat coordinates (the part's first wall: the one the flat is laid out from), so it
//!   moves with the flat when the model changes upstream. [`crate::flatten`] applies each one
//!   like a relief cut to every piece of its part ([`part_cuts`]): the flat outline loses the
//!   region, the bend lines stop at it, and every wall and bend region learns what it lost in its
//!   own coordinates (`(s, u)` across a bend region) for the folded solid.
//! - **Add** grows walls: each new piece of material (the region less what is there already)
//!   goes to the wall whose edge it shares most, mapped back through that wall's placement into
//!   its outline ([`add`]).

use serde::{Deserialize, Serialize};

use crate::flat::{FlatPart, FlatPattern, PieceSource, ReliefCut, ReliefSource};
use crate::model::{Model, WallId};
use crate::poly::{self, GRID, P2, Polygon};

/// Material removed in the flat (a flat-pattern sketch extruded Remove).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlatCut {
    /// The wall the shapes are relative to: its own flat 2D ([`crate::model::Wall::flat_local`]).
    pub anchor: WallId,
    pub shapes: Vec<Polygon>,
}

/// Why a flat edit can't be made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlatEditError {
    /// The flat pattern has no such part.
    NoPart,
    /// Remove: the region misses the part's material.
    MissesMaterial,
    /// Add: a piece of the region touches no wall of the part.
    Detached,
    /// Add: the region is all material already.
    NothingNew,
    /// Add: the new material would split a wall in two.
    SplitsWall,
}

impl FlatEditError {
    pub fn message(&self) -> &'static str {
        match self {
            FlatEditError::NoPart => "The flat pattern part no longer exists",
            FlatEditError::MissesMaterial => "The region to remove doesn't touch the flat pattern",
            FlatEditError::Detached => "Material added in the flat must touch the flat pattern",
            FlatEditError::NothingNew => "The region to add is all material already",
            FlatEditError::SplitsWall => "The added material can't be joined to its wall",
        }
    }
}

impl std::fmt::Display for FlatEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for FlatEditError {}

/// The part's anchor: the wall its flat is laid out from (its flat coordinates are this wall's).
pub fn anchor(part: &FlatPart) -> Option<WallId> {
    part.walls.first().copied()
}

/// The model's flat cuts that belong to `part`, as relief cuts on all of its pieces (their
/// `removed` is filled in by the flat solver like any relief's).
pub(crate) fn part_cuts(m: &Model, part: &FlatPart) -> Vec<ReliefCut> {
    let targets: Vec<PieceSource> = part.pieces.iter().map(|p| p.source).collect();
    m.flat_cuts
        .iter()
        .enumerate()
        .filter_map(|(k, c)| {
            let pm = part.placement(c.anchor)?;
            Some(ReliefCut {
                source: ReliefSource::Flat { index: k },
                shapes: c.shapes.iter().map(|s| s.map(|q| pm.apply(q))).collect(),
                slit: None,
                targets: targets.clone(),
                removed: Vec::new(),
            })
        })
        .collect()
}

/// Remove: cuts `region` (in the flat coordinates of `flat.parts[part]`) out of the sheet.
pub fn remove(m: &mut Model, flat: &FlatPattern, part: usize, region: &[Polygon]) -> Result<(), FlatEditError> {
    let p = flat.parts.get(part).ok_or(FlatEditError::NoPart)?;
    let a = anchor(p).ok_or(FlatEditError::NoPart)?;
    let inv = p.placement(a).and_then(|m| m.inverse()).ok_or(FlatEditError::NoPart)?;
    let touched: f64 = region.iter().flat_map(|r| p.outline.iter().map(move |o| poly::overlap_area(r, o))).sum();
    if touched <= 1e-12 {
        return Err(FlatEditError::MissesMaterial);
    }
    m.flat_cuts.push(FlatCut {
        anchor: a,
        shapes: region.iter().filter(|r| !r.is_empty()).map(|r| r.map(|q| inv.apply(q))).collect(),
    });
    Ok(())
}

/// The distance from `q` to the nearest edge of `p`'s loops.
fn boundary_distance(p: &Polygon, q: P2) -> f64 {
    std::iter::once(&p.outer)
        .chain(p.holes.iter())
        .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
        .map(|(a, b)| {
            let d = b - a;
            let t = if d.norm_squared() < 1e-30 { 0.0 } else { ((q - a).dot(&d) / d.norm_squared()).clamp(0.0, 1.0) };
            (a + d * t - q).norm()
        })
        .fold(f64::INFINITY, f64::min)
}

/// How much of `piece`'s boundary runs along `wall`'s (sampled).
fn contact(piece: &Polygon, wall: &Polygon, tol: f64) -> usize {
    let l = &piece.outer;
    let mut n = 0;
    for i in 0..l.len() {
        let (a, b) = (l[i], l[(i + 1) % l.len()]);
        for k in 0..8 {
            let q = a + (b - a) * ((k as f64 + 0.5) / 8.0);
            if boundary_distance(wall, q) <= tol {
                n += 1;
            }
        }
    }
    n
}

/// Add: grows the part's walls by `region` (in the flat coordinates of `flat.parts[part]`).
/// Each piece of new material joins the wall whose edge it runs along most.
pub fn add(m: &mut Model, flat: &FlatPattern, part: usize, region: &[Polygon]) -> Result<(), FlatEditError> {
    let p = flat.parts.get(part).ok_or(FlatEditError::NoPart)?;
    let size = p.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0).max(1.0);
    let tol = (1e-7 * size).max(10.0 * GRID);
    let existing: Vec<Polygon> = p.pieces.iter().map(|pc| pc.polygon.clone()).collect();
    let new = poly::difference(&poly::union(region), &existing);
    let new: Vec<Polygon> = new.into_iter().filter(|n| n.area() > 1e-9).collect();
    if new.is_empty() {
        return Err(FlatEditError::NothingNew);
    }
    for piece in &new {
        // The wall it joins.
        let best = p
            .pieces
            .iter()
            .filter_map(|pc| match pc.source {
                PieceSource::Wall(w) => Some((w, contact(piece, &pc.polygon, tol))),
                PieceSource::Bend(_) => None,
            })
            .max_by_key(|(_, c)| *c)
            .filter(|(_, c)| *c > 0)
            .ok_or(FlatEditError::Detached)?
            .0;
        let inv = p.placement(best).and_then(|a| a.inverse()).ok_or(FlatEditError::NoPart)?;
        let params = m.params;
        let wall = m.walls.iter_mut().find(|w| w.id == best).ok_or(FlatEditError::NoPart)?;
        let k = wall.flat_scale(&params);
        let local = piece.map(|q| {
            let l = inv.apply(q);
            P2::new(l.x / k, l.y)
        });
        let joined = poly::union(&[wall.outline.clone(), local.clone()]);
        let [one] = joined.as_slice() else {
            return Err(FlatEditError::SplitsWall);
        };
        // The booleans round to the grid: back onto the exact corners.
        let exact: Vec<P2> = wall.outline.outer.iter().chain(wall.outline.holes.iter().flatten()).chain(local.outer.iter()).copied().collect();
        wall.outline = poly::snap_to(one, &exact, 10.0 * GRID);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Params, flatten, samples};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Polygon {
        Polygon::rect(P2::new(x0, y0), P2::new(x1, y1))
    }

    #[test]
    fn a_slot_across_a_bend_keeps_its_flat_size() {
        let p = Params::default();
        let mut m = samples::l_bracket(p, true).unwrap();
        let flat = flatten(&m);
        assert!(flat.is_ok(), "{:?}", flat.errors);
        let before = flat.parts[0].area();
        let bend = &flat.parts[0].bends[0];
        // A 4 × 30 slot centred on the bend, across it.
        let c = P2::from((bend.center.a.coords + bend.center.b.coords) / 2.0);
        let d = bend.center.dir();
        let n = poly::perp(d);
        let slot = Polygon::new(vec![c - d * 2.0 - n * 15.0, c + d * 2.0 - n * 15.0, c + d * 2.0 + n * 15.0, c - d * 2.0 + n * 15.0]);
        remove(&mut m, &flat, 0, std::slice::from_ref(&slot)).unwrap();
        let after = flatten(&m);
        assert!(after.is_ok(), "{:?}", after.errors);
        let part = &after.parts[0];
        assert!((before - part.area() - 120.0).abs() < 1e-6, "{}", before - part.area());
        // One hole, exactly the slot.
        let holes: Vec<&Vec<P2>> = part.outline.iter().flat_map(|o| o.holes.iter()).collect();
        assert_eq!(holes.len(), 1);
        assert!((poly::signed_area(holes[0]).abs() - 120.0).abs() < 1e-6);
        // The bend's centreline stops at the slot: two pieces, 4 short of the whole.
        let b = &part.bends[0];
        assert_eq!(b.center_visible.len(), 2);
        let vis: f64 = b.center_visible.iter().map(|s| s.len()).sum();
        assert!((b.center.len() - vis - 4.0).abs() < 1e-6, "{vis}");
        // Each piece learns what it lost: the bend region a 4 × allowance rectangle in (s, u).
        let cut = part.cuts.iter().find(|c| matches!(c.source, ReliefSource::Flat { .. })).unwrap();
        let (_, on_bend) = cut.removed.iter().find(|(s, _)| matches!(s, PieceSource::Bend(_))).unwrap();
        let ba = m.joints[0].bend().unwrap().allowance(&m.params).unwrap();
        let (lo, hi) = on_bend[0].bounds().unwrap();
        assert!(((hi.x - lo.x) - 4.0).abs() < 1e-6 && ((hi.y - lo.y) - ba).abs() < 1e-6, "{lo:?} {hi:?}");
        // Removed from both walls too.
        assert_eq!(cut.removed.iter().filter(|(s, _)| matches!(s, PieceSource::Wall(_))).count(), 2);
    }

    #[test]
    fn a_cut_that_misses_is_refused() {
        let mut m = samples::l_bracket(Params::default(), true).unwrap();
        let flat = flatten(&m);
        assert_eq!(remove(&mut m, &flat, 0, &[rect(500.0, 500.0, 510.0, 510.0)]), Err(FlatEditError::MissesMaterial));
        assert!(m.flat_cuts.is_empty());
    }

    #[test]
    fn a_tab_added_in_the_flat_grows_its_wall() {
        let p = Params::default();
        let mut m = samples::l_bracket(p, true).unwrap();
        let flat = flatten(&m);
        let before = flat.parts[0].area();
        let base = m.walls[0].outline.area();
        // The base is (0..50) × (0..40) less the bend's side; a 10 × 8 tab on its y = 0 edge.
        let tab = rect(10.0, -8.0, 20.0, 0.0);
        add(&mut m, &flat, 0, std::slice::from_ref(&tab)).unwrap();
        let after = flatten(&m);
        assert!(after.is_ok(), "{:?}", after.errors);
        assert!((after.parts[0].area() - before - 80.0).abs() < 1e-6);
        assert!((m.walls[0].outline.area() - base - 80.0).abs() < 1e-6, "{}", m.walls[0].outline.area());
        // Floating material is refused.
        let mut m2 = samples::l_bracket(p, true).unwrap();
        assert_eq!(add(&mut m2, &flat, 0, &[rect(-30.0, -30.0, -20.0, -20.0)]), Err(FlatEditError::Detached));
    }
}
