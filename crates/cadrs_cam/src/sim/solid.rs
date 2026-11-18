//! The simulation on a solid stock: the tool at any angle, cutters that undercut.
//!
//! [`simulate_mill_solid`] is to a [`TriDexel`] stock what [`super::simulate_mill_on`] is to a
//! height map: it runs the operations' moves over the stock and reports the same things
//! (gouges against the part, rapids into material, flutes too short, the holder or the shank
//! in the stock, anything in the workholding). The cutting itself, the tool's convex pieces
//! and how exact they are, is [`crate::dexel`]'s.

use crate::dexel::{Pose, SimCutter, TriDexel, Which};
use crate::geom::P3;
use crate::tool::Tool;
use crate::toolpath::Segment;

use super::MAX_REPORTED;

/// One operation of a solid simulation ([`simulate_mill_solid`]).
pub struct SolidOp<'a> {
    pub tool: &'a Tool,
    /// Its motion, in `frame`'s coordinates, each move with the tool's axis at its ends
    /// ([`Segment::axis`]).
    pub segments: &'a [Segment],
    /// The frame the motion is in (the operation's work coordinate system, or its tilted work
    /// plane), in the coordinates the stock's own frame is given in: the model's.
    pub frame: crate::geom::Frame,
    /// The tag its cuts leave on the stock's surfaces (1 to 254), which the stock's mesh is
    /// coloured by.
    pub tag: u8,
}

/// What a solid simulation found: what [`super::MillResult`] reports, on a stock cut from any side.
/// Places are in the model's coordinates (the ones the stock's frame is given in).
#[derive(Debug, Clone, PartialEq)]
pub struct SolidResult {
    /// The in-process stock after the operations, its gouged surfaces tagged
    /// [`crate::dexel::GOUGE`].
    pub stock: TriDexel,
    /// Volume removed (mm³).
    pub removed: f64,
    /// Where cuts went into the part (up to [`MAX_REPORTED`]), how many surface points in all,
    /// and the deepest (mm).
    pub gouges: Vec<P3>,
    pub gouge_count: usize,
    pub gouge_depth: f64,
    /// Rapids into material: (operation, where).
    pub rapid_hits: Vec<(usize, P3)>,
    /// Cuts with the shank above the flutes, the holder or the arbor in the stock:
    /// (operation, where).
    pub holder_hits: Vec<(usize, P3)>,
    /// Moves (rapids too) with any of the tool in the workholding: (operation, where).
    pub fixture_hits: Vec<(usize, P3)>,
    /// The most material left on the part (mm).
    pub excess: f64,
}

/// Runs the operations over a solid stock, the tool at whatever angle each move holds it.
///
/// `part` (on the stock's lattice: [`TriDexel::of_solid`] with the stock's frame, box and
/// cell) is what the finished stock is compared with; `fixtures` (on any lattice) is the
/// workholding. Each cutting move is taken in pieces a quarter of the flutes long at most:
/// for each, the shank and the holder are tested against the stock as it is (so the shank
/// following the flutes down a wall they have just cleared isn't a hit), then the cutter
/// cuts. Rapids are tested with the whole tool pulled in by half a cell, cuts' shanks and
/// holders by the cutter's tolerance, so a tool running along a surface it cut isn't reported.
pub fn simulate_mill_solid(
    stock: TriDexel,
    part: Option<&TriDexel>,
    fixtures: Option<&TriDexel>,
    ops: &[SolidOp],
) -> SolidResult {
    simulate_mill_solid_with(stock, part, fixtures, ops, &[], &mut |_, _| {})
}

/// [`simulate_mill_solid`], handing `snap` the stock as it is along the way: after the first
/// `n` moves (counted through all the operations' segments in order) for each `n` of `after`
/// (ascending), before the gouges are marked. For a preview that shows the stock being cut.
pub fn simulate_mill_solid_with(
    mut stock: TriDexel,
    part: Option<&TriDexel>,
    fixtures: Option<&TriDexel>,
    ops: &[SolidOp],
    after: &[usize],
    snap: &mut dyn FnMut(usize, &TriDexel),
) -> SolidResult {
    let mut done = 0usize;
    let mut marks = after.iter().copied().peekable();
    // (Marks at 0 come before any move.)
    while let Some(n) = marks.next_if(|n| *n == 0) {
        snap(n, &stock);
    }
    let cell = stock.cell;
    let eps = crate::dexel::sim_eps(cell);
    // What a surface the tool cut itself can be off by: the cutter's own tolerance each way.
    let graze = 2.0 * eps + 0.01;
    let lean = (cell * 0.5).max(graze);
    let mut removed = 0.0;
    let (mut rapid_hits, mut holder_hits, mut fixture_hits) = (Vec::new(), Vec::new(), Vec::new());
    let push = |v: &mut Vec<(usize, P3)>, k: usize, p: P3| {
        if v.len() < MAX_REPORTED {
            v.push((k, p));
        }
    };
    for (k, op) in ops.iter().enumerate() {
        let cutter = SimCutter::new(op.tool, eps);
        let fixture_cutter =
            fixtures.map(|f| SimCutter::new(op.tool, crate::dexel::sim_eps(f.cell)));
        let flutes = op.tool.cutting_length.max(0.1);
        for s in op.segments {
            // The stock after the moves before this one, where a mark asks for it.
            while let Some(n) = marks.next_if(|n| *n <= done) {
                snap(n, &stock);
            }
            done += 1;
            let at = |f: f64| Pose {
                tip: op.frame.to_world(crate::geom::lerp(s.from, s.to, f)),
                axis: op.frame.dir_to_world(s.axis_at(f)),
            };
            let (from, to) = (at(0.0), at(1.0));
            if let (Some(fx), Some(c)) = (fixtures, &fixture_cutter)
                && let Some(p) = fx.hits(c, Which::All, from, to, 2.0 * c.eps + 0.01)
            {
                push(&mut fixture_hits, k, p);
            }
            if s.is_rapid() {
                if let Some(p) = stock.hits(&cutter, Which::All, from, to, lean) {
                    push(&mut rapid_hits, k, p);
                }
                continue;
            }
            let [a0, a1] = s.axes();
            let turn = crate::kinematics::angle_between(a0, a1);
            let pieces = (crate::geom::dist(s.from, s.to) / (flutes / 4.0).max(cell))
                .max(turn / 5.0)
                .ceil()
                .clamp(1.0, 10_000.0) as usize;
            let mut hit = false;
            for i in 0..pieces {
                let (p0, p1) = (
                    at(i as f64 / pieces as f64),
                    at((i + 1) as f64 / pieces as f64),
                );
                if !hit && let Some(p) = stock.hits(&cutter, Which::Assembly, p0, p1, graze) {
                    push(&mut holder_hits, k, p);
                    hit = true;
                }
                removed += stock.cut(&cutter, p0, p1, op.tag);
            }
        }
    }
    for n in marks {
        snap(n, &stock);
    }
    let d = match part {
        Some(part) => stock.mark_gouges(part, (cell * 0.5).max(0.01)),
        None => Default::default(),
    };
    SolidResult {
        stock,
        removed,
        gouges: d.gouges,
        gouge_count: d.gouge_count,
        gouge_depth: d.gouge_depth,
        rapid_hits,
        holder_hits,
        fixture_hits,
        excess: d.excess,
    }
}

#[cfg(test)]
mod tests;
