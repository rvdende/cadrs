//! Toolpath strategies.
//!
//! A toolpath picks a [`Type`] (what it machines: edges, faces, bodies or holes) and a
//! [`Pattern`] (how), as the Create toolpath dialog does; the patterns a tool and machine can
//! use are [`Pattern::available`]. Its [`Params`] are the dialog's settings: the Tool path tab
//! (drive selection, offsets, step down and over, sorting, heights, speeds, posting), the Link
//! tab (clearance, entry, leads, gaps) and the Misc tab (smoothing, tolerance), plus the beam
//! settings of laser, plasma and waterjet cuts.
//!
//! [`generate`] makes the toolpath from an [`Input`]: the parts in the setup's coordinates
//! ([`crate::model::Model`]), the stock, the tool and, for rest machining, the in-process
//! stock left by the operations before.
//!
//! - [`mill`]: Face, 2-axis rough (pockets and bosses at levels), 2-axis profile, 2-axis
//!   chamfer, Engrave, Flatlands and the 3D Rough (constant-Z roughing).
//! - [`finish`]: the 3D finishing patterns, Parallel (drop-cutter raster) and Constant Z
//!   (waterlines).
//! - [`holes`]: Drill (drill, peck, chip break and tap cycles) and Bore (helical hole milling).
//! - [`beam`]: Cut (through-cutting with kerf compensation, pierces, leads and microjoints,
//!   holes before outlines), Etch (marking along edges) and Fill (laser hatching).

pub mod adaptive;
pub mod beam;
pub mod collision;
pub mod finish;
pub mod holes;
pub mod link;
pub mod mill;
pub mod path;

use serde::{Deserialize, Serialize};

use crate::geom::{Aabb, P2};
use crate::heightmap::HeightMap;
use crate::machine::MachineKind;
use crate::model::Model;
use crate::tool::{Tool, ToolKind};
use crate::toolpath::{Step, Toolpath};

mod params;
pub use params::*;

/// The stock a job starts from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stock {
    pub bounds: Aabb,
    /// A stock body of its own (a casting): its triangles. `None`: the box.
    pub triangles: Option<Vec<[crate::geom::P3; 3]>>,
}

impl Stock {
    pub fn block(bounds: Aabb) -> Stock {
        Stock {
            bounds,
            triangles: None,
        }
    }

    /// The stock's footprint from above.
    pub fn footprint(&self) -> crate::region::Region {
        match &self.triangles {
            Some(t) => Model::from_triangles("stock", t.clone()).silhouette(),
            None => vec![crate::region::rect(
                [self.bounds.min[0], self.bounds.min[1]],
                [self.bounds.max[0], self.bounds.max[1]],
            )],
        }
    }

    /// The stock's top at each point, as a height map.
    pub fn height_map(&self, cell: f64) -> HeightMap {
        match &self.triangles {
            Some(t) => HeightMap::of_triangles(
                t,
                [self.bounds.min[0], self.bounds.min[1]],
                [self.bounds.max[0], self.bounds.max[1]],
                cell,
            ),
            None => HeightMap::block(&self.bounds, cell),
        }
    }
}

/// A rectangle far round the stock and the model: the whole plane as far as a strategy is
/// concerned (the complement of a keep-out is this minus it).
pub fn everywhere(input: &Input) -> crate::region::Ring {
    let mut b = input.stock.bounds;
    let m = input.model.bounds();
    for k in 0..3 {
        b.min[k] = b.min[k].min(m.min[k]);
        b.max[k] = b.max[k].max(m.max[k]);
    }
    crate::region::rect(
        [b.min[0] - 1e4, b.min[1] - 1e4],
        [b.max[0] + 1e4, b.max[1] + 1e4],
    )
}

/// What a strategy works from.
pub struct Input<'a> {
    pub model: &'a Model,
    pub stock: &'a Stock,
    pub tool: &'a Tool,
    pub machine: MachineKind,
    pub params: &'a Params,
    /// The stock as the operations before left it (rest machining, re-machining).
    pub rest: Option<&'a HeightMap>,
    /// The workholding's triangles (vises, clamps, fixture plates): never cut, never touched
    /// by the tool or its holder ([`collision::apply`]).
    pub fixtures: &'a [[crate::geom::P3; 3]],
}

impl Input<'_> {
    /// The z of a height.
    pub fn height(&self, h: Height) -> f64 {
        let mb = self.model.bounds();
        let base = match h.from {
            HeightRef::StockTop => self.stock.bounds.max[2],
            HeightRef::StockBottom => self.stock.bounds.min[2],
            HeightRef::ModelTop => mb.max[2],
            HeightRef::ModelBottom => mb.min[2],
            HeightRef::Selection => self.selection_z().unwrap_or(mb.max[2]),
            HeightRef::Origin => 0.0,
        };
        base + h.offset
    }

    /// The highest material over the disc of radius `r` round `c`: the stock as the operations
    /// before left it, or the stock itself (the top of the box, or of a stock body over the
    /// disc's centre and rim).
    pub fn material_top(&self, c: P2, r: f64) -> f64 {
        if let Some(rest) = self.rest {
            let flat = Tool::new(ToolKind::EndMill, 2.0 * r.max(1e-3));
            return rest.drop_cutter(&flat, c, f64::NEG_INFINITY);
        }
        match &self.stock.triangles {
            None => self.stock.bounds.max[2],
            Some(tris) => {
                let pts = std::iter::once(c).chain((0..8).map(|k| {
                    let a = k as f64 * std::f64::consts::FRAC_PI_4;
                    [c[0] + r * a.cos(), c[1] + r * a.sin()]
                }));
                pts.flat_map(|p| {
                    tris.iter()
                        .filter(move |t| crate::model::point_in_tri2(p, t))
                        .filter_map(move |t| crate::model::height_on(t, p))
                })
                .fold(f64::NEG_INFINITY, f64::max)
            }
        }
    }

    /// The drive geometry's lowest height: picked edges or faces.
    pub fn selection_z(&self) -> Option<f64> {
        let d = &self.params.drive;
        let edges = d
            .edges
            .iter()
            .filter_map(|n| self.model.edge(n))
            .flat_map(|e| e.points.iter().map(|p| p[2]));
        let faces = d
            .faces
            .iter()
            .filter_map(|n| self.model.face(n))
            .flat_map(|f| f.loops.iter().flatten().map(|p| p[2]));
        edges.chain(faces).reduce(f64::min)
    }

    /// Which side of the segment `a`→`b` at height `z` the part is on (`Some(true)`: left),
    /// probed just either side of its middle: first where material stands above it (an edge at
    /// a wall's foot has the wall on one side, the floor on the other), else where there is
    /// material up to it (a top edge); `None` when neither tells.
    fn material_beside(&self, a: P2, b: P2, z: f64) -> Option<bool> {
        let m = crate::geom::lerp2(a, b, 0.5);
        let t = path::unit(crate::geom::sub2(b, a));
        let e = 0.05;
        let left = [m[0] - t[1] * e, m[1] + t[0] * e];
        let right = [m[0] + t[1] * e, m[1] - t[0] * e];
        let top = |p: P2| self.model.top_at(p);
        let above = |p: P2| top(p).is_some_and(|h| h > z + 1e-3);
        let up_to = |p: P2| top(p).is_some_and(|h| h >= z - 1e-3);
        if above(left) != above(right) {
            return Some(above(left));
        }
        (up_to(left) != up_to(right)).then(|| up_to(left))
    }

    /// Which side of a chain at `z` the part is on (`Some(true)`: left of its way), from its
    /// longest segment ([`Input::material_beside`]).
    pub fn material_left_of(&self, c: &path::Contour, z: f64) -> Option<bool> {
        Self::longest_segment(c).and_then(|(a, b)| self.material_beside(a, b, z))
    }

    /// Whether Inside on the picked open chains puts the tool on the material's side of any (it
    /// would cut into the part).
    pub fn inside_into_material(&self) -> bool {
        if self.params.side != Side::Inside {
            return false;
        }
        let polys: Vec<Vec<crate::geom::P3>> = self
            .params
            .drive
            .edges
            .iter()
            .filter_map(|n| self.model.edge(n))
            .map(|e| e.points.clone())
            .collect();
        let z = self.selection_z().unwrap_or(self.model.bounds().max[2]);
        path::chain(&polys, 1e-3)
            .iter()
            .any(|c| !c.closed && self.material_left_of(c, z).is_some())
    }

    /// The longest segment of `c` (closing a closed one).
    fn longest_segment(c: &path::Contour) -> Option<(P2, P2)> {
        let n = c.pts.len();
        let segs = if c.closed { n } else { n.saturating_sub(1) };
        (0..segs)
            .map(|i| (c.pts[i], c.pts[(i + 1) % n]))
            .max_by(|x, y| crate::geom::dist2(x.0, x.1).total_cmp(&crate::geom::dist2(y.0, y.1)))
    }

    /// An open chain at `z` moved `dist` off it, to the side `side` asks for: Automatic and
    /// Outside away from the material beside it, Inside towards it (beside neither or both:
    /// left for Automatic and Outside, right for Inside). With whether its free side (the tool's)
    /// is left of its way.
    pub fn offset_open_chain(
        &self,
        c: &path::Contour,
        z: f64,
        dist: f64,
        side: Side,
    ) -> (Vec<P2>, bool) {
        let material_left = self.material_left_of(c, z);
        let inside = side == Side::Inside;
        let free_left = match material_left {
            Some(m) => m == inside,
            None => !inside,
        };
        let d = if free_left { dist } else { -dist };
        (path::offset_open(&c.pts, d), free_left)
    }

    /// Whether the part lies inside a closed contour at height `z`: probed just either side of
    /// its longest segment (a loop round a pocket has the part outside it even though the
    /// part's material is below the pocket's floor).
    pub fn part_inside(&self, c: &path::Contour, z: f64) -> bool {
        let Some((a, b)) = Self::longest_segment(c) else {
            return false;
        };
        let left_inside = c.ccw();
        if let Some(l) = self.material_beside(a, b, z) {
            return l == left_inside;
        }
        // Both or neither: whether the material is inside at all.
        let mut ring = c.pts.clone();
        if !left_inside {
            ring.reverse();
        }
        self.model.top_at(path::inner_point(&ring)).is_some()
    }

    pub fn top(&self) -> f64 {
        self.height(self.params.heights.top)
    }

    /// The bottom height. Selection with nothing picked, or with the picked edges at the top
    /// (a contour's top edges, the ones in view: [`Input::bottom_from_top_pick`]): the model's
    /// bottom.
    pub fn bottom(&self) -> f64 {
        let h = self.params.heights.bottom;
        if h.from == HeightRef::Selection
            && (self.selection_z().is_none() || self.bottom_from_top_pick())
        {
            return self.model.bounds().min[2] + h.offset;
        }
        self.height(h)
    }

    /// Whether the bottom is the selection's and the picks are at (or over) the top height:
    /// cut down the wall under them instead.
    pub fn bottom_from_top_pick(&self) -> bool {
        self.params.heights.bottom.from == HeightRef::Selection
            && self.selection_z().is_some_and(|z| z >= self.top() - 1e-6)
    }

    /// The highest of the stock, the model and the workholding.
    fn highest(&self) -> f64 {
        let fixtures = self
            .fixtures
            .iter()
            .flatten()
            .map(|p| p[2])
            .fold(f64::NEG_INFINITY, f64::max);
        self.stock.bounds.max[2]
            .max(self.model.bounds().max[2])
            .max(fixtures)
    }

    /// The clearance plane (absolute): over the stock, the model and the workholding.
    pub fn clearance(&self) -> f64 {
        self.highest() + self.params.links.clearance
    }

    /// The rapid plane (absolute).
    pub fn rapid_plane(&self) -> f64 {
        self.highest() + self.params.links.rapid_distance
    }

    /// Depth levels from `top` down to `bottom` by the step down (the first one by the first
    /// step down when set), always ending exactly at `bottom`; with the model's flat floors in
    /// between added (so floors are cut at their height).
    pub fn levels(&self, top: f64, bottom: f64, floors: bool) -> Vec<f64> {
        let p = self.params;
        let mut v = Vec::new();
        if bottom >= top - 1e-9 {
            return vec![bottom];
        }
        let mut z = top;
        let mut first = true;
        loop {
            let step = if first && p.first_step_down > 0.0 {
                p.first_step_down
            } else {
                p.step_down
            };
            first = false;
            z -= step.max(1e-3);
            if z <= bottom + 1e-6 {
                v.push(bottom);
                break;
            }
            v.push(z);
            if v.len() > 10_000 {
                break;
            }
        }
        if floors {
            for f in self.model.floor_levels() {
                let fz = f + p.floor_stock;
                if fz < top - 1e-6 && fz > bottom + 1e-6 && !v.iter().any(|z| (z - fz).abs() < 1e-3)
                {
                    v.push(fz);
                }
            }
            v.sort_by(|a, b| b.total_cmp(a));
        }
        v
    }
}

/// Generates the toolpath.
pub fn generate(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    p.check(input.tool)?;
    input.tool.check()?;
    if !p.pattern.allowed(input.tool.kind, input.machine) {
        return Err(format!(
            "{} can't be cut with a {} on a {}",
            p.pattern.label(),
            input.tool.kind.label().to_lowercase(),
            input.machine.label().to_lowercase()
        ));
    }
    if input.model.triangles.is_empty() && p.drive.is_empty() {
        return Err("The job has no bodies to machine".into());
    }
    let tp = match p.pattern {
        Pattern::Face => mill::face(input),
        Pattern::Rough2D => mill::rough(input, false),
        Pattern::Rough3D => mill::rough(input, true),
        Pattern::Adaptive => adaptive::adaptive(input),
        Pattern::Profile2D => mill::profile(input),
        Pattern::Chamfer2D => mill::chamfer(input),
        Pattern::Engrave => mill::engrave(input),
        Pattern::Flatlands => mill::flatlands(input),
        Pattern::Parallel => finish::parallel(input),
        Pattern::ConstantZ => finish::constant_z(input),
        Pattern::Scallop => finish::scallop(input),
        Pattern::Spiral => finish::spiral(input),
        Pattern::Pencil => finish::pencil(input),
        Pattern::ThreadMill => holes::thread_mill(input),
        Pattern::Drill => holes::drill(input),
        Pattern::Bore => holes::bore(input),
        Pattern::Cut => beam::cut(input),
        Pattern::Etch => beam::etch(input),
        Pattern::Fill => beam::fill(input),
    }?;
    // Repeated, then every copy checked.
    let tp = p.repeat.apply(tp);
    let mut tp = if input.machine.is_milling() {
        collision::apply(input, tp)
    } else {
        tp
    };
    // Through the bottom of the stock (a hole's break-through, a profile's cut-off): whatever
    // the part sits on is there.
    if input.machine.is_milling() {
        let lowest = tp
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Feed { to, .. } | Step::Arc { to, .. } => Some(to[2]),
                Step::Drill(c) => Some(c.bottom),
                _ => None,
            })
            .fold(f64::INFINITY, f64::min);
        let under = input.stock.bounds.min[2] - lowest;
        if under > 1e-3 {
            tp.warnings.push(format!(
                "It cuts {} mm below the stock's bottom: what the part sits on must be clear \
                 there (parallels, a spoil board)",
                crate::tool::fmt_mm(under)
            ));
        }
    }
    if tp.is_empty() {
        return Err(format!(
            "{} found nothing to cut{}",
            p.pattern.label(),
            tp.warnings
                .first()
                .map(|w| format!(": {w}"))
                .unwrap_or_default()
        ));
    }
    Ok(tp)
}

/// The levels a toolpath sections the model at (the studio asks the kernel for exact sections
/// there before generating).
pub fn section_levels(input: &Input) -> Vec<f64> {
    match input.params.pattern {
        Pattern::ConstantZ => input.levels(
            input.top().min(input.model.bounds().max[2]),
            input.bottom(),
            true,
        ),
        _ => Vec::new(),
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
