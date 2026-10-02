//! Modelling in the flat (P3I.6, SM14; `reference/onshape/sheetmetal/raw/`
//! `help-sheet_metal_table.txt`, "Sketching on a flat pattern" and "Extruding a flat pattern
//! sketch"; lesson `14-modeling-in-the-flat-view`):
//!
//! - **New sketch** on the flat pattern ([`begin_flat_sketch`]): a sketch whose plane is the
//!   model's flat pattern plane (`cadrs_core::sheetmetal_flat`), so its coordinates are the
//!   flat's. Until the flat view (P3I.3) exists, the plane shows over the folded part's anchor
//!   wall in the 3D view.
//! - The **abbreviated Extrude** (`help/feature-tools/extrude2_abbrev_dialogbox.png`): **Add |
//!   Remove** and *Faces and sketch regions to extrude*, in the applied-feature dialogs. Extrude
//!   (the toolbar button or Shift+E) with regions of a flat-pattern sketch selected opens it
//!   instead of the 3D Extrude ([`extrude_redirect`]). A new one starts on Remove when all its
//!   regions lie on material, else on Add.
//!
//! Names: `flat-extrude-dialog`, `flat-extrude-operation-0` (Add) / `-1` (Remove),
//! `flat-extrude-regions-field`.

use bevy::prelude::*;
use cadrs_core::document::{BodyType, RegionRef, interior_point};
use cadrs_core::sheetmetal_flat::{FlatExtrudeFeature, flat_plane_id, sketch_target};
use cadrs_core::{Feature, FeatureId, FeatureKind};
use cadrs_ui::prelude::*;
use cadrs_ui::TabStrip;

use crate::ActiveDocument;
use crate::applied::{AppliedField, AppliedKind};
use crate::applied_dialog::{Role, body_column, list};
use crate::parts::PartCache;
use crate::viewport::{Pick, Selection};

fn region_of(cache: &PartCache, sketch: FeatureId, index: u32) -> Option<RegionRef> {
    let r = cache.sketch_regions(sketch)?.regions.get(index as usize)?.clone();
    Some(RegionRef::new(sketch, &r))
}

fn features(world: &World) -> Option<Vec<Feature>> {
    Some(world.get_resource::<ActiveDocument>()?.active_element()?.features().to_vec())
}

/// Whether every picked region lies on the flat's material (a new flat extrude then starts on
/// Remove).
fn all_on_material(features: &[Feature], cache: &PartCache, picks: &[(FeatureId, u32)]) -> bool {
    let build = cadrs_core::rebuild::build(features);
    !picks.is_empty()
        && picks.iter().all(|(s, i)| {
            let Some((model, part)) = sketch_target(features, *s) else { return false };
            let Some(r) = cache.sketch_regions(*s).and_then(|r| r.regions.get(*i as usize)) else { return false };
            let p = interior_point(r);
            build
                .sheet_metal
                .iter()
                .rev()
                .find(|c| c.feature == model)
                .and_then(|c| c.flat.parts.get(part))
                .is_some_and(|f| f.outline.iter().any(|o| o.contains(cadrs_sheetmetal::poly::P2::new(p.x, p.y))))
        })
}

/// A new flat extrude from the selection: its regions (and whole sketches).
pub fn initial(world: &World, picked: &[Pick]) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let features = features(world)?;
    let cache = world.resource::<PartCache>();
    let mut x = FlatExtrudeFeature::default();
    let mut regions = Vec::new();
    for p in picked {
        match *p {
            Pick::Region(s, i) if sketch_target(&features, s).is_some() => {
                regions.push((s, i));
                x.regions.extend(region_of(cache, s, i));
            }
            Pick::Feature(f) if sketch_target(&features, f).is_some() => x.sketches.push(f),
            _ => {}
        }
    }
    x.remove = all_on_material(&features, cache, &regions);
    Some(("Extrude", FeatureKind::FlatExtrude(x), AppliedField::FlatRegions))
}

/// Extrude with regions or sketches of a flat-pattern sketch selected: the abbreviated flat
/// extrude instead (true if it started).
pub fn extrude_redirect(world: &mut World) -> bool {
    let Some(features) = features(world) else { return false };
    let flat = world.resource::<Selection>().0.iter().any(|p| match *p {
        Pick::Region(s, _) | Pick::Feature(s) => sketch_target(&features, s).is_some(),
        _ => false,
    });
    if flat {
        crate::applied::begin(world, AppliedKind::FlatExtrude);
    }
    flat
}

/// A pick into the regions field. False if it doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let (FeatureKind::FlatExtrude(x), AppliedField::FlatRegions) = (kind, field) else { return false };
    let Some(features) = features(world) else { return false };
    let cache = world.resource::<PartCache>();
    match pick {
        Pick::Region(s, i) => {
            let Some(r) = region_of(cache, s, i) else { return false };
            match x.regions.iter().position(|y| y.sketch == r.sketch && y.curves == r.curves) {
                Some(k) => {
                    x.regions.remove(k);
                }
                None => x.regions.push(r),
            }
        }
        Pick::Feature(f) if features.iter().any(|g| g.id == f && g.sketch().is_some()) => match x.sketches.iter().position(|s| *s == f) {
            Some(k) => {
                x.sketches.remove(k);
            }
            None => x.sketches.push(f),
        },
        _ => return false,
    }
    true
}

// ---------------------------------------------------------------------------------------------
// The dialog (called at the end of the applied dialogs' chain, from `crate::surfacing_ui`)

pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    matches!(kind, FeatureKind::FlatExtrude(_)).then_some("flat-extrude")
}

pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    match kind {
        FeatureKind::FlatExtrude(x) => Some(format!("flat-extrude {}", x.remove)),
        _ => None,
    }
}

pub(crate) fn items(features: &[Feature], kind: &FeatureKind, role: Role) -> Option<Vec<String>> {
    match (kind, role) {
        (FeatureKind::FlatExtrude(x), Role::FlatRegions) => {
            let name = |id: FeatureId| features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone());
            let mut v: Vec<String> = x.regions.iter().map(|r| format!("Face of {}", name(r.sketch))).collect();
            v.extend(x.sketches.iter().map(|s| crate::extrude_dialog::whole_sketch_label(features, *s, BodyType::Solid)));
            Some(v)
        }
        _ => None,
    }
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    (role == Role::FlatRegions).then_some(AppliedField::FlatRegions)
}

/// Add | Remove, then the regions.
pub(crate) fn body(b: &mut ChildSpawner, t: &Theme, kind: &FeatureKind, field: AppliedField, items_of: &dyn Fn(Role) -> Vec<String>) {
    let FeatureKind::FlatExtrude(x) = kind else { return };
    b.spawn((Role::OpTab, TabStrip::new("flat-extrude-operation").compact().tab("Add").tab("Remove").selected(usize::from(x.remove)).build(t)));
    body_column(b, |b| {
        list(b, t, "flat-extrude-regions-field", "Faces and sketch regions to extrude", Role::FlatRegions, items_of(Role::FlatRegions), field == AppliedField::FlatRegions);
    });
}

pub(crate) fn tab(k: &mut FeatureKind, role: Role, i: usize) {
    if let (FeatureKind::FlatExtrude(x), Role::OpTab) = (k, role) {
        x.remove = i == 1;
    }
}

pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    let (FeatureKind::FlatExtrude(x), Role::FlatRegions) = (k, role) else { return };
    if i < x.regions.len() {
        x.regions.remove(i);
    } else if i - x.regions.len() < x.sketches.len() {
        x.sketches.remove(i - x.regions.len());
    }
}

// ---------------------------------------------------------------------------------------------
// New sketch on the flat pattern

/// The plane of a model's flat-pattern part as the last rebuild has it.
pub fn flat_plane(world: &World, model: FeatureId, part: usize) -> Option<cadrs_sketch::PlaneRef> {
    let id = flat_plane_id(model, part);
    let frame = *world.resource::<PartCache>().planes.get(&FeatureId(id))?;
    Some(cadrs_sketch::PlaneRef::Feature(cadrs_sketch::FeaturePlane::new(id, frame)))
}

/// New sketch on the flat pattern of `model`'s part `part` (SM14.1).
pub fn begin_flat_sketch(world: &mut World, model: FeatureId, part: usize) {
    let Some(plane) = flat_plane(world, model, part) else {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        cadrs_ui::show_notification(&mut commands, &theme, cadrs_ui::Notification::warning("The sheet metal model has no flat pattern to sketch on").name("flat-sketch-toast"));
        world.flush();
        return;
    };
    crate::sketch::begin_sketch_on(world, Some(plane));
}
