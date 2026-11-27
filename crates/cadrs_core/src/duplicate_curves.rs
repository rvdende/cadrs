//! Sketch curves drawn twice, cleaned up when a document opens (#337).
//!
//! Some DXF files draw a line, arc or circle twice in exactly the same place. The import now
//! leaves the copies out (`crate::dxf_import`) and region finding takes them as one edge
//! ([`cadrs_sketch::region::duplicate_curves`]), but a document imported before still has
//! them: deleting one circle in the sketch showed the other still there. So each sketch keeps
//! one curve of every such set: the one with the most constraints and dimensions on it (the
//! first if they tie), deleted as the sketch's Delete does (with the constraints, dimensions
//! and points only the copies used). What the Part Studio's other features say about a copy
//! (a region's boundary curves, a curve picked as an axis or path, a curve's appearance) is
//! moved to the curve kept.

use std::collections::HashMap;

use cadrs_sketch::{CurveId, Sketch};
use serde_json::Value;

use crate::document::{Document, ElementKind};
use crate::ids::FeatureId;

/// Removes the duplicate curves of every sketch in `doc` (see the module docs). Returns how
/// many it removed.
pub fn remove(doc: &mut Document) -> usize {
    let mut removed = 0;
    for el in &mut doc.elements {
        let ElementKind::PartStudio {
            features,
            curve_appearances,
            ..
        } = &mut el.kind
        else {
            continue;
        };
        for i in 0..features.len() {
            let sketch = features[i].id;
            let Some(s) = features[i].sketch_mut() else {
                continue;
            };
            let moved = dedup_sketch(&mut s.geometry);
            if moved.is_empty() {
                continue;
            }
            removed += moved.len();
            for (j, f) in features.iter_mut().enumerate() {
                if j != i {
                    remap_feature(f, sketch, &moved);
                }
            }
            for (s, c, _) in curve_appearances.iter_mut() {
                if *s == sketch
                    && let Some(to) = moved.get(c)
                {
                    *c = *to;
                }
            }
            // A curve's appearance given twice (the copy's moved onto the kept one's): the first.
            let mut seen = Vec::new();
            curve_appearances.retain(|(s, c, _)| {
                let fresh = !seen.contains(&(*s, *c));
                seen.push((*s, *c));
                fresh
            });
        }
    }
    removed
}

/// Deletes the duplicate curves of `s`: each removed curve with the curve kept in its place.
fn dedup_sketch(s: &mut Sketch) -> HashMap<CurveId, CurveId> {
    let dups = cadrs_sketch::region::duplicate_curves(s);
    if dups.is_empty() {
        return HashMap::new();
    }
    // Each set: the curve region finding kept first, then its copies.
    let mut sets: Vec<Vec<CurveId>> = Vec::new();
    for (copy, of) in dups {
        match sets.iter_mut().find(|set| set[0] == of) {
            Some(set) => set.push(copy),
            None => sets.push(vec![of, copy]),
        }
    }
    let uses = |s: &Sketch, c: CurveId| {
        s.dimensions
            .values()
            .filter(|d| d.kind.uses_curve(c))
            .count()
            + s.constraints.values().filter(|k| k.uses_curve(c)).count()
    };
    let mut moved = HashMap::new();
    for set in sets {
        // The most used; the first of those (max_by_key would take the last).
        let most = set.iter().map(|c| uses(s, *c)).max().unwrap_or(0);
        let keep = set
            .iter()
            .copied()
            .find(|c| uses(s, *c) == most)
            .unwrap_or(set[0]);
        for c in set.into_iter().filter(|c| *c != keep) {
            s.remove_curve(c);
            moved.insert(c, keep);
        }
    }
    moved
}

/// Points `f`'s references to the removed curves of sketch `sketch` at the curves kept. A
/// feature names a sketch curve next to its sketch: `{sketch, curve}` (a picked curve),
/// `{sketch, curves, …}` (a region's boundary, kept sorted and without repeats, as
/// [`crate::document::RegionRef::new`] makes it) or a `(sketch, curve, …)` tuple; those are
/// found in its JSON, so every kind of feature is covered.
fn remap_feature(
    f: &mut crate::document::Feature,
    sketch: FeatureId,
    moved: &HashMap<CurveId, CurveId>,
) {
    let Ok(mut v) = serde_json::to_value(&f.kind) else {
        return;
    };
    let (Ok(Value::String(sketch)), Ok(moved)) = (
        serde_json::to_value(sketch),
        moved
            .iter()
            .map(|(a, b)| Ok((serde_json::to_value(a)?, serde_json::to_value(b)?)))
            .collect::<Result<Vec<(Value, Value)>, serde_json::Error>>(),
    ) else {
        return;
    };
    if !remap_value(&mut v, &sketch, &moved) {
        return;
    }
    if let Ok(kind) = serde_json::from_value(v) {
        f.kind = kind;
    }
}

/// [`remap_feature`] on a JSON value. Returns whether anything changed.
fn remap_value(v: &mut Value, sketch: &str, moved: &[(Value, Value)]) -> bool {
    let swap = |c: &mut Value| match moved.iter().find(|(from, _)| from == c) {
        Some((_, to)) => {
            *c = to.clone();
            true
        }
        None => false,
    };
    let mut changed = false;
    match v {
        Value::Object(map) => {
            if map.get("sketch").and_then(Value::as_str) == Some(sketch) {
                if let Some(c) = map.get_mut("curve") {
                    changed |= swap(c);
                }
                if let Some(Value::Array(cs)) = map.get_mut("curves") {
                    let mut any = false;
                    for c in cs.iter_mut() {
                        any |= swap(c);
                    }
                    if any {
                        // In curve order (the ids' index, then version), each once.
                        let key = |c: &Value| {
                            (
                                c.get("idx").and_then(Value::as_u64),
                                c.get("version").and_then(Value::as_u64),
                            )
                        };
                        cs.sort_by_key(key);
                        cs.dedup();
                        changed = true;
                    }
                }
            }
            for x in map.values_mut() {
                changed |= remap_value(x, sketch, moved);
            }
        }
        Value::Array(xs) => {
            if xs.len() >= 2 && xs[0].as_str() == Some(sketch) {
                changed |= swap(&mut xs[1]);
            }
            for x in xs.iter_mut() {
                changed |= remap_value(x, sketch, moved);
            }
        }
        _ => {}
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Feature, FeatureKind, RegionRef, SketchFeature};
    use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

    fn circle(s: &mut Sketch, c: Vec2, r: f64) -> CurveId {
        let before: Vec<CurveId> = s.curves.keys().collect();
        SketchOp::AddCircle {
            center: c,
            radius: r,
            construction: false,
        }
        .apply(s)
        .unwrap();
        s.curves.keys().find(|k| !before.contains(k)).unwrap()
    }

    /// The #337 reporter's case: a ring whose hole is drawn twice, the second copy dimensioned
    /// and an extrude's region naming it. The copy with the dimension stays, the region follows.
    #[test]
    fn a_circle_drawn_twice_is_one_circle_after_opening() {
        let mut g = Sketch::new();
        let outer = circle(&mut g, Vec2::new(0.0, 0.0), 50.0);
        let first = circle(&mut g, Vec2::new(0.0, 0.0), 20.0);
        let second = circle(&mut g, Vec2::new(0.0, 0.0), 20.0);
        g.dimensions.insert(cadrs_sketch::Dimension::new(
            cadrs_sketch::DimensionKind::Diameter { curve: second },
            40.0,
            0.0,
        ));
        let sketch = FeatureId::from_u128(1);
        let hole = RegionRef {
            sketch,
            curves: vec![first],
            seed: Vec2::new(1.0, 1.0),
        };
        let mut sk = SketchFeature::new(Some(PlaneRef::Top));
        sk.geometry = g;
        let mut doc = Document::new("ring");
        let studio = doc.elements[0].id;
        let features = doc.element_mut(studio).unwrap().features_mut().unwrap();
        features.push(Feature::new(sketch, "Sketch 1", FeatureKind::Sketch(sk)));
        features.push(Feature::new(
            FeatureId::from_u128(2),
            "Extrude 1",
            FeatureKind::Extrude(crate::document::ExtrudeFeature {
                regions: vec![hole],
                ..Default::default()
            }),
        ));

        assert_eq!(remove(&mut doc), 1);
        let el = doc.element(studio).unwrap();
        let g = &el.features()[0].sketch().unwrap().geometry;
        assert_eq!(g.curves.len(), 2);
        assert!(g.curves.contains_key(outer) && g.curves.contains_key(second));
        assert_eq!(g.dimensions.len(), 1, "the dimensioned copy stays");
        let (regions, _) = el.features()[1].input_regions();
        assert_eq!(regions[0].curves, vec![second]);
        // Nothing left to do the next time.
        assert_eq!(remove(&mut doc), 0);
    }
}
