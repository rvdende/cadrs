//! The features of P3.7 in the app: **Plane** (PS12), **Sweep** (PS19), **Loft** (PS20) and
//! **Split** (PS18.5). They run in the applied features' session ([`crate::applied`]: the
//! toolbar button inserts "Plane 1" (…) and opens its dialog, one selection field at a time
//! takes the view's picks, ✓/✕, one undo step), with their dialogs in
//! [`crate::advanced_dialog`]. Also **Create selection** (X12): Edges → Tangent connected fills
//! the active field with an edge's tangent-connected chain.
//!
//! What a click adds, by field:
//! - a Plane's *Entities*: default planes and Plane features, planar faces, edges, vertices,
//!   sketch points and curves, the origin;
//! - a Sweep's *Faces and sketch regions to sweep*: regions, planar faces, a whole sketch
//!   (picked in the feature list); its *Sweep path*: part edges, sketch curves, a whole sketch;
//! - a Loft's *Profiles*, in the order picked: regions (a region of a sketch already listed joins
//!   that profile: "Faces of Sketch 2"), a whole sketch, planar faces, a sketch point or vertex
//!   (first or last);
//! - a Split's *Parts to split* (any face or edge of a part) and *Entity to split with* (a plane,
//!   a Plane feature, a face, a sketch).

use bevy::prelude::*;
use cadrs_core::advanced::{LoftFeature, LoftProfile, PathRef, SplitFeature, SplitToolRef, SweepFeature};
use cadrs_core::document::{DirectionRef, EdgeRef, FaceRef, RegionRef, VertexRef};
use cadrs_core::plane::{PlaneEntity, PlaneFeature};
use cadrs_core::{Feature, FeatureId, FeatureKind, PartId};
use cadrs_sketch::PlaneRef;
use cadrs_sketch::units::LengthUnit;

use crate::ActiveDocument;
use crate::applied::{AppliedField, AppliedKind, entity_of};
use crate::parts::PartCache;
use crate::viewport::Pick;

/// A plane the pick stands for: a default plane or a Plane feature.
fn plane_of(features: &[Feature], pick: Pick) -> Option<PlaneRef> {
    match pick {
        Pick::Plane(k) => Some(k.plane_ref()),
        Pick::Feature(f) => cadrs_core::parts::plane_feature_ref(features, f),
        _ => None,
    }
}

/// True if `f` is a sketch of the features.
fn is_sketch(features: &[Feature], f: FeatureId) -> bool {
    features.iter().any(|x| x.id == f && x.sketch().is_some())
}

/// A Plane feature's entity for a pick.
fn plane_entity(features: &[Feature], cache: &PartCache, pick: Pick) -> Option<PlaneEntity> {
    Some(match pick {
        Pick::Plane(_) | Pick::Feature(_) => PlaneEntity::Plane(plane_of(features, pick)?),
        Pick::Origin => PlaneEntity::Origin,
        Pick::Face(..) | Pick::Edge(..) => match entity_of(cache, pick)? {
            cadrs_core::applied::EdgeOrFace::Face(f) => PlaneEntity::Face(f),
            cadrs_core::applied::EdgeOrFace::Edge(e) => PlaneEntity::Edge(e),
        },
        Pick::Vertex(part, vertex) => {
            let point = cache.part(part)?.solid.vertex(&vertex)?.point;
            PlaneEntity::Vertex(VertexRef { part, vertex, point })
        }
        Pick::SketchPoint(sketch, point) => PlaneEntity::SketchPoint { sketch, point },
        Pick::SketchCurve(sketch, curve) => PlaneEntity::SketchCurve { sketch, curve },
        _ => return None,
    })
}

/// The region a pick stands for, as a feature refers to it.
fn region_of(cache: &PartCache, sketch: FeatureId, index: u32) -> Option<RegionRef> {
    let r = cache.sketch_regions(sketch)?.regions.get(index as usize)?.clone();
    Some(RegionRef::new(sketch, &r))
}

/// The same region (the same boundary curves around the same seed).
fn same_region(cache: &PartCache, a: &RegionRef, b: &RegionRef) -> bool {
    if a.sketch != b.sketch || a.curves != b.curves {
        return false;
    }
    let contains = cache
        .sketch_regions(a.sketch)
        .and_then(|sr| sr.regions.iter().find(|r| r.contains(b.seed) && RegionRef::new(a.sketch, r).curves == a.curves))
        .is_some_and(|r| r.contains(a.seed));
    contains || a.seed == b.seed
}

/// The pick that shows a region reference in the view.
fn region_pick(cache: &PartCache, r: &RegionRef) -> Option<Pick> {
    let sr = cache.sketch_regions(r.sketch)?;
    let i = sr.regions.iter().position(|x| x.contains(r.seed) && RegionRef::new(r.sketch, x).curves == r.curves)?;
    Some(Pick::Region(r.sketch, i as u32))
}

fn face_ref(cache: &PartCache, pick: Pick) -> Option<FaceRef> {
    match entity_of(cache, pick)? {
        cadrs_core::applied::EdgeOrFace::Face(f) => Some(f),
        _ => None,
    }
}

fn edge_ref(cache: &PartCache, pick: Pick) -> Option<EdgeRef> {
    match entity_of(cache, pick)? {
        cadrs_core::applied::EdgeOrFace::Edge(e) => Some(e),
        _ => None,
    }
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
    if let Some(i) = list.iter().position(|y| *y == x) {
        list.remove(i);
    } else {
        list.push(x);
    }
}

/// Toggles a region in a list of region references.
fn toggle_region(cache: &PartCache, list: &mut Vec<RegionRef>, r: RegionRef) {
    if let Some(i) = list.iter().position(|x| same_region(cache, x, &r)) {
        list.remove(i);
    } else {
        list.push(r);
    }
}

/// A new feature's name, parameters and first field, from what was selected when its button
/// was clicked.
pub fn initial(world: &World, kind: AppliedKind, picked: &[Pick]) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let el = doc.active_element()?;
    let features = el.features();
    let cache = world.resource::<PartCache>();
    let inch = doc.doc.units.length == LengthUnit::Inch;
    Some(match kind {
        AppliedKind::Plane => {
            let entities = picked.iter().filter_map(|p| plane_entity(features, cache, *p)).collect();
            // Onshape's default offset: 1 in in an inch document.
            let (offset, offset_expr) = if inch { (25.4, "1 in".to_string()) } else { (25.0, "25 mm".to_string()) };
            (
                "Plane",
                FeatureKind::Plane(PlaneFeature { entities, offset, offset_expr, ..PlaneFeature::default() }),
                AppliedField::PlaneEntities,
            )
        }
        AppliedKind::Sweep => {
            let mut x = SweepFeature::default();
            for p in picked {
                match *p {
                    Pick::Region(s, i) => x.regions.extend(region_of(cache, s, i)),
                    Pick::Feature(f) if is_sketch(features, f) => x.sketches.push(f),
                    Pick::Face(..) => x.faces.extend(face_ref(cache, *p)),
                    Pick::Edge(..) => x.path.extend(edge_ref(cache, *p).map(PathRef::Edge)),
                    Pick::SketchCurve(sketch, curve) => x.path.push(PathRef::SketchCurve { sketch, curve }),
                    _ => {}
                }
            }
            let field = if !x.regions.is_empty() && x.path.is_empty() { AppliedField::Path } else { AppliedField::Profile };
            ("Sweep", FeatureKind::Sweep(x), field)
        }
        AppliedKind::Loft => {
            let mut x = LoftFeature::default();
            for p in picked {
                add_loft_profile(features, cache, &mut x, *p);
            }
            ("Loft", FeatureKind::Loft(x), AppliedField::Profiles)
        }
        AppliedKind::Split => {
            let mut x = SplitFeature::default();
            for p in picked {
                if let Some(part) = p.part()
                    && !x.parts.contains(&part)
                {
                    x.parts.push(part);
                }
            }
            ("Split", FeatureKind::Split(x), AppliedField::SplitParts)
        }
        _ => return None,
    })
}

/// Adds (or, picked again, removes) a loft profile for a pick. Returns false if the pick isn't
/// one.
fn add_loft_profile(features: &[Feature], cache: &PartCache, x: &mut LoftFeature, pick: Pick) -> bool {
    match pick {
        Pick::Region(s, i) => {
            let Some(r) = region_of(cache, s, i) else { return false };
            // A region of a sketch already listed joins (or leaves) that profile.
            if let Some(k) = x.profiles.iter().position(|p| matches!(p, LoftProfile::Regions { sketch, .. } if *sketch == s)) {
                if let LoftProfile::Regions { regions, .. } = &mut x.profiles[k] {
                    toggle_region(cache, regions, r);
                    if regions.is_empty() {
                        x.profiles.remove(k);
                    }
                }
            } else {
                x.profiles.push(LoftProfile::Regions { sketch: s, regions: vec![r] });
            }
        }
        Pick::Feature(f) if is_sketch(features, f) => toggle(&mut x.profiles, LoftProfile::Sketch(f)),
        Pick::Face(..) => {
            let Some(f) = face_ref(cache, pick) else { return false };
            match x.profiles.iter().position(|p| matches!(p, LoftProfile::Face(g) if g.face == f.face)) {
                Some(i) => {
                    x.profiles.remove(i);
                }
                None => x.profiles.push(LoftProfile::Face(f)),
            }
        }
        Pick::SketchPoint(sketch, point) => toggle(&mut x.profiles, LoftProfile::SketchPoint { sketch, point }),
        Pick::Vertex(part, vertex) => {
            let Some(point) = cache.part(part).and_then(|p| p.solid.vertex(&vertex)).map(|v| v.point) else {
                return false;
            };
            match x.profiles.iter().position(|p| matches!(p, LoftProfile::Vertex(v) if v.vertex == vertex)) {
                Some(i) => {
                    x.profiles.remove(i);
                }
                None => x.profiles.push(LoftProfile::Vertex(VertexRef { part, vertex, point })),
            }
        }
        _ => return false,
    }
    true
}

/// A pick into one of the new features' fields (see the module docs). Returns false if the
/// pick doesn't fit the field.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let Some(features) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element())
        .map(|e| e.features().to_vec())
    else {
        return false;
    };
    let cache = world.resource::<PartCache>();
    match (kind, field) {
        (FeatureKind::Plane(x), AppliedField::PlaneEntities) => {
            let Some(e) = plane_entity(&features, cache, pick) else { return false };
            let same = |a: &PlaneEntity| match (a, &e) {
                (PlaneEntity::Face(a), PlaneEntity::Face(b)) => a.face == b.face,
                (PlaneEntity::Edge(a), PlaneEntity::Edge(b)) => a.edge == b.edge,
                (PlaneEntity::Vertex(a), PlaneEntity::Vertex(b)) => a.vertex == b.vertex,
                (PlaneEntity::Plane(PlaneRef::Feature(a)), PlaneEntity::Plane(PlaneRef::Feature(b))) => a.feature == b.feature,
                (a, b) => a == b,
            };
            match x.entities.iter().position(same) {
                Some(i) => {
                    x.entities.remove(i);
                }
                None => x.entities.push(e),
            }
        }
        (FeatureKind::Sweep(x), AppliedField::Profile) => match pick {
            Pick::Region(s, i) => {
                let Some(r) = region_of(cache, s, i) else { return false };
                toggle_region(cache, &mut x.regions, r);
            }
            Pick::Feature(f) if is_sketch(&features, f) => toggle(&mut x.sketches, f),
            Pick::Face(..) => {
                let Some(f) = face_ref(cache, pick) else { return false };
                match x.faces.iter().position(|g| g.face == f.face) {
                    Some(i) => {
                        x.faces.remove(i);
                    }
                    None => x.faces.push(f),
                }
            }
            _ => return false,
        },
        (FeatureKind::Sweep(x), AppliedField::Path) => match pick {
            Pick::Edge(..) => {
                let Some(e) = edge_ref(cache, pick) else { return false };
                match x.path.iter().position(|p| matches!(p, PathRef::Edge(g) if g.edge == e.edge)) {
                    Some(i) => {
                        x.path.remove(i);
                    }
                    None => x.path.push(PathRef::Edge(e)),
                }
            }
            Pick::SketchCurve(sketch, curve) => toggle(&mut x.path, PathRef::SketchCurve { sketch, curve }),
            Pick::Feature(f) if is_sketch(&features, f) => toggle(&mut x.path, PathRef::Sketch(f)),
            // A curve feature (a Helix) picked in the list or the view.
            Pick::Feature(f) if features.iter().any(|y| y.id == f && matches!(y.kind, FeatureKind::Helix(_))) => {
                toggle(&mut x.path, PathRef::Curve(f))
            }
            _ => return false,
        },
        (FeatureKind::Sweep(x), AppliedField::LockDirection) => {
            let d = match pick {
                Pick::Plane(_) | Pick::Feature(_) => plane_of(&features, pick).map(DirectionRef::PlaneNormal),
                Pick::Face(..) => face_ref(cache, pick).map(DirectionRef::FaceNormal),
                Pick::Edge(..) => edge_ref(cache, pick).map(DirectionRef::Edge),
                Pick::SketchCurve(sketch, curve) => Some(DirectionRef::SketchLine { sketch, curve }),
                _ => None,
            };
            let Some(d) = d else { return false };
            x.lock_direction = if x.lock_direction == Some(d) { None } else { Some(d) };
        }
        // P3.11 (PS20.4): a loft end's picked direction, as the pattern's directions take them.
        (FeatureKind::Loft(x), AppliedField::LoftStartDirection | AppliedField::LoftEndDirection) => {
            let Some(d) = crate::pattern::direction_of(&features, cache, pick) else { return false };
            let slot = if field == AppliedField::LoftStartDirection { &mut x.start_direction } else { &mut x.end_direction };
            *slot = if *slot == Some(d) { None } else { Some(d) };
        }
        (FeatureKind::Sweep(x), AppliedField::MergeScope) => {
            let Some(part) = pick.part() else { return false };
            toggle(&mut x.merge_scope, part);
        }
        (FeatureKind::Loft(x), AppliedField::Profiles) => {
            if !add_loft_profile(&features, cache, x, pick) {
                return false;
            }
        }
        (FeatureKind::Loft(x), AppliedField::MergeScope) => {
            let Some(part) = pick.part() else { return false };
            toggle(&mut x.merge_scope, part);
        }
        (FeatureKind::Split(x), AppliedField::SplitParts) if x.split_type == cadrs_core::advanced::SplitType::Face => {
            let Some(f) = face_ref(cache, pick) else { return false };
            match x.faces.iter().position(|g| g.part == f.part && g.face == f.face) {
                Some(i) => {
                    x.faces.remove(i);
                }
                None => x.faces.push(f),
            }
        }
        (FeatureKind::Split(x), AppliedField::SplitParts) => {
            let Some(part) = pick.part() else { return false };
            toggle(&mut x.parts, part);
        }
        (FeatureKind::Split(x), AppliedField::SplitTool) => {
            let t = match pick {
                Pick::Plane(_) => plane_of(&features, pick).map(SplitToolRef::Plane),
                Pick::Feature(f) if is_sketch(&features, f) => Some(SplitToolRef::Sketch(f)),
                Pick::Feature(_) => plane_of(&features, pick).map(SplitToolRef::Plane),
                Pick::Face(..) => face_ref(cache, pick).map(SplitToolRef::Face),
                _ => None,
            };
            let Some(t) = t else { return false };
            x.tool = if x.tool == Some(t) { None } else { Some(t) };
        }
        _ => return false,
    }
    true
}

/// The pick that shows a face reference in the view (on the part where it is now).
fn face_pick(cache: &PartCache, f: &FaceRef) -> Option<Pick> {
    cache.parts.iter().find(|p| p.solid.face(&f.face).is_some()).map(|p| Pick::Face(p.id, f.face))
}

fn edge_pick(cache: &PartCache, e: &EdgeRef) -> Option<Pick> {
    cache.parts.iter().find(|p| p.solid.edge(&e.edge).is_some()).map(|p| Pick::Edge(p.id, e.edge))
}

/// What a new feature's fields refer to, shown selected in the view while its dialog is open.
pub fn references(kind: &FeatureKind, cache: &PartCache, features: &[Feature]) -> Vec<Pick> {
    let mut out = Vec::new();
    // A whole sketch: its regions.
    let whole = |s: FeatureId, out: &mut Vec<Pick>| {
        let Some(g) = features.iter().find(|f| f.id == s).and_then(|f| f.sketch()) else { return };
        for r in cadrs_core::rebuild::whole_sketch_regions(&g.geometry) {
            out.extend(region_pick(cache, &RegionRef::new(s, &r)));
        }
    };
    match kind {
        FeatureKind::Plane(x) => {
            for e in &x.entities {
                out.extend(match e {
                    PlaneEntity::Plane(p) => crate::viewport::plane_pick(*p),
                    PlaneEntity::Face(f) => face_pick(cache, f),
                    PlaneEntity::Edge(r) => edge_pick(cache, r),
                    PlaneEntity::Vertex(v) => cache
                        .parts
                        .iter()
                        .find(|p| p.solid.vertex(&v.vertex).is_some())
                        .map(|p| Pick::Vertex(p.id, v.vertex)),
                    PlaneEntity::SketchPoint { sketch, point } => Some(Pick::SketchPoint(*sketch, *point)),
                    PlaneEntity::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
                    PlaneEntity::Origin => Some(Pick::Origin),
                });
            }
        }
        FeatureKind::Sweep(x) => {
            out.extend(x.regions.iter().filter_map(|r| region_pick(cache, r)));
            for s in &x.sketches {
                whole(*s, &mut out);
            }
            out.extend(x.faces.iter().filter_map(|f| face_pick(cache, f)));
            for p in &x.path {
                out.extend(match p {
                    PathRef::Edge(e) => edge_pick(cache, e),
                    PathRef::SketchCurve { sketch, curve } => Some(Pick::SketchCurve(*sketch, *curve)),
                    PathRef::Sketch(_) => None,
                    PathRef::Curve(f) => Some(Pick::Feature(*f)),
                });
            }
        }
        FeatureKind::Loft(x) => {
            for p in &x.profiles {
                match p {
                    LoftProfile::Regions { regions, .. } => out.extend(regions.iter().filter_map(|r| region_pick(cache, r))),
                    LoftProfile::Face(f) => out.extend(face_pick(cache, f)),
                    LoftProfile::SketchPoint { sketch, point } => out.push(Pick::SketchPoint(*sketch, *point)),
                    LoftProfile::Vertex(v) => out.extend(
                        cache
                            .parts
                            .iter()
                            .find(|q| q.solid.vertex(&v.vertex).is_some())
                            .map(|q| Pick::Vertex(q.id, v.vertex)),
                    ),
                    LoftProfile::Sketch(s) => whole(*s, &mut out),
                }
            }
        }
        FeatureKind::Split(x) => {
            match x.split_type {
                cadrs_core::advanced::SplitType::Part => out.extend(x.parts.iter().map(|p| Pick::Part(*p))),
                // The faces split: every piece of each (the same name but for the split index).
                cadrs_core::advanced::SplitType::Face => {
                    for f in &x.faces {
                        for p in &cache.parts {
                            out.extend(
                                p.solid
                                    .faces
                                    .iter()
                                    .filter(|g| g.name.op == f.face.op && g.name.origin == f.face.origin)
                                    .map(|g| Pick::Face(p.id, g.name)),
                            );
                        }
                    }
                }
            }
            match &x.tool {
                Some(SplitToolRef::Plane(p)) => out.extend(crate::viewport::plane_pick(*p)),
                Some(SplitToolRef::Face(f)) => out.extend(face_pick(cache, f)),
                _ => {}
            }
        }
        _ => {}
    }
    out
}

/// Create selection → Edges: Tangent connected (X12): the edge and every edge of its part
/// tangent-connected to it (the kernel's exact tangents; the display mesh's where a part has
/// none).
pub fn tangent_connected(cache: &PartCache, part: PartId, edge: cadrs_sketch::EdgeName) -> Vec<cadrs_sketch::EdgeName> {
    let Some(p) = cache.part(part) else { return vec![edge] };
    match p.solid.edge(&edge).and_then(|e| e.tangent_group) {
        Some(g) => p.solid.edges.iter().filter(|e| e.tangent_group == Some(g)).map(|e| e.name).collect(),
        None => crate::applied::tangent_chain(&p.solid, &edge),
    }
}
