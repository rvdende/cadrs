//! The sweep engine: one builder that sweeps planar profiles (faces and open chains) and
//! bodies along a motion, and the motions it sweeps along.
//!
//! **The builder** ([`sweep_full`]): a profile point q traces a *rail* as the sweep parameter s
//! runs from s0 to s1; each profile piece sweeps a side face whose surface and p-curves the
//! motion gives ([`SweepKind::side`]); the profile itself is the start cap at s0 and the end
//! cap at s1 (none for a whole turn). Faces are named after the profile curves that swept them
//! and the regions their caps close.
//!
//! **The motions** ([`SweepKind`]), from exact to sampled:
//! - [`Prism`]: a straight sweep (extrudes, straight path segments): planes, cylinders and
//!   extrusion surfaces, exact;
//! - [`Revolve`] and [`Turn`]: a turn about an axis in the profile's plane or any axis
//!   (revolves, arcs of a path, SMLib's rotational sweeps): planes, cylinders, cones, spheres,
//!   tori and surfaces of revolution, exact;
//! - [`frames::PathSweep`]: any other path (B-splines, ellipses, arcs the profile isn't square
//!   to, curves out of a plane) with sampled frames: rotation-minimizing (double reflection),
//!   fixed orientation, or a locked direction ([`crate::SweepControl`]); a path that follows a
//!   helix is swept by the helix's exact screw motion, one that bends about as much everywhere
//!   (a thread's) with Frenet frames. The sections are skinned into B-spline
//!   side surfaces whose boundaries are the rails.
//!
//! **Paths** ([`path`]): the path's curves are chained; where the profile's plane crosses the
//! path the sweep starts (both ways when that is part way along an open path; a closed path is
//! restarted there). Each segment gets the exact motion it allows, tangent runs of general
//! segments one [`frames::PathSweep`], and the pieces are joined where they meet: the caps at
//! each joint are glued away ([`path::glue`]). At a corner (a joint where the path's tangent
//! breaks) the profile turns with the path by the least rotation and the pieces are joined
//! ([`Join`]): mitred (the default of [`crate::Kernel::sweep_with`]) or round.
//!
//! The sampled motion runs along the path by length (no jump in speed where segments meet),
//! and a run gets as many sections as keep the skinned surfaces within
//! [`crate::native::tol::LOOP_GAP`] (per unit of the profile's reach) of the true sweep.
//!
//! **Bodies** ([`sweep_body`]): points, wires and sheets swept one kind higher along a straight
//! or turning motion, through the same side faces ([`side_loop`]).

use super::build::{Built, MIN_DEPTH, fix_senses};
use super::curve::Curve;
use super::math::*;
use super::profile::{MergedFace, Piece, Src};
use super::surface::Surface;
use super::topo::*;
use crate::{KernelError, Origin, Result};

mod body;
mod corner;
pub mod frames;
mod kinds;
pub mod path;

pub use body::{BodyMove, Screw, sweep_body};

/// How a sampled sweep ([`frames::PathSweep`]) turns its profile along a path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Frames {
    /// As the caller's [`crate::SweepControl`] asks: for `None` rotation-minimizing frames (a
    /// helix's screw motion, Frenet frames for a path that bends about as much everywhere),
    /// else a fixed orientation or a locked direction.
    Control(crate::SweepControl),
    /// Frenet frames (the principal normal) wherever the path bends everywhere, however its
    /// curvature varies (OCCT's `FR` mode); rotation-minimizing where it doesn't.
    Frenet,
}

/// How a solid sweep joins its pieces at a corner of its path (where the path's tangent
/// breaks), the profile turned with the path there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// No join: the profile keeps its placement round the corner and the pieces are united as
    /// they are (OCCT's `Transformed` mode), which usually fails at a sharp corner.
    None,
    /// Each piece extended to the plane bisecting the corner and cut there (OCCT's
    /// `RightCorner`): exact for a profile square to the path.
    Mitre,
    /// The pieces meet square to the path at the corner and its outside is filled by the
    /// profile turned about the corner (OCCT's `RoundCorner`).
    Round,
}

impl Frames {
    /// The control the exact motions (prisms and turns) follow.
    pub fn control(self) -> crate::SweepControl {
        match self {
            Frames::Control(c) => c,
            Frames::Frenet => crate::SweepControl::None,
        }
    }
}

impl From<crate::SweepControl> for Frames {
    fn from(c: crate::SweepControl) -> Self {
        Frames::Control(c)
    }
}
pub use kinds::{Prism, Revolve, Turn, line2};

/// The geometry of a side face: its surface and the p-curves of the profile piece at a sweep
/// parameter (parametrized by the piece's t) and of the rail of a joint (parametrized by s).
pub struct SideGeo {
    pub surf: Surface,
    pub along: Box<dyn Fn(f64) -> Curve>,
    pub across: Box<dyn Fn(f64) -> Curve>,
}

pub trait SweepKind {
    /// Sketch coordinates (z = 0) to model space at sweep parameter s.
    fn place(&self, s: f64) -> Affine;
    /// The rail traced by the sketch point q (parametrized by s), or None when it stays put
    /// (a point on the axis).
    fn rail(&self, q: P2) -> Option<Curve>;
    /// The side face swept by a piece (`c3` is the piece's curve at s = 0), or None when it
    /// sweeps no area (a piece lying on the axis).
    fn side(&self, piece: &Piece, c3: &Curve) -> Option<SideGeo>;
    /// Whether the sweep leaves the profile plane along its normal.
    fn up(&self) -> bool;
}

/// Sweeps the faces from s0 to s1 (s0 < s1). `whole`: a closed sweep (a whole turn) whose end
/// is its start: no caps, the start edges are seams.
pub fn sweep(
    _plane: &crate::Plane,
    faces: &[MergedFace],
    kind: &dyn SweepKind,
    s0: f64,
    s1: f64,
    whole: bool,
) -> Result<Built> {
    sweep_full(faces, &[], kind, s0, s1, whole, true)
}

/// The loop of the side face a piece sweeps: the piece's start copy `e.0` (used `fwd` when
/// the piece runs along it), the rail at its end, its end copy `e.1` the other way, and the
/// rail at its start back (`rails`: each rail's edge and whether it is degenerate, as the
/// piece starts and ends; `s`: the sweep's range; `t`: the piece's start and end parameters).
/// A degenerate rail (a point on the axis) bounds a planar side face by no coedge at all.
pub(crate) fn side_loop(
    side: &SideGeo,
    e: (usize, usize),
    fwd: bool,
    rails: ((usize, bool), (usize, bool)),
    s: (f64, f64),
    t: (f64, f64),
) -> Vec<Coedge> {
    let plane_side = matches!(side.surf, Surface::Plane { .. });
    let (rs, re) = rails;
    let mut sl = vec![Coedge {
        edge: e.0,
        fwd,
        pc: (side.along)(s.0),
    }];
    if !(re.1 && plane_side) {
        sl.push(Coedge {
            edge: re.0,
            fwd: true,
            pc: (side.across)(t.1),
        });
    }
    sl.push(Coedge {
        edge: e.1,
        fwd: !fwd,
        pc: (side.along)(s.1),
    });
    if !(rs.1 && plane_side) {
        sl.push(Coedge {
            edge: rs.0,
            fwd: false,
            pc: (side.across)(t.0),
        });
    }
    sl
}

/// The side faces swept by one strand of pieces (a closed loop or an open chain): returns the
/// face indices with their origins and the start and end profile coedges (cap boundaries).
struct Strand {
    sides: Vec<(usize, Vec<Src>)>,
    bottom: Vec<Coedge>,
    top: Vec<Coedge>,
}

#[allow(clippy::too_many_arguments)]
fn sweep_strand(
    b: &mut Body,
    lp: &[Piece],
    closed: bool,
    kind: &dyn SweepKind,
    s0: f64,
    s1: f64,
    whole: bool,
    up: bool,
    m0: &Affine,
    m1: &Affine,
) -> Strand {
    let k = lp.len();
    let nj = if closed { k } else { k + 1 };
    let joint = |j: usize| -> P2 {
        if j < k {
            lp[j].start()
        } else {
            lp[k - 1].end()
        }
    };
    let rails: Vec<Option<Curve>> = (0..nj).map(|j| kind.rail(joint(j))).collect();
    let vb: Vec<usize> = (0..nj)
        .map(|j| {
            let q = joint(j);
            b.add_vertex(m0.point(&P3::new(q.x, q.y, 0.0)))
        })
        .collect();
    let vt: Vec<usize> = (0..nj)
        .map(|j| {
            if whole || rails[j].is_none() {
                vb[j]
            } else {
                let q = joint(j);
                b.add_vertex(m1.point(&P3::new(q.x, q.y, 0.0)))
            }
        })
        .collect();
    let rail_e: Vec<(usize, bool)> = (0..nj)
        .map(|j| match &rails[j] {
            Some(c) => (b.add_edge(c.clone(), s0, s1, vb[j], vt[j]), false),
            None => (b.add_degenerate(b.verts[vb[j]].p, s0, s1, vb[j]), true),
        })
        .collect();
    let mut out = Strand {
        sides: Vec::new(),
        bottom: Vec::new(),
        top: Vec::new(),
    };
    let mut turned: Vec<usize> = Vec::new();
    for (i, p) in lp.iter().enumerate() {
        let j = if closed { (i + 1) % k } else { i + 1 };
        let c0 = p.c.transformed(m0);
        let (v0b, v1b) = if p.rev {
            (vb[j], vb[i])
        } else {
            (vb[i], vb[j])
        };
        let (v0t, v1t) = if p.rev {
            (vt[j], vt[i])
        } else {
            (vt[i], vt[j])
        };
        let Some(side) = kind.side(p, &p.c.transformed(&kind.place(0.0))) else {
            let e = b.add_edge(c0.clone(), p.t0, p.t1, v0b, v1b);
            out.bottom.push(Coedge {
                edge: e,
                fwd: !p.rev,
                pc: p.c.clone(),
            });
            out.top.push(Coedge {
                edge: e,
                fwd: !p.rev,
                pc: p.c.clone(),
            });
            continue;
        };
        let eb = b.add_edge(c0.clone(), p.t0, p.t1, v0b, v1b);
        let et = if whole {
            eb
        } else {
            b.add_edge(p.c.transformed(m1), p.t0, p.t1, v0t, v1t)
        };
        out.bottom.push(Coedge {
            edge: eb,
            fwd: !p.rev,
            pc: p.c.clone(),
        });
        out.top.push(Coedge {
            edge: et,
            fwd: !p.rev,
            pc: p.c.clone(),
        });
        let mut sl = side_loop(
            &side,
            (eb, et),
            !p.rev,
            (rail_e[i], rail_e[j]),
            (s0, s1),
            (p.ts(), p.te()),
        );
        if !up {
            sl.reverse();
            for c in &mut sl {
                c.fwd = !c.fwd;
            }
        }
        b.faces.push(Face {
            surf: side.surf,
            sense: true,
            loops: vec![sl],
        });
        out.sides.push((
            b.faces.len() - 1,
            std::iter::once(p.src)
                .chain(p.more.iter().copied())
                .collect(),
        ));
        if !p.more.is_empty() && et != eb {
            turned.push(et);
        }
    }
    // The far edge of a side fused from several curves runs back along the loop: OCCT's merging
    // of faces on one surface rebuilt such edges the way the side's boundary runs (a straight
    // edge picked as a direction then points the same way).
    for e in turned {
        turn_line_edge(b, e, &mut out.top);
    }
    out
}

/// Turns a line edge round (its curve, vertices and every coedge on it, `extra` too), its
/// p-curves lines; left as it is when one isn't.
fn turn_line_edge(b: &mut Body, e: usize, extra: &mut [Coedge]) {
    let flip = |c: &Curve, t: [f64; 2]| match c {
        Curve::Line { o, d } => Some(Curve::Line {
            o: o + d * (t[0] + t[1]),
            d: -d,
        }),
        _ => None,
    };
    let t = b.edges[e].t;
    let on_e = |c: &Coedge| c.edge == e;
    let all_lines = b
        .faces
        .iter()
        .flat_map(|f| f.loops.iter().flatten())
        .chain(extra.iter())
        .filter(|c| on_e(c))
        .all(|c| matches!(c.pc, Curve::Line { .. }));
    let Some(curve) = flip(&b.edges[e].curve, t) else {
        return;
    };
    if !all_lines {
        return;
    }
    let edge = &mut b.edges[e];
    edge.curve = curve;
    edge.v.swap(0, 1);
    let turn = |c: &mut Coedge| {
        if c.edge == e
            && let Some(pc) = flip(&c.pc, t)
        {
            c.pc = pc;
            c.fwd = !c.fwd;
        }
    };
    b.faces
        .iter_mut()
        .flat_map(|f| f.loops.iter_mut().flatten())
        .for_each(turn);
    extra.iter_mut().for_each(turn);
}

/// Sweeps closed faces (solids, or their boundaries as sheets when `solid` is false) and open
/// chains (sheets) from s0 to s1.
pub fn sweep_full(
    faces: &[MergedFace],
    chains: &[Vec<Piece>],
    kind: &dyn SweepKind,
    s0: f64,
    s1: f64,
    whole: bool,
    solid: bool,
) -> Result<Built> {
    if s1 - s0 < MIN_DEPTH {
        return Err(KernelError::InvalidParameter(format!(
            "The depth {:e} is below the modelling tolerance ({MIN_DEPTH:e} mm)",
            s1 - s0
        )));
    }
    let up = kind.up();
    let m0 = kind.place(s0);
    let m1 = kind.place(s1);
    let mut out = Built::default();
    let b = &mut out.body;
    let rev_loop = |l: &mut Vec<Coedge>| {
        l.reverse();
        for c in l.iter_mut() {
            c.fwd = !c.fwd;
        }
    };
    for mf in faces {
        let face_start = b.faces.len();
        let mut bottom_loops: Vec<Vec<Coedge>> = Vec::new();
        let mut top_loops: Vec<Vec<Coedge>> = Vec::new();
        let mut sides = Vec::new();
        for lp in std::iter::once(&mf.outer).chain(mf.holes.iter()) {
            let st = sweep_strand(b, lp, true, kind, s0, s1, whole, up, &m0, &m1);
            sides.extend(st.sides);
            bottom_loops.push(st.bottom);
            top_loops.push(st.top);
        }
        let n_sides = b.faces.len() - face_start;
        if solid && !whole {
            if up {
                bottom_loops.iter_mut().for_each(rev_loop);
            } else {
                top_loops.iter_mut().for_each(rev_loop);
            }
            // A cap's plane has a unit frame; a placement that scales the profile (a sweep's
            // law) scales its p-curves instead.
            let frame = |m: &Affine| {
                let k = m.scale();
                Frame {
                    o: m.point(&P3::origin()),
                    x: m.vector(&V3::x()) / k,
                    y: m.vector(&V3::y()) / k,
                    z: m.vector(&V3::z()) / k,
                }
            };
            let scaled = |loops: &mut Vec<Vec<Coedge>>, m: &Affine| {
                let k = m.scale();
                if (k - 1.0).abs() > 1e-15 {
                    let a = Affine {
                        m: nalgebra::Matrix3::from_diagonal(&V3::new(k, k, 1.0)),
                        t: V3::zeros(),
                    };
                    for c in loops.iter_mut().flatten() {
                        c.pc = c.pc.transformed(&a);
                    }
                }
            };
            scaled(&mut bottom_loops, &m0);
            scaled(&mut top_loops, &m1);
            b.faces.push(Face {
                surf: Surface::Plane { f: frame(&m0) },
                sense: !up,
                loops: bottom_loops,
            });
            let ib = b.faces.len() - 1;
            b.faces.push(Face {
                surf: Surface::Plane { f: frame(&m1) },
                sense: up,
                loops: top_loops,
            });
            let it = b.faces.len() - 1;
            for r in &mf.regions {
                out.origins.push((ib, Origin::StartCap { region: *r }));
                out.origins.push((it, Origin::EndCap { region: *r }));
            }
        }
        for (fi, srcs) in sides {
            for s in srcs {
                out.origins.push((fi, s.origin()));
            }
        }
        fix_senses(b, face_start..face_start + n_sides);
        // The caps join the loops' strands into one shell. Without them (a whole turn, a sheet)
        // each loop sweeps a surface of its own: a shell each, edge-connected as shells must
        // be, the outer loop's first (pieces come in face order; a hole's closed tube is a
        // void of the solid).
        let fs: Vec<usize> = (face_start..b.faces.len()).collect();
        let pieces = b.face_components(&fs);
        let first = b.shells.len();
        b.shells.extend(pieces.into_iter().map(|faces| Shell {
            faces,
            closed: solid,
        }));
        if solid {
            b.solids.push(Solid {
                shells: (first..b.shells.len()).collect(),
            });
        }
    }
    for ch in chains {
        let face_start = b.faces.len();
        let st = sweep_strand(b, ch, false, kind, s0, s1, whole, true, &m0, &m1);
        for (fi, srcs) in st.sides {
            for s in srcs {
                out.origins.push((fi, s.origin()));
            }
        }
        fix_senses(b, face_start..b.faces.len());
        b.shells.push(Shell {
            faces: (face_start..b.faces.len()).collect(),
            closed: false,
        });
    }
    out.origins.sort_by_key(|(f, _)| *f);
    Ok(out)
}
