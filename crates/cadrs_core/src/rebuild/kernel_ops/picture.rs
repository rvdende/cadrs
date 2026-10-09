//! Rebuilding the Image feature (`crate::picture`): its rectangle filled with its plane into a
//! surface part named after the picture's file, carrying the picture (`Solid::images`). A child
//! of `rebuild::kernel_ops`, so it shares its helpers (naming, merging).

use super::*;
use cadrs_kernel::{Curve2, FillCurve, FillSpec, Kernel};
use nalgebra::Point2;

use crate::picture::ImageFeature;

impl Rebuilder {
    /// The Image feature.
    pub(in crate::rebuild) fn picture(&mut self, id: FeatureId, x: &ImageFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let frame = x.plane.frame();
        let plane = cadrs_kernel::Plane {
            origin: Point3::from(frame.origin),
            x_dir: Unit::new_normalize(Vector3::from(frame.u)),
            normal: Unit::new_normalize(Vector3::from(frame.normal())),
        };
        let (cx, cy, hw, hh) = (x.center[0], x.center[1], x.width / 2.0, x.height / 2.0);
        let corners = [Point2::new(cx - hw, cy - hh), Point2::new(cx + hw, cy - hh), Point2::new(cx + hw, cy + hh), Point2::new(cx - hw, cy + hh)];
        let curves = (0..4)
            .map(|i| FillCurve::Sketch { plane, curve: Curve2::Line { a: corners[i], b: corners[(i + 1) % 4], source: Some(i as u64) } })
            .collect();
        let op = id.0;
        let r = self.kernel.fill(&FillSpec { curves, source: op.as_u128() as u64 }).map_err(|e| format!("The image failed: {e}"))?;
        let tool = self.name_new(state, op, &r).map_err(|e| format!("The image failed: {e}"))?;
        let merge = Merge { op: BooleanOp::New, merge_all: false, scope: &[], surface: true };
        let geoms = state.geoms.clone();
        let mut out = self.combine(id, &merge, tool, state, geoms)?;
        // The new surface is named after the file and shows the picture.
        let st = Arc::make_mut(&mut out.state);
        for p in st.parts.iter_mut().filter(|p| p.part.feature == id) {
            p.part.name = x.stem();
            Arc::make_mut(&mut p.part.solid).images = vec![x.placement()];
        }
        Ok(out)
    }
}
