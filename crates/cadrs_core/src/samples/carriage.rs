//! #22's carriage core, guessed from the issue (the reporter's STEP isn't available): a
//! 40 × 30 × 20 block (x 0…40, y −30…0, z 0…20) with an Ø8 bore along y through it, centred at
//! x = 20, z = 10, with a 3D-printing teardrop (two flat faces at 45° to an apex r·√2 over the
//! centre) and, by [`Case`], an M8 hex-nut recess at its front end and a 608 bearing slot
//! across it. Deleting the teardrop's faces should give the same part with a round bore
//! (`tests/delete_face_carriage.rs`); the `carriage-step` script command writes one to import.

use std::f64::consts::FRAC_1_SQRT_2;
use std::sync::Arc;

use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

use crate::document::{BooleanOp, ExtrudeFeature, Feature, FeatureKind, Offset, SketchFeature};
use crate::ids::{ElementId, FeatureId};
use crate::rebuild::Rebuilder;
use crate::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn sketch(plane: PlaneRef, ops: Vec<SketchOp>) -> Feature {
    let mut g = Sketch::new();
    for op in ops {
        op.apply(&mut g).unwrap();
    }
    Feature {
        id: FeatureId::new(),
        name: "Sketch".into(),
        kind: FeatureKind::Sketch(SketchFeature {
            plane: Some(plane),
            disable_imprinting: false,
            geometry: g,
        }),
        suppress_by: None,
    }
}

/// An extrude of the regions of `s` under `seeds`, `depth` deep, starting `start` along the
/// sketch's normal.
fn extrude(s: &Feature, seeds: &[Vec2], depth: f64, op: BooleanOp, start: f64) -> Feature {
    let g = &s.sketch().unwrap().geometry;
    let regions = super::region_refs(s.id, g, seeds);
    assert_eq!(regions.len(), seeds.len(), "a region is missing");
    Feature {
        id: FeatureId::new(),
        name: "Extrude".into(),
        kind: FeatureKind::Extrude(ExtrudeFeature {
            op,
            start_offset: (start != 0.0).then(|| Offset {
                value: start,
                expr: format!("{start} mm"),
                flip: false,
            }),
            ..super::extrude_of(regions, depth)
        }),
        suppress_by: None,
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    SketchOp::AddPolyline {
        points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

/// The bore's centre on the Front plane (world x = 20, z = 10) and radius (Ø8).
pub const BORE: (f64, f64, f64) = (20.0, 10.0, 4.0);

/// The bore's profile on Front: a teardrop (the circle with two lines at 45° to an apex
/// r·√2 above the centre, tangent to it) or the plain circle.
fn bore_profile(teardrop: bool) -> Vec<SketchOp> {
    let (cx, cy, r) = BORE;
    if !teardrop {
        return vec![SketchOp::AddCircle {
            center: v(cx, cy),
            radius: r,
            construction: false,
        }];
    }
    let d = r * FRAC_1_SQRT_2;
    let (left, right) = (v(cx - d, cy + d), v(cx + d, cy + d));
    vec![
        // From the left tangent point the long way round (counter-clockwise) to the right one.
        SketchOp::AddArc {
            center: v(cx, cy),
            start: left,
            end: right,
            construction: false,
        },
        SketchOp::AddPolyline {
            points: vec![right, v(cx, cy + r * std::f64::consts::SQRT_2), left],
            closed: false,
            construction: false,
            label: "Add line",
        },
    ]
}

/// What the block has besides the bore.
#[derive(Clone, Copy, Debug)]
pub enum Case {
    /// Only the bore (the issue: this heals).
    Plain,
    /// An M8 hex-nut recess (13 across flats, 6.5 deep) round the bore's front end: the
    /// teardrop runs into its floor.
    HexRecess,
    /// A tighter recess (11.26 across flats, its top flat 5.63 over the centre, just under the
    /// teardrop's tip at 5.66): a sliver of the teardrop runs on past the floor to the front,
    /// against the recess's top wall.
    TightHexRecess,
    /// A 608 bearing slot (Ø22 wide across x, 7 thick in y, down to 4 above the bottom)
    /// across the bore in the middle of the block: the teardrop is cut in two.
    BearingSlot,
    /// Both.
    Both,
}

/// A 40 × 30 × 20 block (x 0..40, y −30..0, z 0..20) with the bore along y through it and
/// the case's other cuts.
pub fn features(case: Case, teardrop: bool) -> Vec<Feature> {
    let (cx, cy, _) = BORE;
    let base = sketch(PlaneRef::Top, vec![rect(0.0, -30.0, 40.0, 0.0)]);
    let block = extrude(&base, &[v(20.0, -15.0)], 20.0, BooleanOp::New, 0.0);
    let bore = sketch(PlaneRef::Front, bore_profile(teardrop));
    let cut = extrude(&bore, &[v(cx, cy)], 30.0, BooleanOp::Remove, 0.0);
    let mut out = vec![base, block, bore, cut];
    let hex_corner = match case {
        Case::HexRecess | Case::Both => Some(13.0 / 3f64.sqrt()),
        Case::TightHexRecess => Some(6.5),
        _ => None,
    };
    if let Some(corner) = hex_corner {
        // Corners at 0° and every 60°, so a flat is on top, corner·cos 30° above the centre.
        let hex = sketch(
            PlaneRef::Front,
            vec![SketchOp::AddPolygon {
                center: v(cx, cy),
                radius: corner,
                angle: 0.0,
                sides: 6,
                inscribed: false,
                construction: false,
            }],
        );
        // Seeded between the bore and the hex (the ring), and in the bore (cut again).
        let recess = extrude(
            &hex,
            &[v(cx, cy - 5.5), v(cx, cy)],
            6.5,
            BooleanOp::Remove,
            0.0,
        );
        out.extend([hex, recess]);
    }
    if matches!(case, Case::BearingSlot | Case::Both) {
        let slot = sketch(
            PlaneRef::Top,
            vec![rect(cx - 11.0, -18.5, cx + 11.0, -11.5)],
        );
        let pocket = extrude(&slot, &[v(cx, -15.0)], 20.0, BooleanOp::Remove, 4.0);
        out.extend([slot, pocket]);
    }
    out
}

/// The case's part, teardrop or round, as a STEP file.
pub fn step(r: &mut Rebuilder, case: Case, teardrop: bool) -> Result<Vec<u8>, String> {
    let features = Arc::new(features(case, teardrop));
    let build = r.rebuild(&features);
    if let Some((_, e)) = build.errors.first() {
        return Err(e.clone());
    }
    let p = build.parts.first().ok_or("no part")?;
    let item = ExportItem {
        features: features.clone(),
        part: p.id,
        name: "Carriage".into(),
        pose: None,
        source: (ElementId::from_u128(0xca7), p.id),
        source_name: p.name.clone(),
    };
    let files = r.export_files(&ExportRequest::new(
        ModelFormat::Step,
        "Carriage",
        vec![item],
    ))?;
    files
        .into_iter()
        .next()
        .map(|f| f.bytes)
        .ok_or_else(|| "no file".into())
}
