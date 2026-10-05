//! Rebuilding the flat pattern extrude (`crate::sheetmetal_flat`, P3I.6, SM14): the regions of a
//! flat-pattern sketch are added to or removed from the Sheet metal model's definition in the
//! flat ([`cadrs_sheetmetal::flat_edit`]) as a step of the model's definition, and the model
//! refolds through the one sheet metal pipeline (`sheetmetal/refold.rs`: same part ids, names
//! and appearances). A child of `rebuild::kernel_ops`.
//!
//! Also here: the flat pattern planes each model's rebuild registers (sketches on the flat
//! follow them) and the slicing of a non-rectangular cut on a bend region into wedges.

use super::*;
use cadrs_sheetmetal::flat_edit;
use cadrs_sheetmetal::poly::{P2, Polygon, V2};
use cadrs_sheetmetal::definition::StepEdit;

use crate::sheetmetal::SheetMetalContext;
use crate::sheetmetal_flat::{FlatExtrudeFeature, flat_frame, flat_plane_id, sketch_target};

/// Registers the flat pattern plane of each of a model's parts (as its Plane-feature frame).
pub(super) fn register_flat_planes(next: &mut State, ctx: &SheetMetalContext) {
    for (k, part) in ctx.flat.parts.iter().enumerate().take(crate::sheetmetal_flat::MAX_PARTS) {
        if let Some(f) = flat_frame(&ctx.model, part) {
            next.planes.insert(FeatureId(flat_plane_id(ctx.feature, k)), f);
        }
    }
}

/// How many slices a non-rectangular cut across a bend region is taken out in.
const SLICES: usize = 24;

/// The (s, u) boxes a cut on a bend region is taken out as: itself when it is a rectangle, else
/// [`SLICES`] slices across the bend, each as long as the cut is over its slice (`allowance`: the
/// region's width).
pub(super) fn wedge_boxes(cut: &Polygon, allowance: f64) -> Vec<(P2, P2)> {
    let Some((lo, hi)) = cut.bounds() else { return Vec::new() };
    let box_area = (hi.x - lo.x) * (hi.y - lo.y);
    if (box_area - cut.area()).abs() <= 1e-9 * box_area.max(1e-12) || hi.y - lo.y <= 1e-9 {
        return vec![(lo, hi)];
    }
    let (u0, u1) = (lo.y.max(0.0), hi.y.min(allowance.max(lo.y)));
    let step = (u1 - u0) / SLICES as f64;
    let mut out = Vec::new();
    for k in 0..SLICES {
        let (a, b) = (u0 + step * k as f64, u0 + step * (k + 1) as f64);
        let slice = cut.clip_half_plane(P2::new(0.0, a), V2::new(0.0, 1.0)).clip_half_plane(P2::new(0.0, b), V2::new(0.0, -1.0));
        if let Some((l, h)) = slice.bounds().filter(|_| slice.area() > 1e-12) {
            out.push((P2::new(l.x, a), P2::new(h.x, b)));
        }
    }
    out
}

impl Rebuilder {
    pub(in crate::rebuild) fn flat_extrude(&mut self, before: &[Feature], id: FeatureId, name: &str, x: &FlatExtrudeFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        // The flat pattern its sketches lie on: one model's one part.
        let mut target: Option<(FeatureId, usize)> = None;
        for s in x.sketch_ids() {
            let t = sketch_target(before, s).ok_or("Select regions of a sketch on the flat pattern")?;
            if target.is_some_and(|have| have != t) {
                return Err("The regions must all be on one flat pattern".into());
            }
            target = Some(t);
        }
        let (model_id, part) = target.ok_or("Select regions of a sketch on the flat pattern")?;
        let ctx = state
            .sheet_metal
            .iter()
            .rev()
            .find(|c| c.feature == model_id)
            .cloned()
            .ok_or("The sheet metal model failed: there is no flat pattern to extrude on")?;
        if !ctx.active {
            return Err("The sheet metal model is finished: its flat pattern can't change".into());
        }
        let (groups, lost) = sweep_groups(before, &x.regions, &x.sketches, crate::document::BodyType::Solid);
        // The sketch's coordinates are the flat pattern's.
        let ring = |l: &[cadrs_sketch::Vec2]| l.iter().map(|q| P2::new(q.x, q.y)).collect::<Vec<_>>();
        let region: Vec<Polygon> = groups
            .iter()
            .flat_map(|g| g.regions.iter().map(|(_, r)| Polygon::with_holes(ring(&r.outer), r.holes.iter().map(|h| ring(h)).collect())))
            .collect();
        if region.is_empty() {
            return Err("The selected regions no longer exist".into());
        }
        // Try it on the model as it is, for the feature's own error.
        let mut model = ctx.model.clone();
        let edit = if x.remove { flat_edit::remove(&mut model, &ctx.flat, part, &region) } else { flat_edit::add(&mut model, &ctx.flat, part, &region) };
        edit.map_err(|e| e.message().to_string())?;
        let mut warning = (lost > 0).then(|| "A selected region no longer exists".to_string());
        // Features other than sheet metal ones that changed the folded parts since: their changes
        // are rebuilt away (the definition is the source of truth).
        let ours = |f: &FeatureId| *f == model_id || ctx.editors.contains(f);
        if let Some(other) = ctx
            .parts
            .iter()
            .filter_map(|(p, _)| state.part(*p))
            .flat_map(|p| p.part.features.iter())
            .find(|f| !ours(f))
            .and_then(|f| before.iter().find(|b| b.id == *f))
        {
            warning = Some(format!("{} changed the folded part; the flat pattern extrude rebuilds the part without it", other.name));
        }
        // A step of the model's definition, refolded through the one sheet metal pipeline.
        let ci = state.sheet_metal.iter().rposition(|c| c.feature == model_id).expect("found");
        self.edit_sheet_metal(id, name, state, ci, move |ctx| {
            ctx.def.as_mut().ok_or("The sheet metal model must be rebuilt first")?.push(name, StepEdit::Flat { part, regions: region, remove: x.remove });
            Ok(warning)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_are_one_wedge_and_other_shapes_slices() {
        let r = Polygon::rect(P2::new(1.0, 0.0), P2::new(3.0, 2.0));
        assert_eq!(wedge_boxes(&r, 2.0), vec![(P2::new(1.0, 0.0), P2::new(3.0, 2.0))]);
        // A triangle: slices that get shorter towards its tip, none longer than it.
        let t = Polygon::new(vec![P2::new(0.0, 0.0), P2::new(4.0, 0.0), P2::new(0.0, 2.0)]);
        let b = wedge_boxes(&t, 2.0);
        assert_eq!(b.len(), SLICES);
        assert!(b.windows(2).all(|w| w[1].1.x <= w[0].1.x + 1e-12));
        let covered: f64 = b.iter().map(|(l, h)| (h.x - l.x) * (h.y - l.y)).sum();
        assert!(covered >= t.area() && covered < t.area() * 1.1, "{covered}");
    }
}
