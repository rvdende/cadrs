//! Changing a built sheet metal model's joints (P3I.3; SM6.4, SM13.3, SM13.4): what the
//! **Modify joint** feature and the Sheet metal table do.
//!
//! - A [`JointEdit`] turns one joint (by its persistent id) into a bend (with its own radius
//!   and K factor / allowance / deduction, or the model's) or a rip (with a style). Edits act
//!   on the walls **at their virtual sharps** ([`crate::SharpBuilder`]) before they are trimmed,
//!   so a new radius moves the tangent lines and a bend made a rip leaves a gap: the folded
//!   solid and the flat pattern follow as if the model had been made that way.
//! - A [`Recipe`] is what a Sheet metal model was built from (the faces, edges and cylinders of
//!   a Convert or Thicken, the chains of an Extrude), kept with the model so features after it
//!   can build it again with their edits ([`Recipe::build`]).
//! - [`reorder`]: the table order (Move up / Move down) as a list of joint ids.

use serde::{Deserialize, Serialize};

use crate::bend::BendValue;
use crate::construct::{self, Built, ChainIn, ChainOpts, ConstructError, CylIn, EdgeIn, FaceIn, FaceOpts};
use crate::model::{JointId, Model, RipStyle, SharpBuilder, SharpJointKind};
use crate::params::Params;

/// What a joint becomes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum JointChange {
    /// A bend: `radius: None` uses the model's bend radius, `value: None` the model's K factor
    /// (or allowance, or deduction).
    Bend { radius: Option<f64>, value: Option<BendValue> },
    Rip { style: RipStyle },
}

/// One joint's edit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointEdit {
    pub joint: JointId,
    pub change: JointChange,
}

/// "Bend C" ↔ "Joint C": the joint keeps its letter when it changes kind (its row moves between
/// the tables, SM13.4).
pub fn renamed(name: &str, bend: bool) -> String {
    let letter = name.strip_prefix("Bend ").or_else(|| name.strip_prefix("Joint ")).or_else(|| name.strip_prefix("Rip "));
    match letter {
        Some(l) if bend => format!("Bend {l}"),
        Some(l) => format!("Joint {l}"),
        None => name.to_string(),
    }
}

/// Applies the edits to a builder's joints (the last edit of a joint wins). Returns the edits
/// whose joint the builder doesn't have (a tangent joint, or one that is gone).
pub fn apply(b: &mut SharpBuilder, edits: &[JointEdit]) -> Vec<JointId> {
    let mut missing = Vec::new();
    for e in edits {
        let Some(j) = b.joints.iter_mut().find(|j| j.id == Some(e.joint)) else {
            if !missing.contains(&e.joint) {
                missing.push(e.joint);
            }
            continue;
        };
        let bend = matches!(e.change, JointChange::Bend { .. });
        j.kind = match e.change {
            JointChange::Bend { radius, value } => SharpJointKind::Bend { radius, value },
            JointChange::Rip { style } => SharpJointKind::Rip { style },
        };
        if let Some(n) = &j.name {
            j.name = Some(renamed(n, bend));
        }
    }
    missing
}

/// Puts the joints named in `order` in that order, in the places those joints take in the
/// table; the others keep their places (Move up / Move down, SM13.4).
pub fn reorder(m: &mut Model, order: &[JointId]) {
    let rank = |id: JointId| order.iter().position(|o| *o == id);
    let slots: Vec<usize> = (0..m.joints.len()).filter(|i| rank(m.joints[*i].id).is_some()).collect();
    let mut listed: Vec<_> = slots.iter().map(|i| m.joints[*i].clone()).collect();
    listed.sort_by_key(|j| rank(j.id));
    for (slot, j) in slots.into_iter().zip(listed) {
        m.joints[slot] = j;
    }
}

/// The table order after moving `joint` up (`by < 0`) or down past the next row of its table:
/// the whole order of the model's joints, to keep with the model.
pub fn moved(m: &Model, joint: JointId, by: isize) -> Option<Vec<JointId>> {
    let mut m = m.clone();
    m.move_joint(joint, by).then(|| m.joints.iter().map(|j| j.id).collect())
}

/// What a Sheet metal model was built from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Recipe {
    /// Convert or Thicken.
    Faces { params: Params, faces: Vec<FaceIn>, cyls: Vec<CylIn>, edges: Vec<EdgeIn>, opts: FaceOpts },
    /// Extrude: one set of chains per sketch, into one model.
    Chains { params: Params, groups: Vec<(Vec<ChainIn>, ChainOpts)> },
}

impl Recipe {
    /// Builds the model again with `edits` (and the edits its options carry already).
    pub fn build(&self, edits: &[JointEdit]) -> Result<Built, ConstructError> {
        match self {
            Recipe::Faces { params, faces, cyls, edges, opts } => {
                let mut o = opts.clone();
                o.edits.extend_from_slice(edits);
                construct::from_faces(*params, faces, cyls, edges, &o)
            }
            Recipe::Chains { params, groups } => {
                let mut all: Option<Built> = None;
                for (chains, opts) in groups {
                    let mut o = opts.clone();
                    o.edits.extend_from_slice(edits);
                    let b = construct::from_chains(*params, chains, &o)?;
                    all = Some(match all {
                        None => b,
                        Some(mut a) => {
                            a.model.walls.extend(b.model.walls);
                            a.model.joints.extend(b.model.joints);
                            a.walls.extend(b.walls);
                            a.joints.extend(b.joints);
                            a.warnings.extend(b.warnings);
                            a
                        }
                    });
                }
                all.ok_or(ConstructError::NoWalls)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::construct::stable_id;
    use crate::flat::flatten;
    use crate::model::{JointKind, P3, V3};
    use crate::poly::{P2, Polygon};

    fn params() -> Params {
        Params { thickness: 2.0, bend_radius: 3.0, k_factor: 0.45, minimal_gap: 0.2, ..Default::default() }
    }

    /// A 100 × 60 × 40 box's faces and edges (as `construct`'s tests), converted with its
    /// bottom's four edges bent: an open box and a separate top.
    fn open_box() -> Recipe {
        let (x, y, z) = (100.0, 60.0, 40.0);
        let f = |key, origin: P3, u: V3, v: V3, w: f64, h: f64| FaceIn { key, origin, u, v, outline: Polygon::rect(P2::origin(), P2::new(w, h)) };
        let faces = vec![
            f(1, P3::new(0.0, y, 0.0), V3::x(), -V3::y(), x, y),
            f(2, P3::new(0.0, 0.0, z), V3::x(), V3::y(), x, y),
            f(3, P3::new(0.0, 0.0, 0.0), V3::x(), V3::z(), x, z),
            f(4, P3::new(x, y, 0.0), -V3::x(), V3::z(), x, z),
            f(5, P3::new(0.0, y, 0.0), -V3::y(), V3::z(), y, z),
            f(6, P3::new(x, 0.0, 0.0), V3::y(), V3::z(), y, z),
        ];
        let e = |key, a: P3, b: P3, fa, fb| EdgeIn { key, a, b, faces: (fa, fb) };
        let edges = vec![
            e(10, P3::new(0.0, 0.0, 0.0), P3::new(x, 0.0, 0.0), 0, 2),
            e(11, P3::new(0.0, y, 0.0), P3::new(x, y, 0.0), 0, 3),
            e(12, P3::new(0.0, 0.0, 0.0), P3::new(0.0, y, 0.0), 0, 4),
            e(13, P3::new(x, 0.0, 0.0), P3::new(x, y, 0.0), 0, 5),
            e(14, P3::new(0.0, 0.0, z), P3::new(x, 0.0, z), 1, 2),
            e(15, P3::new(0.0, y, z), P3::new(x, y, z), 1, 3),
            e(16, P3::new(0.0, 0.0, z), P3::new(0.0, y, z), 1, 4),
            e(17, P3::new(x, 0.0, z), P3::new(x, y, z), 1, 5),
            e(18, P3::new(0.0, 0.0, 0.0), P3::new(0.0, 0.0, z), 2, 4),
            e(19, P3::new(x, 0.0, 0.0), P3::new(x, 0.0, z), 2, 5),
            e(20, P3::new(0.0, y, 0.0), P3::new(0.0, y, z), 3, 4),
            e(21, P3::new(x, y, 0.0), P3::new(x, y, z), 3, 5),
        ];
        Recipe::Faces { params: params(), faces, cyls: vec![], edges, opts: FaceOpts { bends: vec![10, 11, 12, 13], ..Default::default() } }
    }

    fn id(key: u64) -> JointId {
        JointId(stable_id(key))
    }

    #[test]
    fn a_bend_made_a_rip_and_a_rip_made_a_bend() {
        let r = open_box();
        let before = r.build(&[]).unwrap();
        assert_eq!(flatten(&before.model).parts.len(), 2);
        // Bend A (the bottom–south edge) becomes a rip: the south wall comes off on its own.
        let a = id(10);
        let edits = [JointEdit { joint: a, change: JointChange::Rip { style: RipStyle::EdgeJoint } }];
        let after = r.build(&edits).unwrap();
        let j = after.model.joint(a).unwrap();
        assert!(matches!(j.kind, JointKind::Rip { .. }));
        assert_eq!(j.name, "Joint A");
        assert!(after.model.validate().is_empty(), "{:?}", after.model.validate());
        let f = flatten(&after.model);
        assert!(f.is_ok(), "{:?}", f.errors);
        assert_eq!(f.parts.len(), 3);
        // The top–south rip becomes a bend (model radius): the top joins the south wall.
        let top_south = id(14);
        let name = before.model.joint(top_south).unwrap().name.clone();
        assert!(name.starts_with("Joint "));
        let edits = [edits[0], JointEdit { joint: top_south, change: JointChange::Bend { radius: None, value: None } }];
        let after = r.build(&edits).unwrap();
        let j = after.model.joint(top_south).unwrap();
        assert_eq!(j.bend().map(|b| b.radius), Some(3.0));
        assert_eq!(j.name, renamed(&name, true));
        let f = flatten(&after.model);
        assert!(f.is_ok(), "{:?}", f.errors);
        assert_eq!(f.parts.len(), 2);
        // A joint that isn't there is ignored (the feature reports it).
        let mut b = SharpBuilder::new(params());
        assert_eq!(apply(&mut b, &edits), vec![a, top_south]);
    }

    #[test]
    fn a_new_radius_and_k_factor_change_the_flat_and_a_rip_style_holds() {
        let r = open_box();
        let before = r.build(&[]).unwrap();
        let b = id(11);
        let len = |m: &Model| {
            let f = flatten(m);
            let (lo, hi) = f.parts.iter().max_by_key(|p| p.walls.len()).unwrap().bounds().unwrap();
            (hi.x - lo.x) + 1e3 * (hi.y - lo.y)
        };
        let k = BendValue::KFactor(0.3);
        let edits = [JointEdit { joint: b, change: JointChange::Bend { radius: Some(6.0), value: Some(k) } }];
        let after = r.build(&edits).unwrap();
        let bend = *after.model.joint(b).unwrap().bend().unwrap();
        assert_eq!(bend.radius, 6.0);
        assert!(!bend.model_radius);
        assert_eq!(bend.value, Some(k));
        assert!(after.model.validate().is_empty());
        // The flat across that bend changes by the change in setbacks and allowance.
        assert!((len(&after.model) - len(&before.model)).abs() > 1e-3);
        // A rip's style (a 90° rip, so a butt joint is allowed).
        let rip = id(18);
        let edits = [JointEdit { joint: rip, change: JointChange::Rip { style: RipStyle::ButtDirection1 } }];
        let after = r.build(&edits).unwrap();
        assert!(matches!(after.model.joint(rip).unwrap().kind, JointKind::Rip { style: RipStyle::ButtDirection1, .. }));
    }

    #[test]
    fn the_table_order_survives_a_rebuild() {
        let r = open_box();
        let mut m = r.build(&[]).unwrap().model;
        let order = moved(&m, id(12), -1).unwrap();
        reorder(&mut m, &order);
        let bends: Vec<&str> = m.joints.iter().filter(|j| j.bend().is_some()).map(|j| j.name.as_str()).collect();
        assert_eq!(bends, ["Bend A", "Bend C", "Bend B", "Bend D"]);
        // Rebuilt from scratch and reordered: the same order.
        let mut again = r.build(&[]).unwrap().model;
        reorder(&mut again, &order);
        assert_eq!(again.joints.iter().map(|j| j.id).collect::<Vec<_>>(), m.joints.iter().map(|j| j.id).collect::<Vec<_>>());
        // Moving the first bend up does nothing.
        assert!(moved(&m, id(10), -1).is_none());
    }

    #[test]
    fn a_recipe_round_trips_through_ron() {
        let r = open_box();
        let s = ron::to_string(&r).unwrap();
        let back: Recipe = ron::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn names_keep_their_letter() {
        assert_eq!(renamed("Bend C", false), "Joint C");
        assert_eq!(renamed("Joint AB", true), "Bend AB");
        assert_eq!(renamed("Bend C", true), "Bend C");
        assert_eq!(renamed("Hem", true), "Hem");
    }
}
