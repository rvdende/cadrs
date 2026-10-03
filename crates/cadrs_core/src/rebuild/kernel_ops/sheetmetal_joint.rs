//! Rebuilding a **Modify joint** (`crate::sheetmetal_joint`, P3I.3; SM6.4): the joint's edit
//! made in the Sheet metal model's definition ([`cadrs_sheetmetal::definition::Definition::edit_joint`]:
//! a joint of the walls at their virtual sharps, the model's own or a Flange's, Hem's or Make
//! joint's, changes kind, radius or bend value; a Bend's or Jog's bend takes the new radius and K
//! factor into its step), then the model refolded through the one sheet metal pipeline
//! (`sheetmetal/refold.rs`): its parts keep their ids, a part a rip cuts off gets a new one, a
//! part a bend joins to another is gone.

use super::sheetmetal::refold::plain_output;
use super::*;
use cadrs_sheetmetal::JointKind;

use crate::sheetmetal_joint::ModifyJointFeature;

impl Rebuilder {
    pub(in crate::rebuild) fn modify_joint(&mut self, id: FeatureId, name: &str, x: &ModifyJointFeature, state: &Arc<State>) -> Result<Output, String> {
        let ci = state.sheet_metal.iter().position(|c| c.feature == x.model).ok_or("The sheet metal model of this joint doesn't exist")?;
        if let Some(e) = x.range_error() {
            return Err(e.message());
        }
        let joint = x.joint.ok_or("Select a joint")?;
        let now = state.sheet_metal[ci].model.joint(joint).ok_or("The joint no longer exists")?;
        let tangent_now = matches!(now.kind, JointKind::Tangent { .. });
        let Some(edit) = x.edit() else {
            // Tangent: only a joint between a rolled wall and its neighbour is one, as it is.
            return if tangent_now {
                Ok(plain_output((**state).clone(), None))
            } else {
                Err("Only a joint with a rolled (cylindrical) wall can be tangent".into())
            };
        };
        if tangent_now {
            return Err("A tangent joint can't be made a bend or a rip".into());
        }
        self.edit_sheet_metal(id, name, state, ci, |ctx| {
            ctx.def.as_mut().ok_or("The sheet metal model must be rebuilt before its joints can be modified")?.edit_joint(edit)?;
            Ok(None)
        })
    }
}
