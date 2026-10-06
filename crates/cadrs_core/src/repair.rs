//! P3D.4: Replace reference and the Repair view's references (IR3.6, IR4, X5).
//!
//! A feature's main selection list ("Faces and sketch regions to extrude", a fillet's "Edges
//! or faces") holds [`Reference`]s, in the order [`crate::rebuild::Build::missing`] numbers
//! them. [`ReplaceReference`] swaps one of them for another and, with **Propagate changes**
//! (on by default, IR4.4), every use of the same reference by the features after it: the same
//! sketch region (sketch and boundary curves), the same whole sketch, or the same persistent
//! face or edge name.
//!
//! [`outline`] is where a reference was in a past state of the Part Studio (the Repair view's
//! highlight, IR3.6); [`tangent_chain`] and [`chain_check`] are the pro-tip's check (IR4.7): a
//! replacement edge whose tangent chain is shorter than the missing one's fillets fewer edges.

use cadrs_sketch::Vec3;

use crate::applied::EdgeOrFace;
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, EdgeRef, FaceRef, Feature, FeatureKind, RegionRef};
use crate::ids::{ElementId, FeatureId};
use crate::parts::Part;

/// One item of a feature's main selection list.
#[derive(Debug, Clone, PartialEq)]
pub enum Reference {
    /// A sketch region ("Face of Sketch 3").
    Region(RegionRef),
    /// A whole sketch.
    Sketch(FeatureId),
    /// A part face ("Face of Extrude 5").
    Face(FaceRef),
    /// A part edge ("Edge of Extrude 5").
    Edge(EdgeRef),
}

impl Reference {
    /// True if `other` is a use of the same thing: the same sketch region (the same sketch and
    /// boundary curves), the same sketch, or the same persistent name.
    pub fn same(&self, other: &Reference) -> bool {
        match (self, other) {
            (Reference::Region(a), Reference::Region(b)) => a.sketch == b.sketch && a.curves == b.curves,
            (Reference::Sketch(a), Reference::Sketch(b)) => a == b,
            (Reference::Face(a), Reference::Face(b)) => a.face == b.face,
            (Reference::Edge(a), Reference::Edge(b)) => a.edge == b.edge,
            _ => false,
        }
    }

    /// "Face of Sketch 3", "Edge of Extrude 5": its kind and the feature that made it (by name
    /// in `features`).
    pub fn label(&self, features: &[Feature]) -> String {
        let name = |id: FeatureId| features.iter().find(|f| f.id == id).map(|f| f.name.clone()).unwrap_or_default();
        match self {
            Reference::Region(r) => format!("Face of {}", name(r.sketch)),
            Reference::Sketch(s) => name(*s),
            Reference::Face(f) => format!("Face of {}", name(FeatureId(f.face.op))),
            Reference::Edge(e) => format!("Edge of {}", name(FeatureId(e.edge.op()))),
        }
    }
}

/// The feature's main selection list, in [`crate::rebuild::Build::missing`]'s order: an
/// extrude's or revolve's regions, whole sketches, then faces; a fillet's or chamfer's edges
/// and faces. Empty for other features.
pub fn references(f: &Feature) -> Vec<Reference> {
    let mut out = Vec::new();
    let mut inputs = |regions: &[RegionRef], sketches: &[FeatureId], faces: &[FaceRef]| {
        out.extend(regions.iter().cloned().map(Reference::Region));
        out.extend(sketches.iter().copied().map(Reference::Sketch));
        out.extend(faces.iter().copied().map(Reference::Face));
    };
    match &f.kind {
        FeatureKind::Extrude(e) => inputs(&e.regions, &e.sketches, &e.faces),
        FeatureKind::Revolve(r) => inputs(&r.regions, &r.sketches, &r.faces),
        FeatureKind::Fillet(x) => out.extend(x.entities.iter().map(entity)),
        FeatureKind::Chamfer(x) => out.extend(x.entities.iter().map(entity)),
        _ => {}
    }
    out
}

fn entity(e: &EdgeOrFace) -> Reference {
    match e {
        EdgeOrFace::Edge(r) => Reference::Edge(*r),
        EdgeOrFace::Face(f) => Reference::Face(*f),
    }
}

/// Puts `refs` back into the feature's selection list (see [`references`]). Items a list can't
/// hold are dropped (an edge in an extrude's faces). Returns false for a feature without one.
pub fn set_references(f: &mut Feature, refs: &[Reference]) -> bool {
    let split = |refs: &[Reference]| {
        let mut regions = Vec::new();
        let mut sketches = Vec::new();
        let mut faces = Vec::new();
        for r in refs {
            match r {
                Reference::Region(x) => regions.push(x.clone()),
                Reference::Sketch(x) => sketches.push(*x),
                Reference::Face(x) => faces.push(*x),
                Reference::Edge(_) => {}
            }
        }
        (regions, sketches, faces)
    };
    let entities = |refs: &[Reference]| {
        refs.iter()
            .filter_map(|r| match r {
                Reference::Edge(e) => Some(EdgeOrFace::Edge(*e)),
                Reference::Face(x) => Some(EdgeOrFace::Face(*x)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    match &mut f.kind {
        FeatureKind::Extrude(e) => (e.regions, e.sketches, e.faces) = split(refs),
        FeatureKind::Revolve(r) => (r.regions, r.sketches, r.faces) = split(refs),
        FeatureKind::Fillet(x) => x.entities = entities(refs),
        FeatureKind::Chamfer(x) => x.entities = entities(refs),
        _ => return false,
    }
    true
}

/// Replaces the `index`th reference of feature `feature` in `features` by `with`, and with
/// `propagate` every use of the same reference by the features after it. Returns the features
/// changed (in list order).
pub fn replace(features: &mut [Feature], feature: FeatureId, index: usize, with: &Reference, propagate: bool) -> Result<Vec<FeatureId>, String> {
    let at = features.iter().position(|f| f.id == feature).ok_or("feature not found")?;
    let refs = references(&features[at]);
    let old = refs.get(index).cloned().ok_or("no such reference")?;
    let mut changed = Vec::new();
    for (i, f) in features.iter_mut().enumerate().skip(at) {
        if i > at && !propagate {
            break;
        }
        let mut refs = references(f);
        let mut hit = false;
        for (j, r) in refs.iter_mut().enumerate() {
            let this = if i == at { j == index } else { r.same(&old) };
            if this {
                *r = with.clone();
                hit = true;
            }
        }
        if !hit {
            continue;
        }
        // A replacement already in the list isn't listed twice.
        let mut unique: Vec<Reference> = Vec::new();
        for r in refs {
            if !unique.iter().any(|u| u.same(&r)) {
                unique.push(r);
            }
        }
        if set_references(f, &unique) {
            changed.push(f.id);
        }
    }
    Ok(changed)
}

/// Replace reference (IR4.1–IR4.5): swaps the `index`th reference of a feature for `with`,
/// and with `propagate` in every feature after it that used the same one. One undo step.
#[derive(Debug, Clone)]
pub struct ReplaceReference {
    pub element: ElementId,
    pub feature: FeatureId,
    pub index: usize,
    pub with: Reference,
    pub propagate: bool,
}

impl Command for ReplaceReference {
    fn label(&self) -> String {
        "Replace reference".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let features = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?
            .features_mut()
            .ok_or_else(|| CommandError::Invalid("not a Part Studio".into()))?;
        let changed = replace(features, self.feature, self.index, &self.with, self.propagate).map_err(CommandError::Invalid)?;
        if changed.is_empty() {
            return Err(CommandError::Invalid("nothing to replace".into()));
        }
        crate::parts::refresh_face_planes(features);
        Ok(())
    }
}

/// Where a reference was in a state of the Part Studio (its `features`, and `parts`: the parts
/// the features before the one holding the reference made), as polylines in model space: a region's or sketch's outlines, a face's boundary
/// loops, an edge (and, with `tangent`, the rest of its tangent chain).
pub fn outline(features: &[Feature], parts: &[Part], r: &Reference, tangent: bool) -> Vec<Vec<Vec3>> {
    let sketch = |id: FeatureId| features.iter().find(|f| f.id == id).and_then(|f| f.sketch());
    let closed = |pts: &[cadrs_sketch::Vec2], frame: &cadrs_sketch::PlaneFrame| {
        let mut v: Vec<Vec3> = pts.iter().map(|p| frame.to_world(*p)).collect();
        if let Some(first) = v.first().copied() {
            v.push(first);
        }
        v
    };
    match r {
        Reference::Region(rr) => {
            let Some(sk) = sketch(rr.sketch) else { return Vec::new() };
            let (Some(plane), Some(region)) = (sk.plane, rr.resolve(&sk.geometry)) else { return Vec::new() };
            let frame = plane.frame();
            std::iter::once(&region.outer).chain(region.holes.iter()).map(|l| closed(l, &frame)).collect()
        }
        Reference::Sketch(s) => {
            let Some(sk) = sketch(*s) else { return Vec::new() };
            let Some(plane) = sk.plane else { return Vec::new() };
            let frame = plane.frame();
            crate::rebuild::whole_sketch_regions(&sk.geometry).iter().map(|g| closed(&g.outer, &frame)).collect()
        }
        Reference::Face(f) => parts
            .iter()
            .find_map(|p| p.solid.face(&f.face))
            .map(|face| {
                face.loops
                    .iter()
                    .map(|l| {
                        let mut v = l.clone();
                        if let Some(first) = v.first().copied() {
                            v.push(first);
                        }
                        v
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Reference::Edge(e) => {
            for p in parts {
                if p.solid.edge(&e.edge).is_none() {
                    continue;
                }
                let names = if tangent { tangent_chain(&p.solid, &e.edge) } else { vec![e.edge] };
                return names.iter().filter_map(|n| p.solid.edge(n)).map(|x| x.points.clone()).collect();
            }
            Vec::new()
        }
    }
}

/// The edges of `edge`'s tangent chain (P3.7's tangent groups): the edges meeting it end to
/// end with parallel tangents, itself included; just the edge without group information.
pub fn tangent_chain(solid: &crate::solid::Solid, edge: &cadrs_sketch::EdgeName) -> Vec<cadrs_sketch::EdgeName> {
    let Some(e) = solid.edge(edge) else { return Vec::new() };
    match e.tangent_group {
        Some(g) => solid.edges.iter().filter(|x| x.tangent_group == Some(g)).map(|x| x.name).collect(),
        None => vec![e.name],
    }
}

/// The edges a fillet or chamfer entity stands for with tangent propagation: an edge's
/// tangent chain, or a face's edges.
pub fn entity_edges(parts: &[Part], r: &Reference) -> usize {
    for p in parts {
        match r {
            Reference::Edge(e) if p.solid.edge(&e.edge).is_some() => return tangent_chain(&p.solid, &e.edge).len(),
            Reference::Face(f) if p.solid.face(&f.face).is_some() => return p.solid.face_edges(&f.face).len(),
            _ => {}
        }
    }
    0
}

/// IR4.7 (the step 16 pro-tip): with tangent propagation a missing edge stood for its whole
/// tangent chain (`old` edges, counted in the state Repair shows); a replacement that stands
/// for fewer edges (`new`: an edge whose own chain is shorter, the new corners not being
/// tangent) fillets less than before. The note the Replace reference dialog shows, if any.
pub fn chain_check(old: usize, new: usize, replacement_is_edge: bool) -> Option<String> {
    if old <= 1 || new >= old || new == 0 {
        return None;
    }
    let tip = if replacement_is_edge { " Its edges aren't tangent-connected: pick the face to take all of them." } else { "" };
    Some(format!("The replacement stands for {new} of the {old} edges the missing reference's tangent chain had.{tip}"))
}

/// P3D.4 (IR6.10): the outer loop of the face a sketch lies on, as exact curves in the
/// sketch's plane with their links (what [`cadrs_sketch::SketchOp::OffsetLoop`] takes):
/// `before` are the features before the sketch and `parts` the parts they made. Empty if the
/// sketch isn't on a part face.
pub fn face_loop(before: &[Feature], parts: &[Part], plane: &cadrs_sketch::PlaneRef) -> Vec<(cadrs_sketch::projection::Projected, cadrs_sketch::Link)> {
    let cadrs_sketch::PlaneRef::Face(fp) = plane else { return Vec::new() };
    let frame = plane.frame();
    let solids: Vec<(FeatureId, &crate::solid::Solid)> = parts.iter().map(|p| (p.feature, &*p.solid)).collect();
    let ctx = crate::links::LinkContext { solids: solids.clone(), features: before };
    for (feature, solid) in &solids {
        let Some(face) = solid.face(&fp.face) else { continue };
        if face.plane.is_none() {
            continue;
        }
        let items: Vec<_> = solid
            .face_edges(&fp.face)
            .into_iter()
            .filter_map(|edge| {
                let link = cadrs_sketch::Link::Edge { feature: feature.0, edge };
                Some((ctx.shape(link, &frame)?, link))
            })
            .collect();
        return cadrs_sketch::face_offset::outer_loop(&items);
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extrude(id: u128, regions: Vec<RegionRef>) -> Feature {
        Feature {
            id: FeatureId::from_u128(id),
            name: format!("Extrude {id}"),
            kind: FeatureKind::Extrude(crate::document::ExtrudeFeature { regions, ..Default::default() }),
            suppress_by: None,
        }
    }

    fn region(sketch: u128, seed: f64) -> RegionRef {
        RegionRef { sketch: FeatureId::from_u128(sketch), curves: vec![], seed: cadrs_sketch::Vec2::new(seed, 0.0) }
    }

    /// IR4.4: Propagate changes replaces the same reference in the features after, and only
    /// there; off, only the feature's own item.
    #[test]
    fn propagate_replaces_every_later_use() {
        let (old, new, other) = (region(7, 1.0), region(8, 2.0), region(9, 3.0));
        let features = vec![
            extrude(1, vec![old.clone()]),
            extrude(2, vec![old.clone(), other.clone()]),
            extrude(3, vec![other.clone()]),
        ];
        let mut f = features.clone();
        let changed = replace(&mut f, FeatureId::from_u128(1), 0, &Reference::Region(new.clone()), true).unwrap();
        assert_eq!(changed, [FeatureId::from_u128(1), FeatureId::from_u128(2)]);
        assert_eq!(references(&f[1]), [Reference::Region(new.clone()), Reference::Region(other.clone())]);
        assert_eq!(f[2], features[2]);
        let mut f = features.clone();
        let changed = replace(&mut f, FeatureId::from_u128(1), 0, &Reference::Region(new.clone()), false).unwrap();
        assert_eq!(changed, [FeatureId::from_u128(1)]);
        assert_eq!(f[1], features[1]);
        // A replacement already in the list isn't listed twice.
        let mut f = features.clone();
        replace(&mut f, FeatureId::from_u128(2), 0, &Reference::Region(other.clone()), false).unwrap();
        assert_eq!(references(&f[1]), [Reference::Region(other)]);
    }

    #[test]
    fn a_shorter_chain_is_flagged() {
        assert!(chain_check(4, 1, true).unwrap().contains("1 of the 4"));
        assert_eq!(chain_check(4, 4, false), None);
        assert_eq!(chain_check(1, 1, true), None);
        assert_eq!(chain_check(4, 0, true), None);
    }
}
