//! The kernel's tolerance model: the distances under which two things are taken as one, named
//! once here and carried explicitly (in [`Tolerances`], held by the kernel session) instead of
//! as literals and globals.
//!
//! Lengths are in mm. Three levels, each ten times the one below:
//! - [`CONFUSION`] (1e-7): exact geometry. Two points closer are the same point; an entity's own
//!   tolerance ([`super::topo::Vertex::tol`], [`super::topo::Edge::tol`]) never goes below it
//!   (OCCT's `Precision::Confusion`).
//! - [`point`] (1e-6, [`POINT_DEFAULT`]; a session may set another): what an operation merges. A boolean pools vertices closer than this and
//!   puts edge ends on edges within it; intersections computed numerically land within it. A
//!   body's [`super::topo::Body::check`] accepts an edge end or a p-curve this far off when the
//!   entity's own tolerance is smaller.
//! - [`LOOP_GAP`] (1e-5): a p-curve fitted to an edge may miss the next coedge's start by this
//!   much (in model space, through the surface's metric), which `check` allows at a vertex of
//!   smaller tolerance.
//!
//! Above these, entities carry their own tolerance: a read file's or a healed body's vertices
//! and edges hold the gaps actually there ([`super::topo::Body::fit_tolerances`], as OCCT), and
//! operations take the larger of their own and the entities'.
//!
//! Models lie within [`SIZE`] of the origin: beyond it a double no longer resolves
//! [`CONFUSION`] (2⁻⁵² · 1e8 ≈ 2e-8 mm). Readers refuse files with geometry outside.
//!
//! A boolean whose first run leaves an open shell runs again at the [`Tolerances::retries`]
//! (1e-5, then 1e-4), as OCCT grows its tolerances; the result's entities record the tolerance
//! the run used.
//!
//! Below these, the round-off margins of exact computations are named by meaning, so a reader
//! sees which comparison is relative and which is absolute (all 1e-9 today, which keeps every
//! result as it was when they were literals):
//! - [`REL`]: relative, always scaled by what it compares (`REL * size`, `REL * (1.0 + |t|)`,
//!   a fraction of a span, `r * (1.0 - REL)`, a parameter fraction in [0, 1]);
//! - [`ANGULAR`]: directions and angles: unit vectors parallel (`1 − cos`, `|sin|`, `|u × v|`) or
//!   square (`|u · v|`), angles and spans in radians (`TAU - ANGULAR`);
//! - [`ROUNDOFF`]: absolute, for values of order one: lengths (mm) and parameters of exactly
//!   built geometry, far below [`CONFUSION`], and floors on sizes;
//! - [`TINY`]: the guard of a division or a normalisation against an exact zero, far below any
//!   meaningful value.
//!
//! A thousandth of those (1e-12), for comparisons of values computed exactly (closed forms,
//! knots, parameters of analytic curves) where 1e-9 would merge distinct ones:
//! - [`REL_FINE`], [`ANGULAR_FINE`] and [`ROUNDOFF_FINE`]: as [`REL`], [`ANGULAR`] and
//!   [`ROUNDOFF`];
//! - [`DEGENERATE`]: a computed length, norm or denominator below which it is taken as zero (a
//!   vector with no direction, `try_normalize(DEGENERATE)`; a span or a denominator of nothing):
//!   values that cancel leave round-off of this order, far above [`TINY`].
//!
//! At the merge level (1e-6, [`POINT_DEFAULT`]'s), for what an operation takes as the same:
//! - [`point`]: absolute distances (mm);
//! - [`REL_POINT`]: a fraction of the size, span or parameter range it multiplies;
//! - [`ANGULAR_POINT`]: directions taken as parallel or square, angles (radians) taken as 0 or π;
//! - [`DIFF_STEP`]: the step of a finite difference, or of a nudge off a point into a face or
//!   along a curve (a parameter step, or a fraction of the span it multiplies).

/// Exact geometry: points closer than this are one point (mm). See the module docs.
pub const CONFUSION: f64 = 1e-7;
/// The default merge distance of an operation (mm): [`point`] unless a session sets another.
/// See the module docs.
pub const POINT_DEFAULT: f64 = 1e-6;

/// The merge distance set for the process ([`set_point`]); 0 for none.
static POINT_BITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The merge distance of an operation (mm): [`POINT_DEFAULT`], or the one the kernel session
/// set ([`super::NativeKernel::set_tolerances`], e.g. [`Tolerances::parasolid`] to merge as
/// Parasolid does). One value for the process: every session and the kernel's worker threads
/// share it. See the module docs.
#[inline]
pub fn point() -> f64 {
    match POINT_BITS.load(std::sync::atomic::Ordering::Relaxed) {
        0 => POINT_DEFAULT,
        b => f64::from_bits(b),
    }
}

/// Sets [`point`] for the process (the default again for [`POINT_DEFAULT`]).
pub(crate) fn set_point(p: f64) {
    let b = if p == POINT_DEFAULT { 0 } else { p.to_bits() };
    POINT_BITS.store(b, std::sync::atomic::Ordering::Relaxed);
}
/// The largest gap between consecutive p-curves of a loop that is still closed (mm).
pub const LOOP_GAP: f64 = 1e-5;
/// The size box: coordinates and radii of a model stay within ±`SIZE` (mm, 100 km).
pub const SIZE: f64 = 1e8;

/// The relative round-off margin: a fraction of the size, span or value it is multiplied by.
/// See the module docs.
pub const REL: f64 = 1e-9;
/// The angular round-off margin (radians, or the dimensionless amount it makes for unit
/// vectors: `1 − cos`, `|sin|`). See the module docs.
pub const ANGULAR: f64 = 1e-9;
/// The absolute round-off margin for lengths (mm) and parameters of order one. See the module
/// docs.
pub const ROUNDOFF: f64 = 1e-9;
/// The guard against dividing by (or normalising) an exact zero.
pub const TINY: f64 = 1e-300;

/// The fine relative round-off margin (a thousandth of [`REL`]). See the module docs.
pub const REL_FINE: f64 = 1e-12;
/// The fine angular round-off margin (a thousandth of [`ANGULAR`]). See the module docs.
pub const ANGULAR_FINE: f64 = 1e-12;
/// The fine absolute round-off margin (a thousandth of [`ROUNDOFF`]). See the module docs.
pub const ROUNDOFF_FINE: f64 = 1e-12;
/// A computed length, norm or denominator below this is zero: no direction, nothing to divide
/// by. See the module docs.
pub const DEGENERATE: f64 = 1e-12;

/// [`POINT_DEFAULT`] as a fraction of a size, span or range. See the module docs.
pub const REL_POINT: f64 = 1e-6;
/// Directions closer than this (radians, or `1 − cos`, `|sin|` for unit vectors) are the same at
/// the merge level. See the module docs.
pub const ANGULAR_POINT: f64 = 1e-6;
/// The step of a finite difference, or of a nudge off a point. See the module docs.
pub const DIFF_STEP: f64 = 1e-6;

/// What a length that is absolute for a part of a millimetre or more is scaled by for a
/// smaller part: 1 from a millimetre up, the part's size in mm below. A nudge of 1e-5 mm, a
/// shortest edge of 1e-3 mm, a floor of 1e-7 are fixed for ordinary parts and shrink with a
/// part they would be a good share of. (A part under a quarter of a millimetre is worked on
/// at a larger size altogether; this is for the ones between.)
pub fn small(size: f64) -> f64 {
    if size.is_finite() && size > 0.0 {
        size.min(1.0)
    } else {
        1.0
    }
}

/// The tolerances an operation runs with, carried by the kernel session
/// ([`super::NativeKernel::tolerances`]) and passed explicitly to the operations that take
/// them. `Default` is the model above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerances {
    /// The merge distance of a boolean's first run ([`point`]).
    pub point: f64,
    /// The merge distances of the runs after a first that leaves an open shell, in order.
    pub retries: [f64; 2],
    /// The smallest tolerance an entity holds ([`CONFUSION`]).
    pub confusion: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        // (Written out: 10.0 * POINT isn't exactly 1e-5 in floating point.)
        Self {
            point: POINT_DEFAULT,
            retries: [LOOP_GAP, 1e-4],
            confusion: CONFUSION,
        }
    }
}

impl Tolerances {
    /// A session merging at `point` (mm) on its first run, its retries ten and a hundred times
    /// that, and entities holding no less than [`CONFUSION`] or a tenth of `point`, the smaller.
    /// Parasolid's (and so Onshape's) linear precision is 1e-8 m: [`Self::parasolid`].
    pub fn with_point(point: f64) -> Self {
        Self {
            point,
            retries: [point * 10.0, point * 100.0],
            confusion: CONFUSION.min(point * 0.1),
        }
    }

    /// Parasolid's linear precision, 1e-8 m = 1e-5 mm (Onshape's `TOLERANCE.zeroLength` in
    /// std's math.fs): points closer are one point there. For comparing with Onshape.
    pub fn parasolid() -> Self {
        Self::with_point(1e-5)
    }

    /// The tolerances for a model of this size (its box's diagonal, mm): the defaults for a
    /// millimetre and more, to the bit; below that the merge distance and the retries shrink
    /// with the model ([`small`]), down to the smallest tolerance an entity holds, so that a
    /// retry is never a good share of the part.
    pub fn for_model(diagonal: f64) -> Self {
        let by = small(diagonal);
        let d = Self::default();
        let at_least = |x: f64| (x * by).max(d.confusion);
        Self {
            point: at_least(d.point),
            retries: [at_least(d.retries[0]), at_least(d.retries[1])],
            confusion: d.confusion,
        }
    }

    /// Checks that the tolerances are usable: finite, positive and growing
    /// (`confusion <= point <= retries`).
    pub fn validate(&self) -> crate::Result<()> {
        let all = [self.confusion, self.point, self.retries[0], self.retries[1]];
        if all.iter().any(|t| !t.is_finite() || *t <= 0.0) || all.windows(2).any(|w| w[1] < w[0]) {
            return Err(crate::KernelError::InvalidParameter(format!(
                "tolerances must be positive and growing: {self:?}"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults are the documented levels, and the constants the kernel used before
    /// (`topo::TOL_MIN`, `boolean::vtol()`, the boolean's retries) are these.
    #[test]
    fn defaults_are_the_model() {
        let t = Tolerances::default();
        assert_eq!(
            (t.confusion, t.point, t.retries),
            (1e-7, 1e-6, [1e-5, 1e-4])
        );
        assert_eq!(super::super::topo::TOL_MIN, CONFUSION);
        assert_eq!(super::super::boolean::vtol(), POINT_DEFAULT);
        assert!(t.validate().is_ok());
        // The named round-off margins are the literals they replaced, bit for bit.
        assert_eq!([REL, ANGULAR, ROUNDOFF], [1e-9; 3]);
        assert_eq!(
            [REL_FINE, ANGULAR_FINE, ROUNDOFF_FINE, DEGENERATE],
            [1e-12; 4]
        );
        assert_eq!(
            [POINT_DEFAULT, REL_POINT, ANGULAR_POINT, DIFF_STEP],
            [1e-6; 4]
        );
        assert!(
            Tolerances {
                point: f64::NAN,
                ..t
            }
            .validate()
            .is_err()
        );
        assert!(Tolerances { point: 1e-3, ..t }.validate().is_err());
        assert!(
            Tolerances {
                confusion: 0.0,
                ..t
            }
            .validate()
            .is_err()
        );
    }

    /// The session holds its tolerances, refusing unusable ones.
    #[test]
    fn session_tolerances() {
        let mut k = super::super::NativeKernel::new();
        assert_eq!(k.tolerances(), Tolerances::default());
        assert!(
            k.set_tolerances(Tolerances {
                point: -1.0,
                ..Tolerances::default()
            })
            .is_err()
        );
        assert_eq!(k.tolerances(), Tolerances::default());
        k.set_tolerances(Tolerances {
            point: 2e-6,
            ..Tolerances::default()
        })
        .unwrap();
        assert_eq!(k.tolerances().point, 2e-6);
    }
}
