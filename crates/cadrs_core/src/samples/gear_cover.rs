//! The Jackhammer Gear Cover stand-in (P3.6, PS17). Onshape's public "Jackhammer" document
//! can't be copied, so the exercise starts from this cover, built from our own features and
//! grouped in a closed "Base Features" folder, in Aluminum - 380, mm and kg:
//!
//! - **Sketch 1 / Extrude 1**: a rounded rectangle on Top, x −55..55, y −200..0, corners R20,
//!   40 mm up (the bottom face is "Face of Extrude 1", which the Shell removes);
//! - **Sketch 2 / Extrude 2**: on Right, the region above the line from (y −210, z 22) to
//!   (y −120, z 40), removed symmetric 200 mm: the cover's nose slopes down to the front at
//!   1 in 5. It stands in for the course's "Edge of Loft 2" (PS17.9): where the slope meets the
//!   walls the angle between the faces goes from 90° on the sides to 78.7° at the front, so a
//!   Width fillet there has a varying radius;
//! - **Sketch 3 / Extrude 3**: two Ø30 bosses at (±30, −40), 6 mm on the top face (a starting
//!   offset of 40), Add: their circular top edges are the course's "A" references (PS17.4).
//!
//! Every id is fixed, so the fixture `fixtures/gear_cover_standin.cadrs` is regenerated exactly
//! (see `cadrs_core/tests/gear_cover.rs`).

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddSketch, CreateFolder, EditSketch, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{BooleanOp, Document, ExtrudeFeature, Offset};
use crate::ids::{ElementId, FeatureId, PartId};

pub const SKETCH_1: FeatureId = FeatureId::from_u128(0x6ea4_c0e4_0000_0000_0000_0000_0000_0001);
pub const EXTRUDE_1: FeatureId = FeatureId::from_u128(0x6ea4_c0e4_0000_0000_0000_0000_0000_0002);
pub const SKETCH_2: FeatureId = FeatureId::from_u128(0x6ea4_c0e4_0000_0000_0000_0000_0000_0003);
pub const EXTRUDE_2: FeatureId = FeatureId::from_u128(0x6ea4_c0e4_0000_0000_0000_0000_0000_0004);
pub const SKETCH_3: FeatureId = FeatureId::from_u128(0x6ea4_c0e4_0000_0000_0000_0000_0000_0005);
pub const EXTRUDE_3: FeatureId = FeatureId::from_u128(0x6ea4_c0e4_0000_0000_0000_0000_0000_0006);
pub const FOLDER: FeatureId = FeatureId::from_u128(0x6ea4_c0e4_0000_0000_0000_0000_0000_0007);
/// The cover part.
pub const PART: PartId = PartId::new(EXTRUDE_1, 0);

/// Half the width (x), the length (y from −LENGTH to 0), the height and the corner radius.
pub const HALF_WIDTH: f64 = 55.0;
pub const LENGTH: f64 = 200.0;
pub const HEIGHT: f64 = 40.0;
pub const CORNER_R: f64 = 20.0;
/// The slope: z = SLOPE_Z0 + SLOPE_K (y + LENGTH), up to z = HEIGHT at y = SLOPE_END.
pub const SLOPE_Z0: f64 = 24.0;
pub const SLOPE_K: f64 = 0.2;
pub const SLOPE_END: f64 = -120.0;
/// The bosses: centres (±BOSS_X, BOSS_Y), radius, height above the top face.
pub const BOSS_X: f64 = 30.0;
pub const BOSS_Y: f64 = -40.0;
pub const BOSS_R: f64 = 15.0;
pub const BOSS_H: f64 = 6.0;
/// Where the four other holes of the course's step 4 go (on the flat top, symmetric about the
/// YZ plane): (±HOLE_X, HOLE_Y), 4 mm outboard of the bosses' centres and 60 mm in front of
/// them, and (±TOP_X, TOP_Y), 16 mm either side of the centreline and 24 mm behind the bosses
/// (the course's 16 and 4; its 93 and 95 don't fit the stand-in's 200 mm length).
pub const HOLE_X: f64 = 34.0;
pub const HOLE_Y: f64 = -100.0;
pub const TOP_X: f64 = 16.0;
pub const TOP_Y: f64 = -16.0;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let v = Vec2::new;
    SketchOp::AddPolyline {
        points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

// The trait moved to [`crate::studio`] (P3H.3); re-exported here so the samples keep compiling.
pub use crate::studio::{DocHistory, Studio};

/// Adds the stand-in's six base features, its folder, the part's name and its material to the
/// Part Studio `el` of `doc`.
pub fn build(doc: &mut Document, h: &mut History, el: ElementId) -> Result<(), CommandError> {
    build_in(&mut DocHistory(doc, h), el)
}

/// [`build`] through any [`Studio`].
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let v = Vec2::new;
    // Sketch 1: the rounded rectangle.
    s.run(&AddSketch { element: el, feature: SKETCH_1, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: SKETCH_1, op: rect(-HALF_WIDTH, -LENGTH, HALF_WIDTH, 0.0) })?;
    for corner in [v(-HALF_WIDTH, -LENGTH), v(HALF_WIDTH, -LENGTH), v(HALF_WIDTH, 0.0), v(-HALF_WIDTH, 0.0)] {
        let p = {
            let g = &s
                .document()
                .element(el)
                .and_then(|e| e.feature(SKETCH_1))
                .and_then(|f| f.sketch())
                .ok_or_else(|| CommandError::Invalid("sketch 1 not found".into()))?
                .geometry;
            g.point_at(corner, 1e-9).ok_or_else(|| CommandError::Invalid("corner not found".into()))?
        };
        s.run(
            &EditSketch {
                element: el,
                feature: SKETCH_1,
                op: SketchOp::Fillet { corner: p, radius: CORNER_R, equal_to: None },
            },
        )?;
    }
    extrude(s, el, SKETCH_1, EXTRUDE_1, &[v(0.0, -100.0)], |e| {
        e.depth = HEIGHT;
        e.depth_expr = "40 mm".into();
    })?;
    // Sketch 2 on Right (sketch x = model Y, sketch y = model Z): the region over the slope.
    let y0 = -LENGTH - 10.0;
    let z0 = SLOPE_Z0 + SLOPE_K * (y0 + LENGTH);
    s.run(&AddSketch { element: el, feature: SKETCH_2, plane: Some(PlaneRef::Right) })?;
    s.run(
        &EditSketch {
            element: el,
            feature: SKETCH_2,
            op: SketchOp::AddPolyline {
                points: vec![v(y0, z0), v(SLOPE_END, HEIGHT), v(SLOPE_END, HEIGHT + 20.0), v(y0, HEIGHT + 20.0)],
                closed: true,
                construction: false,
                label: "Add polygon",
            },
        },
    )?;
    extrude(s, el, SKETCH_2, EXTRUDE_2, &[v(-190.0, 50.0)], |e| {
        e.op = BooleanOp::Remove;
        e.symmetric = true;
        e.depth = 200.0;
        e.depth_expr = "200 mm".into();
    })?;
    // Sketch 3: the bosses, extruded from the top face (a starting offset of 40 mm).
    s.run(&AddSketch { element: el, feature: SKETCH_3, plane: Some(PlaneRef::Top) })?;
    for x in [-BOSS_X, BOSS_X] {
        s.run(
            &EditSketch {
                element: el,
                feature: SKETCH_3,
                op: SketchOp::AddCircle { center: v(x, BOSS_Y), radius: BOSS_R, construction: false },
            },
        )?;
    }
    extrude(s, el, SKETCH_3, EXTRUDE_3, &[v(-BOSS_X, BOSS_Y), v(BOSS_X, BOSS_Y)], |e| {
        e.op = BooleanOp::Add;
        e.depth = BOSS_H;
        e.depth_expr = "6 mm".into();
        e.start_offset = Some(Offset { value: HEIGHT, expr: "40 mm".into(), flip: false });
    })?;
    s.run(&RenamePart { element: el, part: PART, name: "Gear Cover".into() })?;
    s.run(
        &SetPartMaterial { element: el, parts: vec![PART], material: crate::material::library("Aluminum - 380") },
    )?;
    s.run(
        &CreateFolder {
            element: el,
            folder: FOLDER,
            name: Some("Base Features".into()),
            features: vec![SKETCH_1, EXTRUDE_1, SKETCH_2, EXTRUDE_2, SKETCH_3, EXTRUDE_3],
        },
    )?;
    Ok(())
}

fn extrude(
    s: &mut dyn Studio,
    el: ElementId,
    sketch: FeatureId,
    feature: FeatureId,
    seeds: &[Vec2],
    set: impl FnOnce(&mut ExtrudeFeature),
) -> Result<(), CommandError> {
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(sketch, &g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid("a region of the stand-in is missing".into()));
    }
    let mut e = super::extrude_of(regions, 25.0);
    set(&mut e);
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: e, label: "Extrude".into() })?;
    Ok(())
}

/// A new document holding the stand-in: "Jackhammer (stand-in)" with its Part Studio "Gear
/// Cover" (mm, kg), as `fixtures/gear_cover_standin.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Jackhammer (stand-in)");
    doc.id = crate::ids::DocumentId::from_u128(0x6ea4_c0e4_0000_0000_0000_0000_0000_0100);
    let mut el = crate::document::Element::part_studio("Gear Cover");
    el.id = ElementId::from_u128(0x6ea4_c0e4_0000_0000_0000_0000_0000_0101);
    let id = el.id;
    doc.elements.push(el);
    let mut h = History::default();
    build(&mut doc, &mut h, id)?;
    Ok(doc)
}

/// The stand-in as a document file (`fixtures/gear_cover_standin.cadrs`), dated 2026-09-28.
pub fn file(document: Document) -> crate::store::DocumentFile {
    let now = 1_790_553_600;
    crate::store::DocumentFile {
        version: crate::store::SCHEMA_VERSION,
        meta: crate::library::DocumentMeta::new("cadrs", now),
        document,
    }
}
