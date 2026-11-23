//! Corner joins of a solid sweep (OCCT's `BRepFill_RightCorner` and `BRepFill_RoundCorner`):
//! where the path's tangent breaks, the piece before the corner and the piece after it (the
//! profile turned with the path by the least rotation) are joined mitred or round.
//!
//! - **Mitre**: each piece is extended past the corner by a prism of its cap and cut at the
//!   plane bisecting the corner (normal `d1 + d2`); the two cut faces coincide for a profile
//!   square to the path and the pieces unite there.
//! - **Round**: the pieces meet square to the path at the corner, overlapping inside it; the
//!   outside is filled by the half of the end cap away from the turn, turned about the corner's
//!   axis (`d1 × d2` through the corner) by the corner's angle.
//!
//! The cuts are booleans with boxes local to the corner (a few times the profile's reach), so
//! parts of the sweep elsewhere are untouched. Faces keep their names: an extension's side
//! faces are its profile curves', and a round knuckle's side faces take the names of the faces
//! its edges came from.

use super::path::glue;
use super::{BodyMove, Join, Prism, Turn, sweep_full};
use crate::native::features::{Tagged, tagged_bool};
use crate::native::math::*;
use crate::native::profile::{MergedFace, Piece};
use crate::native::tol;
use crate::native::topo::Body;
use crate::{BoolOp, KernelError, Loop, Origin, Plane, Profile, Region, Result};

/// The largest corner (radians) a sweep joins: past it the mitre plane runs almost along the
/// path.
const MAX_CORNER: f64 = 179.0 * PI / 180.0;

/// A corner of a path: its point, the path's direction into it and out of it, and the profile's
/// placement as it arrives (`end`) and as it leaves, turned (`start`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Corner {
    pub v: P3,
    pub d1: V3,
    pub d2: V3,
    pub end: Plane,
    pub start: Plane,
}

impl Corner {
    /// The corner as a sweep running the other way meets it.
    pub fn reversed(self) -> Corner {
        Corner {
            v: self.v,
            d1: -self.d2,
            d2: -self.d1,
            end: self.start,
            start: self.end,
        }
    }
}

/// Joins `next` onto `acc` (the sweep so far, ending at the corner `c`) as `join` asks.
pub(crate) fn join(
    faces: &[MergedFace],
    chains: &[Vec<Piece>],
    acc: &Tagged,
    next: &Tagged,
    c: &Corner,
    join: Join,
) -> Result<Tagged> {
    let theta = c.d1.angle(&c.d2);
    if !(theta <= MAX_CORNER) {
        return Err(KernelError::InvalidParameter(
            "The sweep path turns back on itself at a corner".into(),
        ));
    }
    let failed = |why: &str| {
        KernelError::OpenResult(format!(
            "the sweep's corner at {:?} could not be joined: {why}",
            c.v
        ))
    };
    let (ends, reach, region) = end_cap(acc, &c.v).ok_or_else(|| failed("no end cap"))?;
    match join {
        Join::None => crate::native::features::tagged_union(acc, next),
        Join::Mitre => {
            let m = (c.d1 + c.d2).normalize();
            let ext = reach * (theta * 0.5).tan() * 1.05 + reach * 1e-3 + tol::POINT;
            let size = 4.0 * reach + 2.0 * ext;
            // The piece before: extended along d1, cut at the mitre plane.
            let ea = Tagged::from_built(sweep_full(
                faces,
                chains,
                &Prism {
                    plane: c.end,
                    dir: c.d1,
                },
                0.0,
                ext,
                false,
                true,
            )?);
            let a = glue(acc, &ea)
                .map_or_else(|| crate::native::features::tagged_union(acc, &ea), Ok)?;
            let a = tagged_bool(
                &a,
                &local_box(&c.v, &m, size, Origin::EndCap { region })?,
                BoolOp::Subtract,
            )?;
            // The piece after: extended back along −d2 (its far cap a start cap), cut too.
            let start_region = start_region(next).unwrap_or(region);
            let mut eb = Tagged::from_built(sweep_full(
                faces,
                chains,
                &Prism {
                    plane: c.start,
                    dir: -c.d2,
                },
                0.0,
                ext,
                false,
                true,
            )?);
            super::path::swap_caps(&mut eb);
            let b = glue(&eb, next)
                .map_or_else(|| crate::native::features::tagged_union(&eb, next), Ok)?;
            let b = tagged_bool(
                &b,
                &local_box(
                    &c.v,
                    &-m,
                    size,
                    Origin::StartCap {
                        region: start_region,
                    },
                )?,
                BoolOp::Subtract,
            )?;
            // Both cut by the one mitre plane: their cut faces coincide, glued there where
            // their edges match (else united).
            glue(&a, &b).map_or_else(|| crate::native::features::tagged_union(&a, &b), Ok)
        }
        Join::Round => {
            let axis =
                c.d1.cross(&c.d2)
                    .try_normalize(tol::DEGENERATE)
                    .ok_or_else(|| failed("no turn"))?;
            // Towards the inside of the turn, square to d1.
            let inward = (c.d2 - c.d1 * c.d2.dot(&c.d1)).normalize();
            let cap = crate::native::kernel::subset(&acc.body, &ends, false);
            let size = 4.0 * reach + 1.0;
            let inner = local_box(&c.v, &inward, size, Origin::EndCap { region })?;
            let outer = crate::native::boolean::boolean_bodies(&cap, &inner.body, BoolOp::Subtract)
                .map_err(|e| failed(&e.message()))?
                .body;
            let x = (c.d1 - axis * c.d1.dot(&axis)).normalize();
            let turn = Turn {
                plane: Plane::top(),
                f: Frame {
                    o: c.v,
                    x,
                    y: axis.cross(&x),
                    z: axis,
                },
                up: true,
            };
            let built = super::sweep_body(&outer, &BodyMove::Turn(&turn), theta, false, 0)?;
            // Name the knuckle's side faces after the faces of `acc` their edges came from.
            let mut tags = vec![Vec::new(); built.body.faces.len()];
            for (f, o) in &built.origins {
                tags[*f] = match o {
                    Origin::FromEdge { edge, .. } => {
                        let e = &outer.edges[edge.0 as usize];
                        let p = e.point(e.mid_t());
                        side_tags(acc, &ends, &p, reach)
                    }
                    _ => vec![Origin::EndCap { region }],
                };
            }
            let knuckle = Tagged {
                body: built.body,
                tags,
            };
            let out = crate::native::features::tagged_union(acc, &knuckle)?;
            crate::native::features::tagged_union(&out, next)
        }
    }
}

/// The end-cap faces of `t` (the profile where the sweep so far ends: at the corner, square
/// to the path or not), how far their boundary reaches from the corner `v`, and the region of
/// the first.
fn end_cap(t: &Tagged, v: &P3) -> Option<(Vec<usize>, f64, u64)> {
    let mut faces = Vec::new();
    let mut region = None;
    let mut reach: f64 = 0.0;
    for (f, tags) in t.tags.iter().enumerate() {
        let Some(r) = tags.iter().find_map(|o| {
            if let Origin::EndCap { region } = o {
                Some(*region)
            } else {
                None
            }
        }) else {
            continue;
        };
        reach = boundary_points(&t.body, f)
            .iter()
            .fold(reach, |m, p| m.max((p - v).norm()));
        region.get_or_insert(r);
        faces.push(f);
    }
    (!faces.is_empty() && reach > 0.0).then(|| (faces, reach, region.unwrap_or(0)))
}

/// The region of the first start cap of `t`.
fn start_region(t: &Tagged) -> Option<u64> {
    t.tags.iter().flatten().find_map(|o| {
        if let Origin::StartCap { region } = o {
            Some(*region)
        } else {
            None
        }
    })
}

/// Points along face `f`'s boundary (each edge's ends and three points between).
fn boundary_points(b: &Body, f: usize) -> Vec<P3> {
    let mut out = Vec::new();
    for c in b.faces[f].loops.iter().flatten() {
        let e = &b.edges[c.edge];
        for k in 0..=4 {
            out.push(e.point(e.t[0] + (e.t[1] - e.t[0]) * k as f64 / 4.0));
        }
    }
    out
}

/// The names of the face of `t` beside the caps `ends` along the cap edge through `p` (a
/// point of an edge of the caps, as cut).
fn side_tags(t: &Tagged, ends: &[usize], p: &P3, reach: f64) -> Vec<Origin> {
    let tol = tol::REL_POINT * (1.0 + reach);
    let b = &t.body;
    let edge = ends
        .iter()
        .flat_map(|&f| b.faces[f].loops.iter().flatten())
        .map(|c| c.edge)
        .find(|&e| {
            let ed = &b.edges[e];
            !ed.degenerate && (ed.point(ed.curve.closest(p, ed.t[0], ed.t[1])) - p).norm() <= tol
        });
    let Some(edge) = edge else { return Vec::new() };
    (0..b.faces.len())
        .filter(|f| !ends.contains(f))
        .find(|&f| b.faces[f].loops.iter().flatten().any(|c| c.edge == edge))
        .map(|f| t.tags[f].clone())
        .unwrap_or_default()
}

/// A box on the `n` side of the plane through `o` square to `n`, `size` deep and `2 size`
/// wide, every face named `tag` (the face on the plane is what a cut by it leaves).
fn local_box(o: &P3, n: &V3, size: f64, tag: Origin) -> Result<Tagged> {
    let f = Frame::from_z(*o, *n);
    let plane = Plane {
        origin: *o,
        x_dir: nalgebra::Unit::new_normalize(f.x),
        normal: nalgebra::Unit::new_normalize(*n),
    };
    let p2 = nalgebra::Point2::new;
    let l = |a, b| crate::Curve2::Line { a, b, source: None };
    let c = [
        p2(-size, -size),
        p2(size, -size),
        p2(size, size),
        p2(-size, size),
    ];
    let sq = Region {
        outer: Loop {
            curves: (0..4).map(|i| l(c[i], c[(i + 1) % 4])).collect(),
        },
        holes: vec![],
        source: None,
    };
    let regions = crate::native::profile::regions(&Profile::new(plane, vec![sq]))?;
    let mf = crate::native::profile::merge_regions(&regions)?;
    let built = sweep_full(&mf, &[], &Prism { plane, dir: *n }, 0.0, size, false, true)?;
    let tags = vec![vec![tag]; built.body.faces.len()];
    Ok(Tagged {
        body: built.body,
        tags,
    })
}

/// Closed forms for a 2 × 2 square carried round the corner of an L-shaped path (10 along x,
/// then 10 along y): mitred, volume A·L = 80 and area 168; round, the overlap inside the corner
/// (2) gone and a quarter cylinder of radius 1 and height 2 outside it.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::curve::Curve;
    use crate::native::sweep::path::{Seg, sweep_spines};
    use crate::native::topo::CheckLevel;
    use crate::{Curve2, SweepControl};

    fn square_on(plane: Plane) -> Vec<MergedFace> {
        let p2 = nalgebra::Point2::new;
        let c = [p2(-1.0, -1.0), p2(1.0, -1.0), p2(1.0, 1.0), p2(-1.0, 1.0)];
        let sq = Region {
            outer: Loop {
                curves: (0..4)
                    .map(|i| Curve2::Line {
                        a: c[i],
                        b: c[(i + 1) % 4],
                        source: None,
                    })
                    .collect(),
            },
            holes: vec![],
            source: None,
        };
        crate::native::profile::merge_regions(
            &crate::native::profile::regions(&Profile::new(plane, vec![sq])).unwrap(),
        )
        .unwrap()
    }

    fn seg(a: P3, b: P3) -> Seg {
        Seg {
            c: Curve::Line { o: a, d: b - a },
            t0: 0.0,
            t1: 1.0,
            rev: false,
        }
    }

    fn ell(join: Join) -> Tagged {
        // The profile square to the path at its start (its plane's normal +x).
        let plane = Plane {
            origin: P3::origin(),
            x_dir: V3::y_axis(),
            normal: V3::x_axis(),
        };
        let path = vec![
            seg(P3::new(0.0, 0.0, 0.0), P3::new(10.0, 0.0, 0.0)),
            seg(P3::new(10.0, 0.0, 0.0), P3::new(10.0, 10.0, 0.0)),
        ];
        sweep_spines(
            &square_on(plane),
            &[],
            plane,
            vec![(path, false)],
            SweepControl::None,
            join,
            true,
        )
        .unwrap()
    }

    #[test]
    fn mitred_corner() {
        let t = ell(Join::Mitre);
        let issues = t.body.check_with(CheckLevel::Full);
        assert!(issues.is_empty(), "{issues:?}");
        let m = crate::native::mass::body_mass(&t.body);
        assert!((m.volume - 80.0).abs() < 1e-6, "{}", m.volume);
        assert!((m.surface_area - 168.0).abs() < 1e-6, "{}", m.surface_area);
        // Every face keeps a name: a profile curve's side or a cap.
        assert!(t.tags.iter().all(|ts| !ts.is_empty()));
    }

    #[test]
    fn round_corner() {
        let t = ell(Join::Round);
        let issues = t.body.check_with(CheckLevel::Full);
        assert!(issues.is_empty(), "{issues:?}");
        let m = crate::native::mass::body_mass(&t.body);
        assert!((m.volume - (78.0 + PI / 2.0)).abs() < 1e-6, "{}", m.volume);
        assert!(
            (m.surface_area - (162.0 + 1.5 * PI)).abs() < 1e-6,
            "{}",
            m.surface_area
        );
        assert!(t.tags.iter().all(|ts| !ts.is_empty()));
    }

    #[test]
    fn no_join_keeps_the_old_behaviour() {
        // The profile kept square to x along the y leg sweeps nothing there: an error, never a
        // wrong body.
        let plane = Plane {
            origin: P3::origin(),
            x_dir: V3::y_axis(),
            normal: V3::x_axis(),
        };
        let path = vec![
            seg(P3::new(0.0, 0.0, 0.0), P3::new(10.0, 0.0, 0.0)),
            seg(P3::new(10.0, 0.0, 0.0), P3::new(10.0, 10.0, 0.0)),
        ];
        assert!(
            sweep_spines(
                &square_on(plane),
                &[],
                plane,
                vec![(path, false)],
                SweepControl::None,
                Join::None,
                true
            )
            .is_err()
        );
    }
}
