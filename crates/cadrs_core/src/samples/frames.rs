//! Frames (issue #266): a Part Studio of the Frame feature's members and joints.
//!
//! - A gate on Front: a 600 × 400 rectangle of 40x40x3 square tube, mitred, with a gusset in
//!   each corner, and a mid rail at 200 that butts against the uprights (a T at each end, a
//!   gusset either side of it).
//! - A base on Top behind it: a 600 × 400 rectangle of 20x40 T-slot extrusion standing on the
//!   plane (its bottom on the path), butted, the long sides running on.
//! - A handrail on Front: 33.7x3.2 round tube bent round a 60 mm radius between two straight
//!   runs (one member, tangent segments merged), its ends closed by end caps.
//! - A tee on Top: a 48.3x3.2 round tube coped onto another.
//!
//! [`document`] adds a second Part Studio, "2020 workbench": an 800 × 500 × 700 cube of 20x20
//! T-slot (a rectangle on Top, one on a plane 700 above it, four posts on Right and on a plane
//! 800 along X), butted: at each corner the two rails run through and the post stops between
//! them, so the cut list is two lengths of rail and one of post. `fixtures/frames.cadrs` is
//! that document (`cadrs fixtures/frames.cadrs` opens it).
//!
//! Every id is fixed, so the document is the same each time it is built.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::advanced::PathRef;
use crate::command::{CommandError, History};
use crate::commands::{AddFeature, AddSketch, EditSketch};
use crate::document::{Document, FeatureKind};
use crate::frame::{Anchor, CornerType, FrameFeature, ProfileFamily};
use crate::ids::{ElementId, FeatureId};
use crate::plane::{PlaneEntity, PlaneFeature, PlaneType};
use crate::studio::DocHistory;
use crate::studio::Studio;

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x6ea5_0266_0000_0000_0000_0000_0000_0000 + n)
}

/// The Frame features: gate, base, handrail, tee.
pub const FRAMES: [FeatureId; 4] = [id(0x11), id(0x21), id(0x31), id(0x41)];

fn chain(points: &[(f64, f64)], closed: bool) -> SketchOp {
    SketchOp::AddPolyline {
        points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
        closed,
        construction: false,
        label: "Add line",
    }
}

fn frame(family: ProfileFamily, size: &str, path: Vec<PathRef>) -> FrameFeature {
    let mut x = FrameFeature::default();
    x.set_family(family);
    x.size = size.into();
    x.path = path;
    x
}

/// Adds the sample's sketches and frames to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let sketch = |s: &mut dyn Studio, n: u128, plane, ops: Vec<SketchOp>| {
        let f = id(n);
        s.run(&AddSketch {
            element: el,
            feature: f,
            plane: Some(plane),
        })?;
        for op in ops {
            s.run(&EditSketch {
                element: el,
                feature: f,
                op,
            })?;
        }
        Ok::<FeatureId, CommandError>(f)
    };
    let gate = sketch(
        s,
        0x10,
        PlaneRef::Front,
        vec![
            chain(
                &[(0.0, 0.0), (600.0, 0.0), (600.0, 400.0), (0.0, 400.0)],
                true,
            ),
            chain(&[(0.0, 200.0), (600.0, 200.0)], false),
        ],
    )?;
    let base = sketch(
        s,
        0x20,
        PlaneRef::Top,
        vec![chain(
            &[(0.0, 150.0), (600.0, 150.0), (600.0, 550.0), (0.0, 550.0)],
            true,
        )],
    )?;
    let rail = sketch(
        s,
        0x30,
        PlaneRef::Front,
        vec![
            chain(&[(800.0, 0.0), (800.0, 300.0)], false),
            SketchOp::AddArc {
                center: Vec2::new(860.0, 300.0),
                start: Vec2::new(860.0, 360.0),
                end: Vec2::new(800.0, 300.0),
                construction: false,
            },
            chain(&[(860.0, 360.0), (1160.0, 360.0)], false),
        ],
    )?;
    let tee = sketch(
        s,
        0x40,
        PlaneRef::Top,
        vec![
            chain(&[(1300.0, 150.0), (1700.0, 150.0)], false),
            chain(&[(1500.0, 150.0), (1500.0, 450.0)], false),
        ],
    )?;
    let mut base_frame = frame(ProfileFamily::TSlot, "20x40", vec![PathRef::Sketch(base)]);
    base_frame.anchor = Anchor::Bottom;
    base_frame.corner = CornerType::Butt;
    let mut tee_frame = frame(
        ProfileFamily::RoundTube,
        "48.3x3.2",
        vec![PathRef::Sketch(tee)],
    );
    tee_frame.corner = CornerType::CopedButt;
    let mut gate_frame = frame(
        ProfileFamily::SquareTube,
        "40x40x3",
        vec![PathRef::Sketch(gate)],
    );
    gate_frame.corner = CornerType::Miter;
    gate_frame.gussets = true;
    let mut rail_frame = frame(
        ProfileFamily::RoundTube,
        "33.7x3.2",
        vec![PathRef::Sketch(rail)],
    );
    rail_frame.end_caps = true;
    let frames = [gate_frame, base_frame, rail_frame, tee_frame];
    for (feature, x) in FRAMES.into_iter().zip(frames) {
        s.run(&AddFeature {
            element: el,
            feature,
            base_name: "Frame".into(),
            kind: FeatureKind::Frame(x),
        })?;
    }
    Ok(())
}

/// The workbench's Frame feature.
pub const WORKBENCH: FeatureId = id(0x61);

/// Adds the 2020 workbench to the Part Studio `el`.
pub fn build_workbench_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let (w, d, h) = (800.0, 500.0, 700.0);
    let plane = |s: &mut dyn Studio, n: u128, of: PlaneRef, offset: f64| {
        let f = id(n);
        s.run(&AddFeature {
            element: el,
            feature: f,
            base_name: "Plane".into(),
            kind: FeatureKind::Plane(PlaneFeature {
                kind: PlaneType::Offset,
                entities: vec![PlaneEntity::Plane(of)],
                offset,
                offset_expr: format!("{offset} mm"),
                ..PlaneFeature::default()
            }),
        })?;
        let features = s
            .document()
            .element(el)
            .map(|e| e.features().to_vec())
            .unwrap_or_default();
        crate::parts::plane_feature_ref(&features, f)
            .ok_or_else(|| CommandError::Invalid("A plane did not build".into()))
    };
    let sketch = |s: &mut dyn Studio, n: u128, plane, ops: Vec<SketchOp>| {
        let f = id(n);
        s.run(&AddSketch {
            element: el,
            feature: f,
            plane: Some(plane),
        })?;
        for op in ops {
            s.run(&EditSketch {
                element: el,
                feature: f,
                op,
            })?;
        }
        Ok::<FeatureId, CommandError>(f)
    };
    let rect = chain(&[(0.0, 0.0), (w, 0.0), (w, d), (0.0, d)], true);
    let posts = || {
        vec![
            chain(&[(0.0, 0.0), (0.0, h)], false),
            chain(&[(d, 0.0), (d, h)], false),
        ]
    };
    let top = plane(s, 0x50, PlaneRef::Top, h)?;
    let side = plane(s, 0x51, PlaneRef::Right, w)?;
    let base = sketch(s, 0x52, PlaneRef::Top, vec![rect.clone()])?;
    let lid = sketch(s, 0x53, top, vec![rect])?;
    let left = sketch(s, 0x54, PlaneRef::Right, posts())?;
    let right = sketch(s, 0x55, side, posts())?;
    s.run(&AddFeature {
        element: el,
        feature: WORKBENCH,
        base_name: "Frame".into(),
        kind: FeatureKind::Frame(FrameFeature {
            path: [base, lid, left, right].map(PathRef::Sketch).to_vec(),
            ..FrameFeature::default()
        }),
    })?;
    Ok(())
}

/// A new document holding the samples: "Frames" with its Part Studios "Frames" and "2020
/// workbench", as `fixtures/frames.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Frames");
    doc.id = crate::ids::DocumentId::from_u128(0x6ea5_0266_0000_0000_0000_0000_0000_0100);
    let mut el = crate::document::Element::part_studio("Frames");
    el.id = ElementId::from_u128(0x6ea5_0266_0000_0000_0000_0000_0000_0101);
    let first = el.id;
    doc.elements.push(el);
    let mut bench = crate::document::Element::part_studio("2020 workbench");
    bench.id = ElementId::from_u128(0x6ea5_0266_0000_0000_0000_0000_0000_0102);
    let second = bench.id;
    doc.elements.push(bench);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h), first)?;
    build_workbench_in(&mut DocHistory(&mut doc, &mut h), second)?;
    Ok(doc)
}

/// The samples as a document file (`fixtures/frames.cadrs`), dated 2026-11-19.
pub fn file(document: Document) -> crate::store::DocumentFile {
    let now = 1_795_046_400;
    crate::store::DocumentFile {
        version: crate::store::SCHEMA_VERSION,
        meta: crate::library::DocumentMeta::new("cadrs", now),
        document,
    }
}

#[cfg(all(test, feature = "brep"))]
mod tests {
    use super::*;

    #[test]
    fn the_sample_builds() {
        let mut d = Document::new("Frames");
        let mut h = crate::History::default();
        let el = d.elements[0].id;
        build_in(&mut DocHistory(&mut d, &mut h), el).unwrap();
        let b = crate::rebuild::build(d.element(el).unwrap().features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        // Gate 5, a gusset at each of its 4 corners and 2 at each end of its mid rail, base 4,
        // handrail 1 and its 2 end caps, tee 2.
        assert_eq!(b.parts.len(), 22);
        assert_eq!(b.frames.len(), 22);
        assert_eq!(b.frames.iter().filter(|m| m.plate).count(), 10);
        let rows = crate::frame::cut_list(&b.frames);
        assert!(rows.iter().any(|r| r.curved));
        assert!(
            rows.iter()
                .any(|r| r.ends.contains(&crate::frame::EndCut::Coped))
        );
    }

    #[test]
    fn the_workbench_builds_two_rails_and_a_post() {
        let doc = document().unwrap();
        let el = doc.elements[1].id;
        let b = crate::rebuild::build(doc.element(el).unwrap().features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        assert_eq!(b.parts.len(), 12);
        let rows = crate::frame::cut_list(&b.frames);
        let mut got: Vec<(i64, usize)> = rows
            .iter()
            .map(|r| ((r.length * 10.0).round() as i64, r.quantity()))
            .collect();
        got.sort();
        // Rails along X run through (820), along Y stop at them (480); posts stop between the
        // rails (680).
        assert_eq!(got, [(4800, 4), (6800, 4), (8200, 4)], "{rows:?}");
    }
}
