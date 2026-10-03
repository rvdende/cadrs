//! The **one definition** of a sheet metal model that every feature after it changes (SM1.6):
//! what the Sheet metal model built, then every later change, kept as one value and replayed
//! into the [`Model`] each time (`cadrs_core` refolds the parts from it).
//!
//! A [`Definition`] has two layers:
//!
//! - the **base**: the walls at their virtual sharps ([`SharpDef`]), which the Sheet metal model
//!   builds (Convert, Extrude, Thicken). Features that change how walls meet edit it in place:
//!   **Flange**, **Hem** and **Make joint** add walls, joints and hems ([`crate::sharp_edit`]),
//!   **Modify joint** and the table change a joint ([`Definition::edit_joint`]). Built again, the
//!   builder trims every wall for every bend and rip at once, so a flange's bend made a rip or
//!   given a new radius moves its tangent lines as if it had been made that way. A Sheet metal
//!   Loft made on its own has a fixed model as its base instead ([`Base::Fixed`]).
//! - the **steps**: changes of the built model, in feature order ([`Step`]): **Bend**, **Jog**,
//!   **Tab**, perpendicular **cuts**, **Corner break**, Face pattern / mirror copies, **Corner**
//!   and **Bend relief** overrides, and the walls a **Loft (Add)** adds
//!   ([`crate::model_edit`]). Each step names what it works on by persistent ids and positions,
//!   and makes its new walls and joints with ids seeded by its feature, so the steps replay the
//!   same way whenever the base changes underneath them.
//!
//! [`Definition::build`] builds the base, replays the steps in order and puts the joints in the
//! table's order (Move up / Move down, SM13.4). Base edits made after a step (a Flange after a
//! Bend) go into the base, under the step: the step then replays on the changed walls. A base
//! edit picks its edge on the built model, which [`Definition::pull_back`] carries back onto the
//! base wall (undoing the rigid moves steps made, such as a Bend turning the walls beyond it).

use serde::{Deserialize, Serialize};

use crate::bend::BendValue;
use crate::joint_edit::{self, JointChange, JointEdit};
use crate::model::{BendReliefOverride, BuildError, CornerOverride, Joint, JointId, Model, P3, Surface, Wall, WallId};
use crate::model_edit::{self, BendSpec, CornerBreakKind, CutTool, EditError, JogSpec, Placement, Region3};
use crate::poly::P2;
use crate::sharp_edit::SharpDef;

/// What a model was first built as.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Base {
    /// Walls at their virtual sharps (Convert, Extrude, Thicken; then Flange, Hem, Make joint
    /// and Modify joint edit it).
    Sharp(SharpDef),
    /// A model made directly (a Sheet metal Loft of its own).
    Fixed(Model),
}

/// A change of the built model, by a feature after the Sheet metal model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// The feature's name (for errors when the step no longer fits).
    pub label: String,
    pub edit: StepEdit,
}

/// What a step does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StepEdit {
    /// **Bend** (SM9): new wall and joints seeded by `seed`.
    Bend { spec: BendSpec, seed: u64 },
    /// **Jog** (SM19.1).
    Jog { spec: JogSpec, seed: u64 },
    /// **Tab** (SM5): profiles added to these walls.
    Tab { regions: Vec<Region3>, walls: Vec<WallId> },
    /// Perpendicular cuts (Extrude → Remove on active sheet metal, Tab's subtraction scope;
    /// SM1.6, SM12.1).
    Cut { tools: Vec<CutTool>, walls: Option<Vec<WallId>> },
    /// **Corner break** (SM11) and fillets or chamfers of corners (SM12.1): each wall corner
    /// (local 2D) and its shape.
    CornerBreaks { corners: Vec<(WallId, P2, CornerBreakKind)> },
    /// Face pattern / Face mirror of walls (SM12.2): one copy per placement.
    Copy { walls: Vec<WallId>, places: Vec<Placement>, seed: u64 },
    /// **Corner** (SM7).
    CornerRelief(CornerOverride),
    /// **Bend relief** (SM8).
    BendRelief(BendReliefOverride),
    /// Walls and joints added as they are (a Sheet metal Loft's Add, SM19.2).
    AddWalls { walls: Vec<Wall>, joints: Vec<Joint> },
}

/// Why a definition doesn't build.
#[derive(Clone, Debug, PartialEq)]
pub enum DefError {
    Base(BuildError),
    /// Step `index` (by its feature `label`) no longer fits.
    Step { index: usize, label: String, error: EditError },
}

impl DefError {
    /// The feature error for the feature named `current`: a step's own message, prefixed with
    /// the name of the feature it came from when that is another one.
    pub fn message(&self, current: &str) -> String {
        match self {
            DefError::Base(e) => e.message(),
            DefError::Step { label, error, .. } if label == current => error.message().to_string(),
            DefError::Step { label, error, .. } => format!("{label} no longer fits: {}", error.message()),
        }
    }
}

/// A model's definition (see the module docs).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub base: Base,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// The table order (SM13.4): these joints in this order, the others in their places.
    #[serde(default)]
    pub table_order: Vec<JointId>,
    /// Walls folded as mitred slabs rather than extruded (a loft's facet walls).
    #[serde(default)]
    pub slabs: Vec<WallId>,
    /// Pairs of walls whose flat-pattern parts fold into one part (a loft added to a model is
    /// one part with it, though no joint joins them).
    #[serde(default)]
    pub merges: Vec<(WallId, WallId)>,
}

impl Definition {
    pub fn sharp(def: SharpDef) -> Definition {
        Definition { base: Base::Sharp(def), steps: Vec::new(), table_order: Vec::new(), slabs: Vec::new(), merges: Vec::new() }
    }

    /// A model made directly; its walls fold as slabs when `slabs`.
    pub fn fixed(model: Model, slabs: bool) -> Definition {
        let walls = if slabs { model.walls.iter().map(|w| w.id).collect() } else { Vec::new() };
        Definition { base: Base::Fixed(model), steps: Vec::new(), table_order: Vec::new(), slabs: walls, merges: Vec::new() }
    }

    /// The sharp base, for Flange, Hem and Make joint (`None` for a fixed base).
    pub fn sharp_mut(&mut self) -> Option<&mut SharpDef> {
        match &mut self.base {
            Base::Sharp(d) => Some(d),
            Base::Fixed(_) => None,
        }
    }

    pub fn sharp_def(&self) -> Option<&SharpDef> {
        match &self.base {
            Base::Sharp(d) => Some(d),
            Base::Fixed(_) => None,
        }
    }

    /// Adds a step.
    pub fn push(&mut self, label: impl Into<String>, edit: StepEdit) {
        self.steps.push(Step { label: label.into(), edit });
    }

    /// The model the base builds.
    pub fn base_model(&self) -> Result<Model, DefError> {
        match &self.base {
            Base::Sharp(d) => d.build().map_err(DefError::Base),
            Base::Fixed(m) => Ok(m.clone()),
        }
    }

    /// The model: the base built, the steps replayed, the joints in table order.
    pub fn build(&self) -> Result<Model, DefError> {
        Ok(self.build_traced()?.0)
    }

    /// [`Definition::build`], with the joints each step made (step index per joint).
    pub fn build_traced(&self) -> Result<(Model, Vec<(JointId, usize)>), DefError> {
        let mut m = self.base_model()?;
        let mut made = Vec::new();
        for (i, s) in self.steps.iter().enumerate() {
            let before: Vec<JointId> = m.joints.iter().map(|j| j.id).collect();
            apply(&mut m, &s.edit).map_err(|error| DefError::Step { index: i, label: s.label.clone(), error })?;
            made.extend(m.joints.iter().filter(|j| !before.contains(&j.id)).map(|j| (j.id, i)));
        }
        joint_edit::reorder(&mut m, &self.table_order);
        Ok((m, made))
    }

    /// **Modify joint** (SM6.4) and the table's edits: a joint of the base changes kind (made a
    /// rip or a bend, a new radius, K factor, allowance or deduction); a bend a Bend or Jog
    /// made takes the new radius and K factor into its step. Errors for joints that can't change
    /// that way.
    pub fn edit_joint(&mut self, e: JointEdit) -> Result<(), String> {
        if let Base::Sharp(d) = &mut self.base
            && joint_edit::apply(&mut d.builder, &[e]).is_empty()
        {
            return Ok(());
        }
        if self.sharp_def().is_some_and(|d| d.builder.hems.iter().any(|h| h.id == Some(e.joint))) {
            return Err("A hem's bend is set in its Hem feature".into());
        }
        let (_, made) = self.build_traced().map_err(|x| x.message(""))?;
        let Some(&(_, si)) = made.iter().find(|(j, _)| *j == e.joint) else {
            return Err(match self.base {
                Base::Fixed(_) => "A loft's joints can't be modified".into(),
                Base::Sharp(_) => "The joint no longer exists".into(),
            });
        };
        let JointChange::Bend { radius, value } = e.change else {
            return Err("A bend made by a Bend or Jog feature can't be made a rip".into());
        };
        let k = match value {
            None => None,
            Some(BendValue::KFactor(k)) => Some(k),
            Some(_) => return Err("A bend made by a Bend or Jog feature takes a K factor, not an allowance or deduction".into()),
        };
        match &mut self.steps[si].edit {
            StepEdit::Bend { spec, .. } => {
                spec.radius = radius;
                spec.k_factor = k;
            }
            StepEdit::Jog { spec, .. } => {
                spec.bend.radius = radius;
                spec.bend.k_factor = k;
            }
            _ => return Err("Only bends can be modified".into()),
        }
        Ok(())
    }

    /// Points on wall `wall` of the built model carried back onto the same wall of the base:
    /// steps turn and move walls rigidly (a Bend turns the walls beyond it) without changing
    /// their outlines' own coordinates, so a point keeps its place on its wall. `None` if the
    /// wall isn't a planar wall of the base (a wall a step made).
    pub fn pull_back(&self, built: &Model, wall: WallId, pts: &[P3]) -> Option<Vec<P3>> {
        let w = built.wall(wall)?;
        let Surface::Planar { .. } = w.surface else { return None };
        let nf = w.surface.normal()?;
        let of = w.surface.point(P2::origin());
        let (origin, u, v) = match &self.base {
            Base::Sharp(d) => {
                let s = d.builder.walls.iter().find(|s| s.id == Some(wall))?;
                (s.origin, s.u, s.v)
            }
            Base::Fixed(m) => match m.wall(wall)?.surface {
                Surface::Planar { origin, u, v } => (origin, u, v),
                Surface::Rolled { .. } => return None,
            },
        };
        let base = Surface::Planar { origin, u, v };
        let nb = base.normal()?;
        Some(pts.iter().map(|p| base.point(w.surface.local(*p)) + nb * (p - of).dot(&nf)).collect())
    }
}

/// Replays one step on a model.
fn apply(m: &mut Model, e: &StepEdit) -> Result<(), EditError> {
    match e {
        StepEdit::Bend { spec, seed } => model_edit::bend_wall(m, spec, *seed).map(|_| ()),
        StepEdit::Jog { spec, seed } => model_edit::jog_wall(m, spec, *seed).map(|_| ()),
        StepEdit::Tab { regions, walls } => model_edit::add_tab(m, regions, walls).map(|_| ()),
        StepEdit::Cut { tools, walls } => model_edit::cut_walls(m, tools, walls.as_deref()).map(|_| ()),
        StepEdit::CornerBreaks { corners } => {
            for (w, at, kind) in corners {
                model_edit::break_corner(m, *w, *at, *kind)?;
            }
            Ok(())
        }
        StepEdit::Copy { walls, places, seed } => {
            for (k, p) in places.iter().enumerate() {
                model_edit::copy_walls(m, walls, p, seed ^ ((k as u64) << 48))?;
            }
            Ok(())
        }
        StepEdit::CornerRelief(o) => {
            m.corner_overrides.push(*o);
            Ok(())
        }
        StepEdit::BendRelief(o) => {
            m.bend_relief_overrides.push(*o);
            Ok(())
        }
        StepEdit::AddWalls { walls, joints } => {
            m.walls.extend(walls.iter().cloned());
            m.joints.extend(joints.iter().cloned());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::construct::{self, FaceIn, FaceOpts, stable_id};
    use crate::flat::flatten;
    use crate::model::{JointKind, RipStyle, V3};
    use crate::model_edit::BendAlignment;
    use crate::params::Params;
    use crate::poly::Polygon;
    use crate::sharp_edit::{self, FlangeAlignment, FlangeEdge, FlangeOpts};

    fn params() -> Params {
        Params { thickness: 2.0, bend_radius: 3.0, k_factor: 0.45, minimal_gap: 0.2, ..Default::default() }
    }

    /// A 100 × 60 plate (Thicken of one face).
    fn plate() -> Definition {
        let f = FaceIn { key: 1, origin: P3::origin(), u: V3::x(), v: V3::y(), outline: Polygon::rect(P2::origin(), P2::new(100.0, 60.0)) };
        let b = construct::from_faces(params(), &[f], &[], &[], &FaceOpts::default()).unwrap();
        Definition::sharp(b.def)
    }

    fn flange_on(d: &mut Definition, m: &Model, a: P3, b: P3, key: u64) {
        let pick = sharp_edit::locate(m, &[a, b]).unwrap();
        let wall = pick.wall;
        let pts = d.pull_back(m, wall, &[pick.a, pick.b]).unwrap();
        let pick = sharp_edit::EdgePick { a: pts[0], b: pts[1], ..pick };
        let e = FlangeEdge { pick, key, angle: std::f64::consts::FRAC_PI_2, toward: true, distance: 30.0, partial: None };
        sharp_edit::flange(d.sharp_mut().unwrap(), &[e], &FlangeOpts { alignment: FlangeAlignment::Inner, radius: None, miter: None, hold_adjacent: false, per_chain: false }).unwrap();
    }

    fn bend_step(m: &Model, wall: WallId, x: f64, seed: u64) -> StepEdit {
        let spec = BendSpec {
            wall,
            line: (P3::new(x, 0.0, 0.0), P3::new(x, 60.0, 0.0)),
            hold_opposite: false,
            alignment: BendAlignment::BendLine,
            angle: std::f64::consts::FRAC_PI_2,
            toward_material: true,
            line_height: 0.0,
            radius: None,
            k_factor: None,
        };
        let _ = m;
        StepEdit::Bend { spec, seed }
    }

    #[test]
    fn a_flange_then_a_modify_joint_on_its_bend() {
        let mut d = plate();
        let m = d.build().unwrap();
        flange_on(&mut d, &m, P3::new(0.0, 60.0, 0.0), P3::new(100.0, 60.0, 0.0), 77);
        let m = d.build().unwrap();
        let bend = m.joints.iter().find(|j| j.bend().is_some()).unwrap().id;
        let before = flatten(&m);
        d.edit_joint(JointEdit { joint: bend, change: JointChange::Bend { radius: Some(6.0), value: None } }).unwrap();
        let m2 = d.build().unwrap();
        assert_eq!(m2.joint(bend).unwrap().bend().unwrap().radius, 6.0);
        assert!(m2.validate().is_empty());
        let after = flatten(&m2);
        assert!(after.is_ok());
        assert_ne!(before.parts[0].bounds(), after.parts[0].bounds());
        // Made a rip: the flange comes off as a part of its own.
        d.edit_joint(JointEdit { joint: bend, change: JointChange::Rip { style: RipStyle::EdgeJoint } }).unwrap();
        let m3 = d.build().unwrap();
        assert!(matches!(m3.joint(bend).unwrap().kind, JointKind::Rip { .. }));
        assert_eq!(flatten(&m3).parts.len(), 2);
    }

    #[test]
    fn a_bend_then_a_flange_on_the_wall_it_turned_and_a_modify_joint_on_the_bend() {
        let mut d = plate();
        let m = d.build().unwrap();
        let wall = m.walls[0].id;
        d.push("Bend 1", bend_step(&m, wall, 70.0, 5));
        let m = d.build().unwrap();
        let bent = m.joints.iter().find(|j| j.bend().is_some()).unwrap().id;
        // The flange's edge is the far end of the plate, which the bend turned up: its picks
        // on the built model come back onto the base wall... but that end is on the Bend's new
        // wall, so it can't take a flange; the near end can.
        let moved = m.walls.iter().find(|w| w.id != wall).unwrap().id;
        assert!(d.pull_back(&m, moved, &[P3::origin()]).is_none());
        flange_on(&mut d, &m, P3::new(0.0, 0.0, 0.0), P3::new(0.0, 60.0, 0.0), 9);
        let m = d.build().unwrap();
        assert_eq!(m.joints.iter().filter(|j| j.bend().is_some()).count(), 2);
        assert!(m.validate().is_empty(), "{:?}", m.validate());
        assert!(flatten(&m).is_ok());
        // The Bend's bend takes a new radius into its step.
        d.edit_joint(JointEdit { joint: bent, change: JointChange::Bend { radius: Some(5.0), value: Some(BendValue::KFactor(0.3)) } }).unwrap();
        let m = d.build().unwrap();
        let b = m.joint(bent).unwrap().bend().unwrap();
        assert_eq!((b.radius, b.value), (5.0, Some(BendValue::KFactor(0.3))));
        // ...but it can't be made a rip.
        assert!(d.edit_joint(JointEdit { joint: bent, change: JointChange::Rip { style: RipStyle::EdgeJoint } }).is_err());
    }

    #[test]
    fn a_step_that_no_longer_fits_names_its_feature() {
        let mut d = plate();
        let m = d.build().unwrap();
        let wall = m.walls[0].id;
        d.push("Bend 1", bend_step(&m, wall, 70.0, 5));
        d.push("Bend 2", bend_step(&m, wall, 500.0, 6));
        let e = d.build().unwrap_err();
        assert_eq!(e.message("Bend 2"), EditError::LineMissesWall.message());
        d.steps.swap(0, 1);
        assert!(d.build().unwrap_err().message("Bend 1").starts_with("Bend 2 no longer fits"));
    }

    #[test]
    fn steps_replay_the_same_and_round_trip() {
        let mut d = plate();
        let m = d.build().unwrap();
        d.push("Bend 1", bend_step(&m, m.walls[0].id, 70.0, 5));
        let a = d.build().unwrap();
        let s = ron::to_string(&d).unwrap();
        let back: Definition = ron::from_str(&s).unwrap();
        assert_eq!(back, d);
        assert_eq!(back.build().unwrap(), a);
        let ids: Vec<u32> = a.walls.iter().map(|w| w.id.0).collect();
        assert!(ids.contains(&stable_id(1)));
    }
}
