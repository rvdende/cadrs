//! A cutter's shape as pieces of a revolved profile: the one source of geometry for every
//! cutter, the built-in kinds and the form tools alike.
//!
//! The profile is the cutter's right-hand outline in the (radius, height) half plane, from the
//! tip upward. Its height never decreases along the way, so the cutter is the solid
//! `{ r ≤ R(h) }`: that covers the shapes that undercut (a lollipop, a T-slot cutter, a
//! dovetail), where the radius shrinks again above the widest point, and keeps every ray test
//! against a cutter simple.
//!
//! - A [`FormProfile`] is what a form tool stores: points with a bulge each (the DXF
//!   convention: the tangent of a quarter of the arc's angle, positive for an arc that bulges
//!   out of the cutter, 0 for a straight line).
//! - A [`Piece`] is one stretch of a profile: a straight line (a cone, a cylinder or a flat
//!   disc once revolved) or a circular arc (a zone of a sphere when its centre is on the axis,
//!   else of a torus; a barrel's side is an arc whose centre lies across the axis).
//! - A [`Cutter`] is a tool's pieces in order ([`Cutter::of`]). Dropping a cutter along its
//!   axis only ever meets its **lower envelope** ([`Cutter::height_at`]): the pieces that face
//!   down or sideways. The simulation takes the cutter as a stack of **convex** solids
//!   ([`Cutter::convex`]): frusta and sphere zones, with the arcs off the axis cut into frusta
//!   within a tolerance.
//!
//! [`crate::tool::Tool::height_at`] keeps its closed forms for the kinds that had them and asks
//! the pieces for the rest; a property test below holds the two to the same answers.
//! [`cutter_mesh`] revolves a tool with its shank, holder and arbor into a mesh to draw.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde::{Deserialize, Serialize};

use crate::tool::{Tool, ToolKind};

/// How far an angle or a length may be off before a piece stops answering for it.
const TOL: f64 = 1e-9;

/// A point of a form tool's profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FormPoint {
    /// Distance from the axis (mm).
    pub r: f64,
    /// Height above the tip (mm).
    pub h: f64,
    /// The bulge of the segment arriving at this point: 0 for a straight line, else the tangent
    /// of a quarter of the arc's angle (positive: the arc bulges out of the cutter).
    #[serde(default)]
    pub bulge: f64,
}

/// A form tool's profile: its outline from the tip upward (see the module docs).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FormProfile {
    pub points: Vec<FormPoint>,
}

impl FormProfile {
    /// The most points a profile may have.
    pub const MAX_POINTS: usize = 64;

    /// A profile of straight segments through `[radius, height]` points.
    pub fn straight(points: &[[f64; 2]]) -> FormProfile {
        FormProfile {
            points: points
                .iter()
                .map(|p| FormPoint {
                    r: p[0],
                    h: p[1],
                    bulge: 0.0,
                })
                .collect(),
        }
    }

    /// A T-slot cutter: a disc of `diameter` and `thickness` on a neck of `neck_diameter`,
    /// `length` from the tip to the top of the neck.
    pub fn t_slot(diameter: f64, thickness: f64, neck_diameter: f64, length: f64) -> FormProfile {
        let (r, n) = (diameter / 2.0, neck_diameter / 2.0);
        FormProfile::straight(&[
            [r, 0.0],
            [r, thickness],
            [n, thickness],
            [n, length.max(thickness)],
        ])
    }

    /// Its widest radius (an arc may be wider than both its ends).
    pub fn max_radius(&self) -> f64 {
        let mut m = self.points.first().map_or(0.0, |p| p.r);
        self.each(|p| m = m.max(p.max_radius()));
        m
    }

    /// The height of its last point.
    pub fn top(&self) -> f64 {
        self.points.last().map_or(0.0, |p| p.h)
    }

    /// Each segment as a piece, from the tip upward. A segment whose arc breaks the rules
    /// ([`FormProfile::check`]) comes out as its chord.
    pub fn each(&self, mut f: impl FnMut(Piece)) {
        for w in self.points.windows(2) {
            f(segment(w[0], w[1]).unwrap_or(Piece::Cone {
                r0: w[0].r,
                h0: w[0].h,
                r1: w[1].r,
                h1: w[1].h.max(w[0].h),
            }));
        }
    }

    /// What is wrong with the profile, if anything: it must start at the tip, go upward only,
    /// stay on its side of the axis, and have no more than [`FormProfile::MAX_POINTS`] points.
    pub fn check(&self) -> Result<(), String> {
        let n = self.points.len();
        if n < 2 {
            return Err("The profile needs at least two points".into());
        }
        if n > Self::MAX_POINTS {
            return Err(format!(
                "The profile has {n} points, more than {}",
                Self::MAX_POINTS
            ));
        }
        if self
            .points
            .iter()
            .any(|p| !(p.r.is_finite() && p.h.is_finite() && p.bulge.is_finite()))
        {
            return Err("The profile has a point that is not a number".into());
        }
        if self.points.iter().any(|p| p.r < 0.0) {
            return Err("The profile must stay on one side of the tool's axis".into());
        }
        if self.points[0].h.abs() > 1e-9 {
            return Err("The profile must start at the tip (height 0)".into());
        }
        for (i, w) in self.points.windows(2).enumerate() {
            if w[1].h < w[0].h - 1e-9 {
                return Err(format!(
                    "The profile must go upward: point {} is under point {}",
                    i + 2,
                    i + 1
                ));
            }
            if (w[1].h - w[0].h).hypot(w[1].r - w[0].r) < 1e-9 {
                return Err(format!(
                    "Points {} and {} of the profile are in the same place",
                    i + 1,
                    i + 2
                ));
            }
            segment(w[0], w[1]).map_err(|e| format!("Point {} of the profile: {e}", i + 2))?;
        }
        if self.top() <= 1e-9 {
            return Err("The profile has no height".into());
        }
        if self.max_radius() <= 1e-9 {
            return Err("The profile has no width".into());
        }
        Ok(())
    }
}

/// The piece from `p0` to `p1` (by `p1`'s bulge), or why the arc can't be part of a cutter.
fn segment(p0: FormPoint, p1: FormPoint) -> Result<Piece, &'static str> {
    let b = p1.bulge;
    if b.abs() < 1e-12 {
        return Ok(Piece::Cone {
            r0: p0.r,
            h0: p0.h,
            r1: p1.r,
            h1: p1.h,
        });
    }
    let (cx, cy) = (p1.r - p0.r, p1.h - p0.h);
    let l = cx.hypot(cy);
    if l < 1e-12 {
        return Err("its arc has no length");
    }
    // The centre is off the chord's middle, to its left for a positive bulge.
    let k = (1.0 - b * b) / (4.0 * b);
    let (rc, hc) = ((p0.r + p1.r) / 2.0 - cy * k, (p0.h + p1.h) / 2.0 + cx * k);
    let rho = l * (1.0 + b * b) / (4.0 * b.abs());
    let theta = 4.0 * b.atan();
    let mut a0 = (p0.h - hc).atan2(p0.r - rc);
    // (Looser than `TOL`: a bulge typed to a few digits is that far off a quarter circle.)
    let slack = 1e-6;
    if b > 0.0 {
        // Bulging outward: within the right half of its circle, where the height rises.
        let a1 = a0 + theta;
        if a0 < -FRAC_PI_2 - slack || a1 > FRAC_PI_2 + slack {
            return Err("its arc turns back down");
        }
        Ok(Piece::Arc {
            rc,
            hc,
            rho,
            a0: a0.max(-FRAC_PI_2),
            a1: a1.min(FRAC_PI_2),
            convex: true,
        })
    } else {
        // Hollow: within the left half of its circle, run through from the bottom up.
        if a0 < 0.0 {
            a0 += TAU;
        }
        let a1 = a0 + theta;
        if a0 > 3.0 * FRAC_PI_2 + slack || a1 < FRAC_PI_2 - slack {
            return Err("its arc turns back down");
        }
        if a1 <= PI && a0 >= PI && rc - rho < -1e-9 {
            return Err("its arc crosses the tool's axis");
        }
        Ok(Piece::Arc {
            rc,
            hc,
            rho,
            a0: a0.min(3.0 * FRAC_PI_2),
            a1: a1.max(FRAC_PI_2),
            convex: false,
        })
    }
}

/// One stretch of a cutter's profile, revolved about the axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Piece {
    /// A straight line from `(r0, h0)` up to `(r1, h1)`: a cone, a cylinder when the radii are
    /// equal, a flat disc (or ring) when the heights are.
    Cone { r0: f64, h0: f64, r1: f64, h1: f64 },
    /// An arc of radius `rho` about `(rc, hc)`, its points at `(rc + rho cos a, hc + rho sin a)`
    /// for `a` from `a0` to `a1`. A convex arc bulges out of the cutter and runs within
    /// −90°…90° (`a0 < a1`); a hollow one runs within 270°…90° (`a0 > a1`). With `rc` 0 it is a
    /// zone of a sphere.
    Arc {
        rc: f64,
        hc: f64,
        rho: f64,
        a0: f64,
        a1: f64,
        convex: bool,
    },
}

impl Piece {
    /// Its lower end `[radius, height]`.
    pub fn start(&self) -> [f64; 2] {
        match *self {
            Piece::Cone { r0, h0, .. } => [r0, h0],
            Piece::Arc {
                rc, hc, rho, a0, ..
            } => [rc + rho * a0.cos(), hc + rho * a0.sin()],
        }
    }

    /// Its upper end `[radius, height]`.
    pub fn end(&self) -> [f64; 2] {
        match *self {
            Piece::Cone { r1, h1, .. } => [r1, h1],
            Piece::Arc {
                rc, hc, rho, a1, ..
            } => [rc + rho * a1.cos(), hc + rho * a1.sin()],
        }
    }

    /// Its widest radius.
    pub fn max_radius(&self) -> f64 {
        match *self {
            Piece::Cone { r0, r1, .. } => r0.max(r1),
            Piece::Arc {
                rc,
                rho,
                a0,
                a1,
                convex,
                ..
            } => {
                if convex && a0 <= 0.0 && a1 >= 0.0 {
                    rc + rho
                } else {
                    (rc + rho * a0.cos()).max(rc + rho * a1.cos())
                }
            }
        }
    }

    /// Whether any of it faces upward (its radius shrinking as it rises): the part of an
    /// undercutting cutter a drop along the axis never meets.
    pub fn faces_up(&self) -> bool {
        match *self {
            Piece::Cone { r0, r1, .. } => r1 < r0 - 1e-12,
            Piece::Arc { a0, a1, convex, .. } => {
                if convex {
                    a1 > TOL
                } else {
                    a0 > PI + TOL
                }
            }
        }
    }

    /// The lowest height of the piece at distance `r` from the axis (`None` where it has no
    /// point at that distance).
    pub fn height_at(&self, r: f64) -> Option<f64> {
        match *self {
            Piece::Cone { r0, h0, r1, h1 } => {
                let (lo, hi) = (r0.min(r1), r0.max(r1));
                if r < lo - 1e-12 || r > hi + 1e-12 {
                    None
                } else if hi - lo < 1e-12 {
                    Some(h0.min(h1))
                } else {
                    Some(h0 + (r.clamp(lo, hi) - r0) * (h1 - h0) / (r1 - r0))
                }
            }
            Piece::Arc {
                rc,
                hc,
                rho,
                a0,
                a1,
                convex,
            } => {
                let d = r - rc;
                let c = d / rho;
                if c.abs() > 1.0 + 1e-12 {
                    return None;
                }
                let a = c.clamp(-1.0, 1.0).acos();
                let rise = (rho * rho - d * d).max(0.0).sqrt();
                // The two points of the circle at this radius: the lower first.
                let (low, high, lo, hi) = if convex {
                    (-a, a, a0, a1)
                } else {
                    (TAU - a, a, a1, a0)
                };
                if low >= lo - TOL && low <= hi + TOL {
                    Some(hc - rise)
                } else if high >= lo - TOL && high <= hi + TOL {
                    Some(hc + rise)
                } else {
                    None
                }
            }
        }
    }

    /// The piece's radius at height `h` (`None` outside its heights; the wider end of a flat
    /// disc).
    pub fn radius_at(&self, h: f64) -> Option<f64> {
        match *self {
            Piece::Cone { r0, h0, r1, h1 } => {
                if h < h0 - 1e-12 || h > h1 + 1e-12 {
                    None
                } else if h1 - h0 < 1e-12 {
                    Some(r0.max(r1))
                } else {
                    Some(r0 + (h.clamp(h0, h1) - h0) * (r1 - r0) / (h1 - h0))
                }
            }
            Piece::Arc {
                rc,
                hc,
                rho,
                a0,
                a1,
                convex,
            } => {
                let d = h - hc;
                let s = d / rho;
                if s.abs() > 1.0 + 1e-12 {
                    return None;
                }
                let a = s.clamp(-1.0, 1.0).asin();
                let reach = (rho * rho - d * d).max(0.0).sqrt();
                if convex {
                    (a >= a0 - TOL && a <= a1 + TOL).then_some(rc + reach)
                } else {
                    let a = PI - a;
                    (a >= a1 - TOL && a <= a0 + TOL).then_some(rc - reach)
                }
            }
        }
    }
}

/// A convex solid of revolution about the cutter's axis: what the simulation sweeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Convex {
    /// The cone frustum from radius `r0` at height `h0` to `r1` at `h1` (`h0 < h1`).
    Frustum { r0: f64, h0: f64, r1: f64, h1: f64 },
    /// The ball of radius `rho` centred `hc` up the axis, between the heights `h0` and `h1`.
    Sphere { hc: f64, rho: f64, h0: f64, h1: f64 },
}

impl Convex {
    /// The heights it lies between.
    pub fn heights(&self) -> (f64, f64) {
        match *self {
            Convex::Frustum { h0, h1, .. } | Convex::Sphere { h0, h1, .. } => (h0, h1),
        }
    }

    /// Its widest radius.
    pub fn max_radius(&self) -> f64 {
        match *self {
            Convex::Frustum { r0, r1, .. } => r0.max(r1),
            Convex::Sphere { hc, rho, h0, h1 } => {
                if h0 <= hc && h1 >= hc {
                    rho
                } else {
                    let d = (h0 - hc).abs().min((h1 - hc).abs());
                    (rho * rho - d * d).max(0.0).sqrt()
                }
            }
        }
    }
}

/// A cutter's shape: its profile's pieces from the tip upward.
#[derive(Debug, Clone, PartialEq)]
pub struct Cutter {
    pub pieces: Vec<Piece>,
    /// Its widest radius.
    pub radius: f64,
    /// From the tip to the top of its last piece.
    pub length: f64,
    /// Whether it is narrower somewhere above its widest point (it can cut under an overhang).
    pub undercut: bool,
}

/// The pieces of `tool`'s cutting part, from the tip upward, without allocating (the drop-cutter
/// asks for heights many times over).
pub(crate) fn each_piece(tool: &Tool, mut f: impl FnMut(Piece)) {
    let rad = tool.radius();
    let len = tool.cutting_length;
    // A flat disc out to `r` at the tip, and the cylinder of radius `r` from `from` up to the
    // end of the flutes.
    let disc = |r: f64, f: &mut dyn FnMut(Piece)| {
        if r > 0.0 {
            f(Piece::Cone {
                r0: 0.0,
                h0: 0.0,
                r1: r,
                h1: 0.0,
            });
        }
    };
    let cylinder = |r: f64, from: f64, f: &mut dyn FnMut(Piece)| {
        if len > from {
            f(Piece::Cone {
                r0: r,
                h0: from,
                r1: r,
                h1: len,
            });
        }
    };
    let flat = |f: &mut dyn FnMut(Piece)| {
        disc(rad, f);
        cylinder(rad, 0.0, f);
    };
    match tool.kind {
        ToolKind::BallMill => {
            f(Piece::Arc {
                rc: 0.0,
                hc: rad,
                rho: rad,
                a0: -FRAC_PI_2,
                a1: 0.0,
                convex: true,
            });
            cylinder(rad, rad, &mut f);
        }
        ToolKind::BullMill => {
            let rc = tool.corner_radius.clamp(0.0, rad);
            let ring = rad - rc;
            disc(ring, &mut f);
            if rc > 0.0 {
                f(Piece::Arc {
                    rc: ring,
                    hc: rc,
                    rho: rc,
                    a0: -FRAC_PI_2,
                    a1: 0.0,
                    convex: true,
                });
            }
            cylinder(rad, rc, &mut f);
        }
        ToolKind::ChamferMill | ToolKind::TaperMill | ToolKind::Drill => {
            let tip = (tool.tip_diameter / 2.0).clamp(0.0, rad);
            let half = (tool.angle.clamp(1.0, 179.0) / 2.0).to_radians();
            disc(tip, &mut f);
            let top = (rad - tip) / half.tan();
            if tip < rad {
                f(Piece::Cone {
                    r0: tip,
                    h0: 0.0,
                    r1: rad,
                    h1: top,
                });
            }
            cylinder(rad, top, &mut f);
        }
        ToolKind::TaperBall => {
            // A ball of the tip's radius, then the cone tangent to it out to the diameter or
            // up to the end of the flutes, whichever comes first.
            let ball = (tool.tip_diameter / 2.0).clamp(1e-6, rad);
            let half = (tool.angle.clamp(0.0, 179.0) / 2.0).to_radians();
            let (r_t, h_t) = (ball * half.cos(), ball * (1.0 - half.sin()));
            f(Piece::Arc {
                rc: 0.0,
                hc: ball,
                rho: ball,
                a0: -FRAC_PI_2,
                a1: -half,
                convex: true,
            });
            let slope = half.tan();
            let reach = if slope > 1e-9 {
                h_t + (rad - r_t) / slope
            } else {
                f64::INFINITY
            };
            if reach <= len {
                if rad > r_t {
                    f(Piece::Cone {
                        r0: r_t,
                        h0: h_t,
                        r1: rad,
                        h1: reach,
                    });
                }
                cylinder(rad, reach, &mut f);
            } else if len > h_t {
                f(Piece::Cone {
                    r0: r_t,
                    h0: h_t,
                    r1: r_t + (len - h_t) * slope,
                    h1: len,
                });
            }
        }
        ToolKind::Lollipop => {
            // A ball up to where the neck leaves it, on its upper half.
            let neck = if tool.neck_diameter > 0.0 {
                (tool.neck_diameter / 2.0).min(rad)
            } else {
                rad
            };
            let a1 = (neck / rad).clamp(0.0, 1.0).acos();
            f(Piece::Arc {
                rc: 0.0,
                hc: rad,
                rho: rad,
                a0: -FRAC_PI_2,
                a1,
                convex: true,
            });
            cylinder(neck, rad + rad * a1.sin(), &mut f);
        }
        ToolKind::Barrel => match barrel(tool) {
            Some((rc, rho, half, bottom)) => {
                disc(bottom, &mut f);
                f(Piece::Arc {
                    rc,
                    hc: len / 2.0,
                    rho,
                    a0: -half,
                    a1: half,
                    convex: true,
                });
            }
            None => flat(&mut f),
        },
        ToolKind::Form => match &tool.form {
            Some(p) if p.points.len() >= 2 => {
                let (first, last) = (p.points[0], p.points[p.points.len() - 1]);
                if first.r > 0.0 {
                    f(Piece::Cone {
                        r0: 0.0,
                        h0: first.h,
                        r1: first.r,
                        h1: first.h,
                    });
                }
                p.each(&mut f);
                cylinder(last.r, last.h, &mut f);
            }
            _ => flat(&mut f),
        },
        ToolKind::EndMill
        | ToolKind::FaceMill
        | ToolKind::Tap
        | ToolKind::ThreadMill
        | ToolKind::Laser
        | ToolKind::Plasma
        | ToolKind::Waterjet => flat(&mut f),
    }
}

/// A barrel mill's side arc: its centre's distance from the axis (negative: across it), its
/// radius, half the angle it spans, and the radius of the flat bottom it starts from. `None`
/// when the sizes don't make a barrel (the arc too tight for the flutes, or closing before it
/// reaches the tip).
pub(crate) fn barrel(tool: &Tool) -> Option<(f64, f64, f64, f64)> {
    let (rad, rho, len) = (tool.radius(), tool.profile_radius, tool.cutting_length);
    if !(rho > 0.0 && len > 0.0 && rho >= len / 2.0) {
        return None;
    }
    let half = (len / 2.0 / rho).asin();
    let rc = rad - rho;
    let bottom = rc + rho * half.cos();
    (bottom >= 0.0).then_some((rc, rho, half, bottom))
}

impl Cutter {
    /// The shape of `tool`'s cutting part (a beam's is its kerf, as a cylinder).
    pub fn of(tool: &Tool) -> Cutter {
        let mut pieces = Vec::new();
        each_piece(tool, |p| pieces.push(p));
        Cutter::from_pieces(pieces)
    }

    /// A cutter of `pieces` (from the tip upward, each starting where the last ended).
    pub fn from_pieces(pieces: Vec<Piece>) -> Cutter {
        let radius = pieces.iter().map(Piece::max_radius).fold(0.0, f64::max);
        let length = pieces.iter().map(|p| p.end()[1]).fold(0.0, f64::max);
        let undercut = pieces.iter().any(Piece::faces_up);
        Cutter {
            pieces,
            radius,
            length,
            undercut,
        }
    }

    /// How high above the tip the cutter's lower envelope is at distance `r` from the axis:
    /// what a drop along the axis meets (`None` outside the cutter).
    pub fn height_at(&self, r: f64) -> Option<f64> {
        if r > self.radius + 1e-12 {
            return None;
        }
        let r = r.min(self.radius);
        self.pieces
            .iter()
            .filter_map(|p| p.height_at(r))
            .reduce(f64::min)
    }

    /// The cutter's radius at height `h` above the tip (the wider one where it steps; its top
    /// radius above the top).
    pub fn radius_at_height(&self, h: f64) -> f64 {
        let h = h.clamp(0.0, self.length);
        self.pieces
            .iter()
            .filter_map(|p| p.radius_at(h))
            .fold(0.0, f64::max)
    }

    /// The radius at its top, where the shank goes on.
    pub fn top_radius(&self) -> f64 {
        self.pieces.last().map_or(0.0, |p| p.end()[0])
    }

    /// The profile as a polyline of `[radius, height]` from the axis at the tip to the top,
    /// its arcs within `chord` of the true ones.
    pub fn outline(&self, chord: f64) -> Vec<[f64; 2]> {
        let mut out: Vec<[f64; 2]> = Vec::new();
        let push = |p: [f64; 2], out: &mut Vec<[f64; 2]>| {
            if out
                .last()
                .is_none_or(|l| (l[0] - p[0]).hypot(l[1] - p[1]) > 1e-9)
            {
                out.push(p);
            }
        };
        if let Some(first) = self.pieces.first() {
            let s = first.start();
            push([0.0, s[1]], &mut out);
        }
        for p in &self.pieces {
            push(p.start(), &mut out);
            if let Piece::Arc {
                rc,
                hc,
                rho,
                a0,
                a1,
                ..
            } = *p
            {
                let n = arc_steps(rho, (a1 - a0).abs(), chord);
                for i in 1..n {
                    let a = a0 + (a1 - a0) * i as f64 / n as f64;
                    push([rc + rho * a.cos(), hc + rho * a.sin()], &mut out);
                }
            }
            push(p.end(), &mut out);
        }
        out
    }

    /// The cutter as convex solids stacked up its axis, for the simulation: a frustum for each
    /// straight piece, a zone of a ball for each arc centred on the axis, and frusta within
    /// `eps` of the arc for each other arc (chords: inside a convex arc, outside a hollow one).
    /// Flat discs have no volume and are left out.
    pub fn convex(&self, eps: f64) -> Vec<Convex> {
        let mut out = Vec::new();
        let frustum = |a: [f64; 2], b: [f64; 2], out: &mut Vec<Convex>| {
            if b[1] - a[1] > 1e-9 && a[0].max(b[0]) > 0.0 {
                out.push(Convex::Frustum {
                    r0: a[0].max(0.0),
                    h0: a[1],
                    r1: b[0].max(0.0),
                    h1: b[1],
                });
            }
        };
        for p in &self.pieces {
            match *p {
                Piece::Cone { .. } => frustum(p.start(), p.end(), &mut out),
                Piece::Arc {
                    rc,
                    hc,
                    rho,
                    a0,
                    a1,
                    convex,
                } => {
                    if convex && rc.abs() < 1e-12 {
                        let (h0, h1) = (hc + rho * a0.sin(), hc + rho * a1.sin());
                        if h1 - h0 > 1e-9 {
                            out.push(Convex::Sphere { hc, rho, h0, h1 });
                        }
                        continue;
                    }
                    let n = arc_steps(rho, (a1 - a0).abs(), eps);
                    let at = |i: usize| {
                        let a = a0 + (a1 - a0) * i as f64 / n as f64;
                        [rc + rho * a.cos(), hc + rho * a.sin()]
                    };
                    for i in 0..n {
                        frustum(at(i), at(i + 1), &mut out);
                    }
                }
            }
        }
        out
    }
}

/// How many chords an arc of `angle` (radians) and radius `rho` needs to stay within `chord`.
fn arc_steps(rho: f64, angle: f64, chord: f64) -> usize {
    let chord = chord.max(1e-6);
    if rho <= chord {
        return 1;
    }
    let step = 2.0 * (1.0 - chord / rho).acos();
    ((angle / step).ceil() as usize).clamp(1, 4096)
}

/// The colours [`cutter_mesh`] gives the flutes, the shank, the holder and the arbor.
pub const FLUTE_COLOR: [f32; 4] = [0.80, 0.82, 0.86, 1.0];
pub const SHANK_COLOR: [f32; 4] = [0.55, 0.57, 0.60, 1.0];
pub const HOLDER_COLOR: [f32; 4] = [0.30, 0.32, 0.36, 1.0];
pub const ARBOR_COLOR: [f32; 4] = [0.22, 0.24, 0.28, 1.0];

/// A triangle mesh of a tool, for drawing it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub colors: Vec<[f32; 4]>,
}

/// The tool as a closed mesh: its flutes, shank, holder and arbor revolved in `segments` steps
/// round the axis, each in its own colour, the tip at the origin and the axis along +Z. Every
/// band of the outline has its own vertices (so a corner stays sharp and a colour doesn't
/// bleed); a beam is drawn as its kerf.
pub fn cutter_mesh(tool: &Tool, segments: usize) -> ToolMesh {
    let segments = segments.clamp(3, 512);
    let cutter = Cutter::of(tool);
    // The outline, band by band: (from, to, colour).
    let chord = (cutter.radius * 0.01).clamp(0.002, 0.05);
    type Band = ([f64; 2], [f64; 2], [f32; 4]);
    let mut bands: Vec<Band> = Vec::new();
    let mut at = [0.0, 0.0];
    let go = |to: [f64; 2], color: [f32; 4], at: &mut [f64; 2], bands: &mut Vec<Band>| {
        if (to[0] - at[0]).hypot(to[1] - at[1]) > 1e-9 {
            bands.push((*at, to, color));
            *at = to;
        }
    };
    for p in cutter.outline(chord) {
        go(p, FLUTE_COLOR, &mut at, &mut bands);
    }
    if !tool.is_beam() {
        let (shank, from) = tool.shank();
        if tool.overall_length > from {
            go([shank, at[1]], SHANK_COLOR, &mut at, &mut bands);
            go(
                [shank, tool.overall_length],
                SHANK_COLOR,
                &mut at,
                &mut bands,
            );
        }
        let mut base = at[1].max(tool.overall_length);
        for (profile, color) in [(&tool.holder, HOLDER_COLOR), (&tool.arbor, ARBOR_COLOR)] {
            let Some(profile) = profile else { continue };
            for p in &profile.points {
                go(
                    [p[0].max(0.0), base + p[1].max(0.0)],
                    color,
                    &mut at,
                    &mut bands,
                );
            }
            base += profile.length();
        }
    }
    // Close the top onto the axis, in the colour of what it closes.
    let last = bands.last().map_or(FLUTE_COLOR, |b| b.2);
    go([0.0, at[1]], last, &mut at, &mut bands);

    let mut mesh = ToolMesh::default();
    for (a, b, color) in bands {
        // Outward is to the right of the way up the outline.
        let (dr, dh) = (b[0] - a[0], b[1] - a[1]);
        let l = dr.hypot(dh);
        let (nr, nh) = (dh / l, -dr / l);
        let base = mesh.positions.len() as u32;
        for end in [a, b] {
            for k in 0..segments {
                let t = TAU * k as f64 / segments as f64;
                let (c, s) = (t.cos(), t.sin());
                mesh.positions
                    .push([(end[0] * c) as f32, (end[0] * s) as f32, end[1] as f32]);
                mesh.normals
                    .push([(nr * c) as f32, (nr * s) as f32, nh as f32]);
                mesh.colors.push(color);
            }
        }
        let n = segments as u32;
        for k in 0..n {
            let k1 = (k + 1) % n;
            let (a0, a1, b0, b1) = (base + k, base + k1, base + n + k, base + n + k1);
            // A band that starts or ends on the axis is a fan of triangles.
            if a[0] > 1e-9 {
                mesh.indices.extend([a0, a1, b1]);
            }
            if b[0] > 1e-9 {
                mesh.indices.extend([a0, b1, b0]);
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dropcutter::{TriIndex, drop_point};
    use crate::geom::P3;
    use proptest::prelude::*;

    /// A quarter circle's bulge.
    const QUARTER: f64 = 0.414_213_562_373_095_05;

    fn form(points: &[[f64; 3]]) -> Tool {
        let mut t = Tool::new(ToolKind::Form, 1.0);
        t.set_form(FormProfile {
            points: points
                .iter()
                .map(|p| FormPoint {
                    r: p[0],
                    h: p[1],
                    bulge: p[2],
                })
                .collect(),
        });
        t.check().unwrap();
        t
    }

    /// The old kinds with sizes drawn at random.
    fn old_tool() -> impl Strategy<Value = Tool> {
        (
            0usize..7,
            0.2f64..40.0,
            0.0f64..1.0,
            5.0f64..175.0,
            0.0f64..0.95,
            0.1f64..80.0,
        )
            .prop_map(|(kind, d, corner, angle, tip, len)| {
                let kind = [
                    ToolKind::EndMill,
                    ToolKind::BallMill,
                    ToolKind::BullMill,
                    ToolKind::ChamferMill,
                    ToolKind::TaperMill,
                    ToolKind::FaceMill,
                    ToolKind::Drill,
                ][kind];
                let mut t = Tool::new(kind, d);
                t.corner_radius = corner * d / 2.0;
                t.angle = angle;
                t.tip_diameter = tip * d;
                t.cutting_length = len;
                t
            })
    }

    proptest! {
        /// The pieces say what the closed forms say, for the kinds that have closed forms.
        #[test]
        fn pieces_match_the_closed_forms(t in old_tool(), f in 0.0f64..1.02, g in 0.0f64..1.5) {
            let c = Cutter::of(&t);
            let r = f * t.radius();
            match (t.height_at(r), c.height_at(r)) {
                (Some(a), Some(b)) => prop_assert!((a - b).abs() < 1e-9, "{a} {b} at {r}"),
                (None, None) => {}
                (a, b) => prop_assert!(false, "{a:?} {b:?} at {r}"),
            }
            prop_assert!((c.radius - t.radius()).abs() < 1e-9);
            // (A bull mill's closed form calls its radius the full one in its corner too.)
            let h = g * c.length;
            if t.kind != ToolKind::BullMill || h >= t.corner_radius {
                let (a, b) = (t.radius_at_height(h), c.radius_at_height(h));
                prop_assert!((a - b).abs() < 1e-9, "{a} {b} at {h}");
            }
        }
    }

    /// A deterministic scatter of triangles over a patch, some steep.
    fn scatter(seed: u64, count: usize) -> Vec<[P3; 3]> {
        let mut s = seed;
        let mut next = move || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 11) as f64) / ((1u64 << 53) as f64)
        };
        (0..count)
            .map(|_| {
                let c = [next() * 40.0, next() * 40.0, next() * 10.0];
                let mut tri = [[0.0; 3]; 3];
                for p in &mut tri {
                    *p = [
                        c[0] + (next() - 0.5) * 12.0,
                        c[1] + (next() - 0.5) * 12.0,
                        c[2] + (next() - 0.5) * 8.0,
                    ];
                }
                // Facing up, as a model's top does.
                let n = crate::geom::cross(
                    crate::geom::sub(tri[1], tri[0]),
                    crate::geom::sub(tri[2], tri[0]),
                );
                if n[2] < 0.0 {
                    tri.swap(1, 2);
                }
                tri
            })
            .collect()
    }

    /// A form tool drawn as a ball, a bull-nose or a chamfer mill drops exactly where the
    /// built-in kind does.
    #[test]
    fn a_form_like_a_built_in_kind_drops_the_same() {
        let ball = Tool::new(ToolKind::BallMill, 6.0);
        let ball_form = form(&[[0.0, 0.0, 0.0], [3.0, 3.0, QUARTER], [3.0, 18.0, 0.0]]);
        let mut bull = Tool::new(ToolKind::BullMill, 10.0);
        bull.corner_radius = 1.5;
        let bull_form = form(&[
            [0.0, 0.0, 0.0],
            [3.5, 0.0, 0.0],
            [5.0, 1.5, QUARTER],
            [5.0, 30.0, 0.0],
        ]);
        let mut v = Tool::new(ToolKind::ChamferMill, 8.0);
        v.tip_diameter = 1.0;
        let v_form = form(&[
            [0.0, 0.0, 0.0],
            [0.5, 0.0, 0.0],
            [4.0, 3.5, 0.0],
            [4.0, 24.0, 0.0],
        ]);
        let tris = scatter(7, 400);
        let idx = TriIndex::new(&tris);
        let mut scratch = Vec::new();
        for (built_in, formed) in [(&ball, &ball_form), (&bull, &bull_form), (&v, &v_form)] {
            assert!((formed.diameter - built_in.diameter).abs() < 1e-12);
            let mut touched = 0;
            for i in 0..40 {
                for j in 0..40 {
                    let p = [i as f64 + 0.37, j as f64 + 0.11];
                    let a = drop_point(built_in, &idx, p, -50.0, &mut scratch);
                    let b = drop_point(formed, &idx, p, -50.0, &mut scratch);
                    assert!(
                        (a - b).abs() < 1e-9,
                        "{:?} at {p:?}: {a} {b}",
                        built_in.kind
                    );
                    touched += usize::from(a > -50.0);
                }
            }
            assert!(touched > 1000, "{touched}");
        }
    }

    /// On a big sloped plane the drop is the profile's highest contact: every lower-envelope
    /// point of the outline `(ρ, h)` would touch with the tip at `plane(p − ρ·u) − h`. A
    /// lollipop drops as the ball it is underneath.
    #[test]
    fn new_kinds_drop_onto_slopes_by_their_envelope() {
        let mut taper = Tool::new(ToolKind::TaperBall, 8.0);
        taper.tip_diameter = 2.0;
        taper.angle = 30.0;
        taper.cutting_length = 30.0;
        let mut barrel = Tool::new(ToolKind::Barrel, 10.0);
        barrel.profile_radius = 30.0;
        barrel.cutting_length = 14.0;
        let mut lollipop = Tool::new(ToolKind::Lollipop, 8.0);
        lollipop.neck_diameter = 3.0;
        lollipop.cutting_length = 20.0;
        let rounding = form(&[
            [2.0, 0.0, 0.0],
            [4.0, 2.0, -QUARTER],
            [5.0, 2.0, 0.0],
            [5.0, 12.0, 0.0],
        ]);
        let mut scratch = Vec::new();
        for slope in [0.0f64, 0.05, 0.3, 1.0, 2.5, 8.0] {
            // z = slope · x, as two big triangles.
            let s = 200.0;
            let tris = vec![
                [[-s, -s, -s * slope], [s, -s, s * slope], [s, s, s * slope]],
                [[-s, -s, -s * slope], [s, s, s * slope], [-s, s, -s * slope]],
            ];
            let idx = TriIndex::new(&tris);
            for tool in [&taper, &barrel, &lollipop, &rounding] {
                let c = Cutter::of(tool);
                // The plane rises along +x, so the contact is on the +x side of the axis.
                let mut want = f64::NEG_INFINITY;
                let outline = c.outline(1e-7);
                for w in outline.windows(2) {
                    for i in 0..=200 {
                        let f = i as f64 / 200.0;
                        let (r, h) = (
                            w[0][0] + (w[1][0] - w[0][0]) * f,
                            w[0][1] + (w[1][1] - w[0][1]) * f,
                        );
                        want = want.max(slope * (1.0 + r) - h);
                    }
                }
                let z = drop_point(tool, &idx, [1.0, 2.0], -500.0, &mut scratch);
                assert!(
                    (z - want).abs() < 2e-5,
                    "{:?} {slope}: {z} {want}",
                    tool.kind
                );
            }
            let ball = Tool::new(ToolKind::BallMill, 8.0);
            let (a, b) = (
                drop_point(&ball, &idx, [1.0, 2.0], -500.0, &mut scratch),
                drop_point(&lollipop, &idx, [1.0, 2.0], -500.0, &mut scratch),
            );
            assert!((a - b).abs() < 1e-9, "{slope}: {a} {b}");
        }
    }

    /// A lollipop and a T-slot cutter: the lower envelope is what a drop meets, the radius at
    /// a height is the solid's.
    #[test]
    fn undercutting_shapes() {
        // A Ø10 ball on a Ø4 neck.
        let mut l = Tool::new(ToolKind::Lollipop, 10.0);
        l.neck_diameter = 4.0;
        l.cutting_length = 30.0;
        l.check().unwrap();
        let c = Cutter::of(&l);
        assert!(c.undercut);
        assert!((c.radius - 5.0).abs() < 1e-12 && (c.length - 30.0).abs() < 1e-12);
        // Underneath it is a ball, the neck hidden above it.
        for r in [0.0, 1.0, 2.0, 3.0, 4.9, 5.0] {
            let want = 5.0 - (25.0f64 - r * r).sqrt();
            assert!((l.height_at(r).unwrap() - want).abs() < 1e-9, "{r}");
        }
        assert_eq!(l.height_at(5.01), None);
        // Widest at the ball's middle, the neck's radius from where it leaves the ball.
        let join = 5.0 + 21f64.sqrt();
        assert!((l.radius_at_height(5.0) - 5.0).abs() < 1e-9);
        assert!((l.radius_at_height(8.0) - 4.0).abs() < 1e-9);
        assert!((l.radius_at_height(join) - 2.0).abs() < 1e-6);
        assert!((l.radius_at_height(20.0) - 2.0).abs() < 1e-9);
        assert_eq!(l.shank(), (2.0, 30.0));

        // A Ø20 T-slot cutter 4 thick on a Ø8 neck.
        let mut t = Tool::new(ToolKind::Form, 1.0);
        t.set_form(FormProfile::t_slot(20.0, 4.0, 8.0, 25.0));
        t.check().unwrap();
        assert_eq!(t.diameter, 20.0);
        let c = Cutter::of(&t);
        assert!(c.undercut);
        for r in [0.0, 3.0, 4.0, 7.0, 10.0] {
            assert_eq!(t.height_at(r), Some(0.0), "{r}");
        }
        assert_eq!(t.height_at(10.1), None);
        assert_eq!(t.radius_at_height(2.0), 10.0);
        assert_eq!(t.radius_at_height(4.0), 10.0);
        assert_eq!(t.radius_at_height(4.5), 4.0);
        assert_eq!(t.radius_at_height(40.0), 4.0);
        // The simulation's solids: the disc and the neck.
        assert_eq!(
            c.convex(0.01),
            vec![
                Convex::Frustum {
                    r0: 10.0,
                    h0: 0.0,
                    r1: 10.0,
                    h1: 4.0
                },
                Convex::Frustum {
                    r0: 4.0,
                    h0: 4.0,
                    r1: 4.0,
                    h1: 25.0
                },
            ]
        );
        // A dovetail (60° flanks) is wider at the bottom: its envelope is flat.
        let d = form(&[
            [10.0, 0.0, 0.0],
            [10.0 - 6.0 / 60f64.to_radians().tan(), 6.0, 0.0],
            [3.0, 6.0, 0.0],
            [3.0, 20.0, 0.0],
        ]);
        assert_eq!(d.height_at(9.0), Some(0.0));
        assert!((d.radius_at_height(3.0) - (10.0 - 3.0 / 60f64.to_radians().tan())).abs() < 1e-9);
    }

    #[test]
    fn taper_ball_and_barrel() {
        // R1 ball, 10° a side, out to Ø8.
        let mut t = Tool::new(ToolKind::TaperBall, 8.0);
        t.tip_diameter = 2.0;
        t.angle = 20.0;
        t.cutting_length = 30.0;
        t.check().unwrap();
        let a = 10f64.to_radians();
        // On the ball, at the tangent ring, on the cone.
        assert!((t.height_at(0.5).unwrap() - (1.0 - 0.75f64.sqrt())).abs() < 1e-9);
        let (rt, ht) = (a.cos(), 1.0 - a.sin());
        assert!((t.height_at(rt).unwrap() - ht).abs() < 1e-9);
        assert!((t.height_at(2.0).unwrap() - (ht + (2.0 - rt) / a.tan())).abs() < 1e-9);
        assert!((t.radius_at_height(10.0) - (rt + (10.0 - ht) * a.tan())).abs() < 1e-9);
        assert!((t.radius_at_height(29.0) - 4.0).abs() < 1e-9);
        // Flutes that end before the taper reaches the shank: the cutter stops narrower.
        t.cutting_length = 10.0;
        let c = Cutter::of(&t);
        assert!((c.radius - (rt + (10.0 - ht) * a.tan())).abs() < 1e-9);
        assert_eq!(t.height_at(3.9), None);
        assert_eq!(t.shank(), (4.0, 10.0));

        // A Ø10 barrel, side radius 40, flutes 12.
        let mut b = Tool::new(ToolKind::Barrel, 10.0);
        b.profile_radius = 40.0;
        b.cutting_length = 12.0;
        b.check().unwrap();
        let bottom = -35.0 + (1600.0f64 - 36.0).sqrt();
        assert_eq!(b.height_at(bottom - 0.1), Some(0.0));
        assert!((b.radius_at_height(6.0) - 5.0).abs() < 1e-9);
        assert!((b.radius_at_height(0.0) - bottom).abs() < 1e-9);
        assert!((b.radius_at_height(12.0) - bottom).abs() < 1e-9);
        // Its side as frusta within the tolerance of the arc.
        let c = Cutter::of(&b);
        for k in c.convex(0.002) {
            let Convex::Frustum { r0, h0, r1, h1 } = k else {
                panic!("a barrel has no ball");
            };
            let (rm, hm) = ((r0 + r1) / 2.0, (h0 + h1) / 2.0);
            let off = 40.0 - (rm + 35.0).hypot(hm - 6.0);
            assert!((-1e-9..=0.002 + 1e-9).contains(&off), "{off}");
        }
        b.profile_radius = 5.0;
        assert!(b.check().is_err());
    }

    #[test]
    fn bad_profiles_are_rejected() {
        let check = |pts: &[[f64; 3]]| {
            let mut t = Tool::new(ToolKind::Form, 1.0);
            t.set_form(FormProfile {
                points: pts
                    .iter()
                    .map(|p| FormPoint {
                        r: p[0],
                        h: p[1],
                        bulge: p[2],
                    })
                    .collect(),
            });
            t.check()
        };
        assert!(check(&[[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 10.0, 0.0]]).is_ok());
        // One point; going back down; across the axis; not starting at the tip.
        assert!(check(&[[3.0, 0.0, 0.0]]).is_err());
        assert!(check(&[[3.0, 0.0, 0.0], [3.0, 5.0, 0.0], [2.0, 4.0, 0.0]]).is_err());
        assert!(check(&[[3.0, 0.0, 0.0], [-1.0, 5.0, 0.0]]).is_err());
        assert!(check(&[[3.0, 1.0, 0.0], [3.0, 5.0, 0.0]]).is_err());
        // An arc of more than a half turn, and one that dips under its start.
        assert!(check(&[[0.0, 0.0, 0.0], [0.0, 6.0, 1.5]]).is_err());
        assert!(check(&[[3.0, 0.0, 0.0], [5.0, 0.5, 1.0]]).is_err());
        // A hollow arc across the axis.
        assert!(check(&[[1.0, 0.0, 0.0], [1.0, 4.0, -1.0]]).is_err());
        // Too many points; a number that isn't one.
        let many: Vec<[f64; 3]> = (0..70).map(|i| [3.0, i as f64, 0.0]).collect();
        assert!(check(&many).is_err());
        assert!(check(&[[0.0, 0.0, 0.0], [f64::NAN, 2.0, 0.0]]).is_err());
        // A form tool without a profile; one whose diameter isn't its profile's.
        let mut t = Tool::new(ToolKind::Form, 6.0);
        t.form = None;
        assert!(t.check().is_err());
        let mut t = Tool::new(ToolKind::Form, 6.0);
        t.diameter = 9.0;
        assert!(t.check().is_err());
        // A hollow (corner-rounding) arc is fine, and the envelope follows it.
        let t = form(&[[2.0, 0.0, 0.0], [4.0, 2.0, -QUARTER], [4.0, 12.0, 0.0]]);
        let h = t.height_at(3.0).unwrap();
        assert!((h - 3f64.sqrt()).abs() < 1e-9, "{h}");
    }

    /// The mesh of every kind is closed, wound outward and within the tool's radius and length.
    #[test]
    fn the_mesh_is_closed_and_within_the_tool() {
        for kind in ToolKind::ALL {
            let mut tool = Tool::new(kind, 8.0);
            tool.holder = Some(crate::tool::Profile {
                name: "h".into(),
                points: vec![[10.0, 0.0], [10.0, 20.0], [16.0, 28.0], [16.0, 40.0]],
            });
            tool.arbor = Some(crate::tool::Profile::cylinder("a", 40.0, 15.0));
            let m = cutter_mesh(&tool, 24);
            assert_eq!(m.positions.len(), m.normals.len());
            assert_eq!(m.positions.len(), m.colors.len());
            assert!(!m.indices.is_empty() && m.indices.len() % 3 == 0);
            let reach = if tool.is_beam() { 4.0 } else { 20.0 };
            let top = if tool.is_beam() {
                tool.cutting_length
            } else {
                tool.gauge_length()
            };
            for p in &m.positions {
                let r = (p[0] as f64).hypot(p[1] as f64);
                assert!(r <= reach + 1e-4, "{kind:?}: {r}");
                assert!(
                    p[2] as f64 >= -1e-6 && p[2] as f64 <= top + 1e-4,
                    "{kind:?}"
                );
            }
            // Welded by position, every edge is used once each way.
            let key = |i: u32| {
                let p = m.positions[i as usize];
                [0, 1, 2].map(|k| (p[k] as f64 * 1e4).round() as i64)
            };
            let mut edges = std::collections::HashMap::new();
            let mut volume = 0.0;
            for t in m.indices.chunks(3) {
                let k = [key(t[0]), key(t[1]), key(t[2])];
                assert!(k[0] != k[1] && k[1] != k[2] && k[2] != k[0], "{kind:?}");
                for e in 0..3 {
                    *edges.entry((k[e], k[(e + 1) % 3])).or_insert(0) += 1;
                }
                let p = [0, 1, 2].map(|i| m.positions[t[i] as usize].map(|v| v as f64));
                volume += crate::geom::dot(p[0], crate::geom::cross(p[1], p[2])) / 6.0;
            }
            for ((a, b), n) in &edges {
                assert_eq!(*n, 1, "{kind:?}");
                assert_eq!(edges.get(&(*b, *a)), Some(&1), "{kind:?}");
            }
            assert!(volume > 0.0, "{kind:?}: {volume}");
        }
        // The colours follow the parts, from the tip up.
        let mut tool = Tool::new(ToolKind::EndMill, 6.0);
        tool.holder = Some(crate::tool::Profile::cylinder("h", 20.0, 30.0));
        let m = cutter_mesh(&tool, 12);
        let at = |z: f32, r: f32| {
            (0..m.positions.len())
                .find(|&i| {
                    let p = m.positions[i];
                    (p[2] - z).abs() < 1e-3 && (p[0].hypot(p[1]) - r).abs() < 1e-3
                })
                .map(|i| m.colors[i])
        };
        assert_eq!(at(0.0, 3.0), Some(FLUTE_COLOR));
        assert_eq!(at(tool.overall_length as f32, 3.0), Some(SHANK_COLOR));
        assert_eq!(
            at(tool.overall_length as f32 + 30.0, 10.0),
            Some(HOLDER_COLOR)
        );
    }
}
