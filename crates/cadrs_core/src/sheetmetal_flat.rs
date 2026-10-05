//! Modelling in the flat view (P3I.6, SM14; `reference/onshape/sheetmetal/raw/`
//! `help-sheet_metal_table.txt`, "Sketching on a flat pattern" and "Extruding a flat pattern
//! sketch"):
//!
//! - **Flat pattern planes**: each part of a Sheet metal model's flat pattern has a sketch plane,
//!   the plane its flat is laid out in. It is a [`PlaneRef::Feature`] whose id is
//!   [`flat_plane_id`] (the model's own id for its first part), so a sketch on it follows the
//!   flat like a sketch on a Plane feature follows the plane, and its 2D coordinates *are* the
//!   flat pattern's. In 3D the plane lies on the part's anchor wall (the wall the flat is laid
//!   out from), so the sketch shows over the folded part where that wall is.
//! - **Flat extrude** ([`FlatExtrudeFeature`], shown as "Extrude" with the abbreviated dialog,
//!   `help/feature-tools/extrude2_abbrev_dialogbox.png`): regions of a flat-pattern sketch
//!   **Add**ed to or **Remove**d from the sheet ([`cadrs_sheetmetal::flat_edit`]); the rebuild
//!   (`rebuild/kernel_ops/sheetmetal_flat.rs`) edits the model's definition, lays it flat again
//!   and refolds its parts.
//! - **SM14.3**: an ordinary Extrude of a flat-pattern sketch fails with Onshape's
//!   [`MODEL_SPACE`] message.

use cadrs_sheetmetal::flat::FlatPart;
use cadrs_sheetmetal::model::Surface;
use cadrs_sheetmetal::Model;
use cadrs_sketch::projection::Projected;
use cadrs_sketch::{PlaneFrame, PlaneRef, Vec2};
use serde::{Deserialize, Serialize};

use crate::document::{Feature, FeatureKind, RegionRef};
use crate::ids::FeatureId;

/// Why an ordinary Extrude can't use a flat-pattern sketch (SM14.3; Onshape's words,
/// `help/feature-tools/extrude2_3dmodel_warning.png`).
pub const MODEL_SPACE: &str = "Feature defined in model space can not reference sheet metal flat geometry";

/// How many flat-pattern parts of one model get a plane.
pub const MAX_PARTS: usize = 64;

/// The id of the flat pattern plane of part `part` of the Sheet metal model `model`: the model's
/// own id for its first part, a derived one for the others.
pub fn flat_plane_id(model: FeatureId, part: usize) -> uuid::Uuid {
    if part == 0 {
        return model.0;
    }
    let mut b = *model.0.as_bytes();
    b[15] ^= part as u8;
    b[14] ^= 0xf1;
    uuid::Uuid::from_bytes(b)
}

/// The Sheet metal model and part whose flat pattern plane `plane` is (among `features`).
pub fn flat_target(features: &[Feature], plane: uuid::Uuid) -> Option<(FeatureId, usize)> {
    features
        .iter()
        .filter(|f| matches!(f.kind, FeatureKind::SheetMetalModel(_)))
        .find_map(|f| (0..MAX_PARTS).find(|k| flat_plane_id(f.id, *k) == plane).map(|k| (f.id, k)))
}

/// The model and flat part a sketch lies on, if it is a flat-pattern sketch.
pub fn sketch_target(features: &[Feature], sketch: FeatureId) -> Option<(FeatureId, usize)> {
    let sk = features.iter().find(|f| f.id == sketch)?.sketch()?;
    match sk.plane? {
        PlaneRef::Feature(fp) => flat_target(features, fp.feature),
        _ => None,
    }
}

/// Whether any of `sketches` is a flat-pattern sketch (SM14.3).
pub fn on_flat(features: &[Feature], sketches: &[FeatureId]) -> bool {
    sketches.iter().any(|s| sketch_target(features, *s).is_some())
}

/// The flat pattern plane of `part` in 3D: on its anchor wall (its first), with the flat's 2D
/// coordinates as the plane's. A part laid out from a rolled wall gets the model's first planar
/// wall's plane.
pub fn flat_frame(model: &Model, part: &FlatPart) -> Option<PlaneFrame> {
    let anchor = cadrs_sheetmetal::flat_edit::anchor(part)?;
    let wall = model.wall(anchor)?;
    let pm = part.placement(anchor)?;
    let Surface::Planar { origin, u, v } = wall.surface else {
        let w = model.walls.iter().find(|w| matches!(w.surface, Surface::Planar { .. }))?;
        let Surface::Planar { origin, u, v } = w.surface else { return None };
        let (u, v) = (u.normalize(), v.normalize());
        return Some(PlaneFrame { origin: [origin.x, origin.y, origin.z], u: [u.x, u.y, u.z], v: [v.x, v.y, v.z] });
    };
    let inv = pm.inverse()?;
    // 3D of a flat point p: origin + [u v] · inv(p).
    let to3 = |q: nalgebra::Vector2<f64>| u * q.x + v * q.y;
    let o = origin + to3(inv.t);
    let fu = to3(inv.m.column(0).into_owned()).normalize();
    let fv = to3(inv.m.column(1).into_owned()).normalize();
    Some(PlaneFrame { origin: [o.x, o.y, o.z], u: [fu.x, fu.y, fu.z], v: [fv.x, fv.y, fv.z] })
}

/// The lines of a flat-pattern part a sketch on it can use (snap to, constrain and dimension
/// against): each bend's centre line (with its joint) and each edge of the outline and its
/// cut-outs (`None`), in the flat's coordinates.
pub fn flat_lines(part: &FlatPart) -> Vec<((Vec2, Vec2), Option<u32>)> {
    let v = |p: cadrs_sheetmetal::poly::P2| Vec2::new(p.x, p.y);
    let mut out: Vec<((Vec2, Vec2), Option<u32>)> = part.bends.iter().map(|b| ((v(b.center.a), v(b.center.b)), Some(b.joint.0))).collect();
    for poly in &part.outline {
        for ring in std::iter::once(&poly.outer).chain(&poly.holes) {
            for (i, a) in ring.iter().enumerate() {
                let b = ring[(i + 1) % ring.len()];
                if (b - *a).norm() > 1e-9 {
                    out.push(((v(*a), v(b)), None));
                }
            }
        }
    }
    out
}

/// Where a flat pattern line a sketch on the flat uses ([`cadrs_sketch::Link::FlatLine`]) lies
/// now: the bend's centre line, or the outline edge nearest `at` (points of the used curve as
/// it was). `None` when the model, part or bend is gone.
pub fn flat_line(contexts: &[crate::sheetmetal::SheetMetalContext], model: FeatureId, part: usize, bend: Option<u32>, at: &[Vec2]) -> Option<Projected> {
    let ctx = contexts.iter().rev().find(|c| c.feature == model)?;
    let flat = ctx.flat.parts.get(part)?;
    let lines = flat_lines(flat);
    let ((a, b), _) = match bend {
        Some(j) => lines.into_iter().find(|(_, k)| *k == Some(j))?,
        None => lines
            .into_iter()
            .filter(|(_, k)| k.is_none())
            .min_by(|x, y| {
                let far = |(a, b): (Vec2, Vec2)| at.iter().map(|p| cadrs_sketch::geom::dist_point_segment(*p, a, b)).fold(0.0, f64::max);
                far(x.0).total_cmp(&far(y.0))
            })?,
    };
    Some(Projected::Line(a, b))
}

/// A flat pattern extrude (SM14.2): regions of a flat-pattern sketch added to the sheet or cut
/// out of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FlatExtrudeFeature {
    /// Remove (else Add).
    #[serde(default)]
    pub remove: bool,
    /// Faces and sketch regions to extrude...
    #[serde(default)]
    pub regions: Vec<RegionRef>,
    /// ...and whole sketches' regions.
    #[serde(default)]
    pub sketches: Vec<FeatureId>,
}

impl FlatExtrudeFeature {
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty() && self.sketches.is_empty()
    }

    pub fn problem(&self) -> Option<&'static str> {
        self.is_empty().then_some("Select faces or sketch regions to extrude")
    }

    /// The sketches its regions come from.
    pub fn sketch_ids(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = Vec::new();
        for s in self.regions.iter().map(|r| r.sketch).chain(self.sketches.iter().copied()) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
        v
    }

    /// The features it refers to (PS11): its sketches.
    pub fn parents(&self) -> Vec<FeatureId> {
        self.sketch_ids()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_ids_name_the_model_and_part() {
        let m = FeatureId::new();
        assert_eq!(flat_plane_id(m, 0), m.0);
        let ids: Vec<uuid::Uuid> = (0..MAX_PARTS).map(|k| flat_plane_id(m, k)).collect();
        for (i, a) in ids.iter().enumerate() {
            assert!(ids[i + 1..].iter().all(|b| b != a));
        }
        let f = Feature {
            id: m,
            name: "Sheet metal model 1".into(),
            kind: FeatureKind::SheetMetalModel(Default::default()),
        };
        assert_eq!(flat_target(std::slice::from_ref(&f), flat_plane_id(m, 3)), Some((m, 3)));
        assert_eq!(flat_target(&[f], uuid::Uuid::new_v4()), None);
    }

    #[test]
    fn the_flat_plane_maps_flat_points_onto_the_anchor_wall() {
        let mut p = cadrs_sheetmetal::Params::default();
        for flip in [false, true] {
            p.flip_direction_up = flip;
            let m = cadrs_sheetmetal::samples::l_bracket(p, true).unwrap();
            let flat = cadrs_sheetmetal::flatten(&m);
            let part = &flat.parts[0];
            let f = flat_frame(&m, part).unwrap();
            // A corner of the base in the flat lands on the wall's corner in 3D.
            let w = m.wall(part.walls[0]).unwrap();
            let q = w.outline.outer[0];
            let at = part.placement(w.id).unwrap().apply(q);
            let mine = cadrs_sketch::PlaneFrame::to_world(&f, cadrs_sketch::Vec2::new(at.x, at.y));
            let want = w.surface.point(q);
            assert!((mine[0] - want.x).abs() < 1e-9 && (mine[1] - want.y).abs() < 1e-9 && (mine[2] - want.z).abs() < 1e-9, "{flip}: {mine:?} {want:?}");
        }
    }
}
