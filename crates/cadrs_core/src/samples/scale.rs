//! The scale fixtures of the Essential Tips (P3F.3, `essential-tips.md` T1.2, T4.2, X7): a Part
//! Studio at Onshape's budget (250 features, 10 parts) and a document with 40 tabs.
//!
//! - [`studio_features`]: `parts` plates in a row, each 50 × 40, its thickness a variable
//!   (`#t_<i>` = 10 mm): the Variable, the plate's sketch and extrude (New), then `holes`
//!   blind Ø4 × 5 pockets, each its own sketch and extrude (Remove, from the bottom, scoped to
//!   the plate). With 10 parts and 11 holes: 10 × (3 + 2 × 11) = **250 features**.
//! - [`document`]: the 250-feature studio as the first tab, then small Part Studios (a block
//!   each) and Assemblies up to `tabs` tabs.
//!
//! The features are generated directly (a fixture, like a file opened from disk), with fixed
//! ids so runs compare.

use cadrs_sketch::region::{region_at, regions};
use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

use crate::document::{BooleanOp, Document, Element, ElementKind, ExtrudeFeature, Feature, FeatureKind, RegionRef, SketchFeature};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::variables::VariableFeature;

/// The plate size and pitch (mm).
pub const PLATE: (f64, f64) = (50.0, 40.0);
pub const PITCH: f64 = 70.0;
pub const THICKNESS: f64 = 10.0;
pub const HOLE_R: f64 = 2.0;
pub const HOLE_DEPTH: f64 = 5.0;

fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x5ca1_e000_0000_0000_0000_0000_0000_0000 | n)
}

fn sketch(fid: FeatureId, name: String, ops: Vec<SketchOp>) -> Feature {
    let mut g = Sketch::default();
    for op in ops {
        op.apply(&mut g).expect("fixture sketch");
    }
    Feature { id: fid, name, kind: FeatureKind::Sketch(SketchFeature { plane: Some(PlaneRef::Top), disable_imprinting: false, geometry: g }), suppress_by: None }
}

fn region(sketch: &Feature, seed: Vec2) -> RegionRef {
    let g = &sketch.sketch().expect("a sketch").geometry;
    let rs = regions(g);
    let i = region_at(&rs, seed).expect("a region at the seed");
    RegionRef::new(sketch.id, &rs[i])
}

/// Where hole `j` of plate `i` is.
pub fn hole_center(i: usize, j: usize) -> Vec2 {
    let x0 = i as f64 * PITCH;
    Vec2::new(x0 + 8.0 + (j % 4) as f64 * 11.0, 8.0 + (j / 4) as f64 * 12.0)
}

/// The plate part of plate `i` (its extrude's first part).
pub fn plate_part(i: usize) -> PartId {
    PartId::new(id(i as u128 * 100 + 3), 0)
}

/// The features of the scale studio: `parts` plates with `holes` holes each
/// (3 + 2 × holes features per plate).
pub fn studio_features(parts: usize, holes: usize) -> Vec<Feature> {
    let v = Vec2::new;
    let mut out = Vec::new();
    for i in 0..parts {
        let base = i as u128 * 100;
        let name = format!("t_{}", i + 1);
        out.push(Feature {
            id: id(base + 1),
            name: format!("#{name}"),
            kind: FeatureKind::Variable(VariableFeature::length(&name, &format!("{THICKNESS} mm"))),
            suppress_by: None,
        });
        let x0 = i as f64 * PITCH;
        let (w, h) = PLATE;
        let s = sketch(
            id(base + 2),
            format!("Plate {} sketch", i + 1),
            vec![SketchOp::AddPolyline { points: vec![v(x0, 0.0), v(x0 + w, 0.0), v(x0 + w, h), v(x0, h)], closed: true, construction: false, label: "Add rectangle" }],
        );
        let r = region(&s, v(x0 + 1.0, 1.0));
        out.push(s);
        out.push(Feature {
            id: id(base + 3),
            name: format!("Plate {}", i + 1),
            kind: FeatureKind::Extrude(ExtrudeFeature {
                regions: vec![r],
                depth: THICKNESS,
                depth_expr: format!("#{name}"),
                op: BooleanOp::New,
                ..ExtrudeFeature::default()
            }),
            suppress_by: None,
        });
        for j in 0..holes {
            let c = hole_center(i, j);
            let s = sketch(id(base + 10 + 2 * j as u128), format!("Hole {}.{} sketch", i + 1, j + 1), vec![SketchOp::AddCircle { center: c, radius: HOLE_R, construction: false }]);
            let r = region(&s, c);
            out.push(s);
            out.push(Feature {
                id: id(base + 11 + 2 * j as u128),
                name: format!("Hole {}.{}", i + 1, j + 1),
                kind: FeatureKind::Extrude(ExtrudeFeature {
                    regions: vec![r],
                    depth: HOLE_DEPTH,
                    depth_expr: format!("{HOLE_DEPTH} mm"),
                    op: BooleanOp::Remove,
                    merge_scope: vec![plate_part(i)],
                    ..ExtrudeFeature::default()
                }),
                suppress_by: None,
            });
        }
    }
    out
}

/// A plate's volume with its holes (mm³).
pub fn plate_volume(holes: usize) -> f64 {
    PLATE.0 * PLATE.1 * THICKNESS - holes as f64 * std::f64::consts::PI * HOLE_R * HOLE_R * HOLE_DEPTH
}

/// A document with `tabs` tabs: the scale studio (10 plates × 11 holes, "Plates"), then Part
/// Studios with a block each ("Studio 2", …) and every fifth an Assembly.
pub fn document(tabs: usize) -> Document {
    let mut doc = Document::empty("Scale (40 tabs)");
    let mut studio = Element::part_studio("Plates");
    studio.id = ElementId::from_u128(0x5ca1_e000_0000_0000_0000_0000_0000_0001);
    if let ElementKind::PartStudio { features, .. } = &mut studio.kind {
        *features = studio_features(10, 11);
    }
    doc.elements.push(studio);
    let v = Vec2::new;
    for n in 2..=tabs {
        let eid = ElementId::from_u128(0x5ca1_e000_0000_0000_0000_0000_0000_0000 | (n as u128) << 8);
        if n % 5 == 0 {
            let mut a = Element::assembly(format!("Assembly {}", n / 5));
            a.id = eid;
            doc.elements.push(a);
            continue;
        }
        let mut e = Element::part_studio(format!("Studio {n}"));
        e.id = eid;
        let base = 0x10_0000 + n as u128 * 10;
        let s = sketch(id(base), "Sketch 1".into(), vec![SketchOp::AddPolyline {
            points: vec![v(0.0, 0.0), v(20.0 + n as f64, 0.0), v(20.0 + n as f64, 20.0), v(0.0, 20.0)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        }]);
        let r = region(&s, v(1.0, 1.0));
        let x = Feature {
            id: id(base + 1),
            name: "Extrude 1".into(),
            kind: FeatureKind::Extrude(ExtrudeFeature { regions: vec![r], depth: 10.0, depth_expr: "10 mm".into(), ..ExtrudeFeature::default() }),
            suppress_by: None,
        };
        if let ElementKind::PartStudio { features, .. } = &mut e.kind {
            *features = vec![s, x];
        }
        doc.elements.push(e);
    }
    doc
}
