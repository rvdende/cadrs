//! Frames (issue #266): a Part Studio of the Frame feature's members and joints.
//!
//! - A gate on Front: a 600 × 400 rectangle of 40x40x3 square tube, mitred, with a gusset in
//!   each corner, and a mid rail at 200 that butts against the uprights (a T at each end).
//! - A base on Top behind it: a 600 × 400 rectangle of 20x40 T-slot extrusion standing on the
//!   plane (its bottom on the path), butted, the long sides running on.
//! - A handrail on Front: 33.7x3.2 round tube bent round a 60 mm radius between two straight
//!   runs (one member, tangent segments merged), its ends closed by end caps.
//! - A tee on Top: a 48.3x3.2 round tube coped onto another.
//!
//! Every id is fixed, so the document is the same each time it is built.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::advanced::PathRef;
use crate::command::CommandError;
use crate::commands::{AddFeature, AddSketch, EditSketch};
use crate::document::FeatureKind;
use crate::frame::{Anchor, CornerType, FrameFeature, ProfileFamily};
use crate::ids::{ElementId, FeatureId};
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

#[cfg(all(test, feature = "brep"))]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::studio::DocHistory;

    #[test]
    fn the_sample_builds() {
        let mut d = Document::new("Frames");
        let mut h = crate::History::default();
        let el = d.elements[0].id;
        build_in(&mut DocHistory(&mut d, &mut h), el).unwrap();
        let b = crate::rebuild::build(d.element(el).unwrap().features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        // Gate 5 and a gusset at each of its 4 corners, base 4, handrail 1 and its 2 end caps,
        // tee 2.
        assert_eq!(b.parts.len(), 18);
        assert_eq!(b.frames.len(), 18);
        assert_eq!(b.frames.iter().filter(|m| m.plate).count(), 6);
        let rows = crate::frame::cut_list(&b.frames);
        assert!(rows.iter().any(|r| r.curved));
        assert!(
            rows.iter()
                .any(|r| r.ends.contains(&crate::frame::EndCut::Coped))
        );
    }
}
