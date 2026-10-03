//! Rebuilding a **Modify joint** (`crate::sheetmetal_joint`, P3I.3; SM6.4): the Sheet metal
//! model's definition built again from its recipe with this joint's edit added to the edits
//! before it ([`cadrs_sheetmetal::edit`]), checked and laid flat like the model itself, and its
//! parts refolded **in place**: each new flat-pattern part takes the id of the old part it
//! shares walls with, and the bodies are named with the Sheet metal model's operation, so
//! faces keep their names and the features after it keep their references. A part that no
//! longer exists (two parts joined by a rip made a bend) is removed; a new one (a bend made a
//! rip that cuts a part in two) gets a new id.

use super::*;
use cadrs_sheetmetal::{JointKind, flatten};

use super::sheetmetal::flat_error;
use crate::sheetmetal::SheetMetalContext;
use crate::sheetmetal_joint::ModifyJointFeature;

/// The state with `ctx` as its model's context.
fn with_context(state: &State, ctx: SheetMetalContext) -> State {
    let mut next = state.clone();
    let mut all = (*next.sheet_metal).clone();
    all.retain(|c| c.feature != ctx.feature);
    all.push(ctx);
    next.sheet_metal = Arc::new(all);
    next
}

fn output(state: State, error: Option<String>) -> Output {
    Output {
        state: Arc::new(state),
        error,
        warning: None,
        contacts: None,
        owned: Vec::new(),
        stage: None,
        axis: None,
        arrows: Vec::new(),
        dots: None,
        uses: Vec::new(),
    }
}

impl Rebuilder {
    pub(in crate::rebuild) fn modify_joint(&mut self, id: FeatureId, x: &ModifyJointFeature, state: &Arc<State>) -> Result<Output, String> {
        let ctx = state
            .sheet_metal
            .iter()
            .find(|c| c.feature == x.model)
            .cloned()
            .ok_or("The sheet metal model of this joint doesn't exist")?;
        if let Some(e) = x.range_error() {
            return Err(e.message());
        }
        let joint = x.joint.ok_or("Select a joint")?;
        let now = ctx.model.joint(joint).ok_or("The joint no longer exists")?;
        let tangent_now = matches!(now.kind, JointKind::Tangent { .. });
        let Some(edit) = x.edit() else {
            // Tangent: only a joint between a rolled wall and its neighbour is one, as it is.
            return if tangent_now {
                Ok(output((**state).clone(), None))
            } else {
                Err("Only a joint with a rolled (cylindrical) wall can be tangent".into())
            };
        };
        if tangent_now {
            return Err("A tangent joint can't be made a bend or a rip".into());
        }
        let recipe = ctx.recipe.clone().ok_or("The sheet metal model must be rebuilt before its joints can be modified")?;
        let mut edits = ctx.edits.clone();
        edits.retain(|e| e.joint != joint);
        edits.push(edit);
        let built = recipe.build(&edits).map_err(|e| e.message())?;
        if !built.model.joints.iter().any(|j| j.id == joint) {
            return Err("The joint no longer exists".into());
        }
        let mut model = built.model;
        cadrs_sheetmetal::edit::reorder(&mut model, &ctx.table_order);
        if let Some(e) = model.validate().first() {
            return Err(format!("Sheet metal model is inconsistent: {}", e.message()));
        }
        let flat = flatten(&model);
        let mut next_ctx = SheetMetalContext {
            model: model.clone(),
            flat: flat.clone(),
            wall_keys: built.walls,
            joint_keys: built.joints,
            edits,
            ..ctx.clone()
        };
        if let Some(why) = flat_error(&flat) {
            // Keep the context: the flat view shows where it collides.
            return Ok(output(with_context(state, next_ctx), Some(why)));
        }
        // The model's own operation: the refolded faces keep their names.
        let op = ctx.feature.0;
        let folded = self.fold(&|_| op, op, &model, &flat)?;
        for (_, body, _, sum) in &folded {
            let v = self.kernel.mass_properties(*body).map(|m| m.volume).unwrap_or(*sum);
            if v < sum - (1e-6 * sum + 1e-3) {
                for (_, b, _, _) in &folded {
                    self.kernel.release(*b);
                }
                return Ok(output(with_context(state, next_ctx), Some("Sheet metal walls intersect".into())));
            }
        }
        let geoms = state.geoms.clone();
        let mut next = State { geoms: geoms.clone(), ..(**state).clone() };
        let mut placed: Vec<Placed> = Vec::new();
        let mut walls_of: Vec<(PartId, Vec<cadrs_sheetmetal::WallId>)> = Vec::new();
        let mut folded = folded.into_iter();
        while let Some((walls, body, names, _)) = folded.next() {
            let pieces = self.split(body, op, &[(body, &names)]);
            self.kernel.release(body);
            let pieces = match pieces {
                Ok(p) => p,
                Err(e) => {
                    for (_, b, _, _) in folded {
                        self.kernel.release(b);
                    }
                    for (_, p) in placed {
                        self.kernel.release(p.body);
                    }
                    return Err(e);
                }
            };
            for (n, pc) in pieces.into_iter().enumerate() {
                let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                let old = (n == 0)
                    .then(|| ctx.parts.iter().find(|(p, ws)| !taken.contains(p) && ws.iter().any(|w| walls.contains(w))).map(|(p, _)| *p))
                    .flatten();
                let pid = old.unwrap_or_else(|| Self::new_id(ctx.feature, &next, &taken));
                if n == 0 {
                    walls_of.push((pid, walls.clone()));
                }
                placed.push((pid, pc));
            }
        }
        // The model's parts that are no more.
        let kept: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
        next.parts.retain(|p| kept.contains(&p.part.id) || !ctx.parts.iter().any(|(q, _)| *q == p.part.id));
        let mut o = self.finish(id, placed, next, op, geoms, PartKind::Solid)?;
        next_ctx.parts = walls_of;
        o.state = Arc::new(with_context(&o.state, next_ctx));
        Ok(o)
    }
}
