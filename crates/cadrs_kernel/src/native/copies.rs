//! Faces of a body that are rigid copies of one another (a pattern's: [`super::pattern`]), as
//! the operation that made them knows them: work that depends on a face's shape alone (its mass
//! properties, its mesh) is done for one face of a set and moved to the others.

use super::math::*;

/// A rigid motion x ↦ m·x + t (m a rotation).
#[derive(Debug, Clone, Copy)]
pub struct Rigid {
    pub m: nalgebra::Matrix3<f64>,
    pub t: V3,
}

impl Rigid {
    pub fn identity() -> Self {
        Rigid {
            m: nalgebra::Matrix3::identity(),
            t: V3::zeros(),
        }
    }

    pub fn point(&self, p: &P3) -> P3 {
        P3::from(self.m * p.coords + self.t)
    }

    pub fn vector(&self, v: &V3) -> V3 {
        self.m * v
    }
}

/// Each face of a body that is a copy of another: that face, and the motion that takes it onto
/// this one (None for a face that is no copy, or the first of its set).
pub type FaceCopies = Vec<Option<(usize, Rigid)>>;

/// The sets of copies `copies` makes: each set's first face, then each of its copies with its
/// motion (the first with none).
pub fn sets(copies: &[Option<(usize, Rigid)>]) -> Vec<Vec<(usize, Rigid)>> {
    let mut by_first: rustc_hash::FxHashMap<usize, Vec<(usize, Rigid)>> = Default::default();
    for (i, c) in copies.iter().enumerate() {
        if let Some((first, r)) = c {
            by_first
                .entry(*first)
                .or_insert_with(|| vec![(*first, Rigid::identity())])
                .push((i, *r));
        }
    }
    let mut out: Vec<Vec<(usize, Rigid)>> = by_first.into_values().collect();
    out.sort_by_key(|s| s[0].0);
    out
}
