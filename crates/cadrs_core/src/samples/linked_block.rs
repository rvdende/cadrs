//! The linked-documents block (P3G.1, `derived-and-linking-gaps.md` "What each exercise needs":
//! `linked_block_standin`): document **A**, "Block source", with the Part Studio "Block", a
//! 50 × 30 × 25 box with its corner at the origin (x 0..50, y 0..30, z 0..25):
//! V = 50·30·25 = **37 500 mm³**; after the course-style edit of the extrude depth to 40,
//! 50·30·40 = **60 000 mm³**.
//!
//! - **Sketch 1** on Top: the rectangle (0, 0)–(50, 30); **Extrude 1**: 25 mm, New; Aluminum -
//!   6061 (2.70 g/cm³: 37 500 mm³ weigh 101.25 g, centroid (25, 15, 12.5)).
//! - [`consumer`]: document **B**, "Block consumer", with an empty "Assembly 1": the document
//!   the block is linked into.
//! - [`versions_document`]: one document with both, for references to a version of the same
//!   document (ER3).
//!
//! Every id is fixed, so links between the documents resolve in tests and scenarios.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddSketch, EditSketch, SetExtrude, SetPartMaterial};
use crate::document::{Document, Element, ExtrudeFeature};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};

use super::gear_cover::{DocHistory, Studio};

pub const DOCUMENT: DocumentId = DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0100);
pub const STUDIO: ElementId = ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0101);
pub const SKETCH: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0001);
pub const EXTRUDE: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0002);
/// The block.
pub const PART: PartId = PartId::new(EXTRUDE, 0);

/// Document B and its assembly.
pub const CONSUMER: DocumentId = DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0200);
pub const CONSUMER_ASSEMBLY: ElementId = ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0201);

/// The same-document versions document, its studio and assembly.
pub const VERSIONS: DocumentId = DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0300);
pub const VERSIONS_STUDIO: ElementId = ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0301);
pub const VERSIONS_ASSEMBLY: ElementId = ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0302);

pub const DOCUMENT_NAME: &str = "Block source";
pub const STUDIO_NAME: &str = "Block";
pub const CONSUMER_NAME: &str = "Block consumer";
pub const VERSIONS_NAME: &str = "Block versions";

/// mm.
pub const LENGTH: f64 = 50.0;
pub const WIDTH: f64 = 30.0;
pub const HEIGHT: f64 = 25.0;
/// The edited depth.
pub const EDITED_HEIGHT: f64 = 40.0;

/// Adds Sketch 1 and Extrude 1 to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: SKETCH, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH,
        op: SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(LENGTH, 0.0), Vec2::new(LENGTH, WIDTH), Vec2::new(0.0, WIDTH)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    })?;
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(SKETCH))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(SKETCH, &g, &[Vec2::new(LENGTH / 2.0, WIDTH / 2.0)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("the block's region is missing".into()));
    }
    s.run(&AddExtrude { element: el, feature: EXTRUDE, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: EXTRUDE, extrude: super::extrude_of(regions, HEIGHT), label: "Extrude".into() })?;
    // Aluminum 6061 (2.70 g/cm³), so the mass panel shows the mass and the centre of mass
    // (Onshape leaves them blank without a material); volumes are what the tests check.
    s.run(&SetPartMaterial { element: el, parts: vec![PART], material: crate::material::library("Aluminum - 6061") })?;
    Ok(())
}

/// Sets Extrude 1's depth (the course-style edit of the source, 25 → 40).
pub fn set_height(s: &mut dyn Studio, el: ElementId, depth: f64) -> Result<(), CommandError> {
    let mut e = s
        .document()
        .element(el)
        .and_then(|e| e.feature(EXTRUDE))
        .and_then(|f| f.extrude())
        .cloned()
        .ok_or_else(|| CommandError::Invalid("Extrude 1 not found".into()))?;
    e.depth = depth;
    e.depth_expr = format!("{depth} mm");
    s.run(&SetExtrude { element: el, feature: EXTRUDE, extrude: e, label: "Extrude".into() })
}

/// Document A: "Block source" with its Part Studio "Block".
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(DOCUMENT_NAME);
    doc.id = DOCUMENT;
    let mut el = Element::part_studio(STUDIO_NAME);
    el.id = STUDIO;
    doc.elements.push(el);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h), STUDIO)?;
    Ok(doc)
}

/// Document B: "Block consumer" with an empty "Assembly 1".
pub fn consumer() -> Document {
    let mut doc = Document::empty(CONSUMER_NAME);
    doc.id = CONSUMER;
    let mut el = Element::assembly("Assembly 1");
    el.id = CONSUMER_ASSEMBLY;
    doc.elements.push(el);
    doc
}

/// "Block versions": the Part Studio "Block" and an empty "Assembly 1" in one document.
pub fn versions_document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(VERSIONS_NAME);
    doc.id = VERSIONS;
    let mut el = Element::part_studio(STUDIO_NAME);
    el.id = VERSIONS_STUDIO;
    doc.elements.push(el);
    let mut asm = Element::assembly("Assembly 1");
    asm.id = VERSIONS_ASSEMBLY;
    doc.elements.push(asm);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h), VERSIONS_STUDIO)?;
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// P3G.5: the DV exercises' stand-ins (`derived-and-linking-gaps.md` "What each exercise needs")

/// Document A's "Mate connector 1" at the block's top-face centre (25, 15, 25), owned by the
/// block (a source connector to derive, DV3.2).
pub const TOP_CONNECTOR: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0042);

/// Document B, "Block derived" (ex-dv1–ex-dv2, ex-dv5): "Part Studio 1" with "Mate connector 1"
/// at (0, 0, 100) (`fixtures/linked_block_host_standin.cadrs`).
pub const HOST_DOCUMENT: DocumentId = DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0800);
pub const HOST_STUDIO: ElementId = ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0801);
pub const HOST_CONNECTOR: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0803);
pub const HOST_NAME: &str = "Block derived";

/// Document C, "Block assembly" (ex-dv4): the Part Studio "Base", a 100 × 100 × 10 plate
/// (x, y 0..100, z 0..10), and "Assembly 1" with Base <1> fixed
/// (`fixtures/linked_base_standin.cadrs`).
pub const BASE_DOCUMENT: DocumentId = DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0900);
pub const BASE_STUDIO: ElementId = ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0901);
pub const BASE_ASSEMBLY: ElementId = ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0902);
pub const BASE_SKETCH: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0911);
pub const BASE_EXTRUDE: FeatureId = FeatureId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0912);
pub const BASE_PART: PartId = PartId::new(BASE_EXTRUDE, 0);
pub const BASE_INSTANCE: crate::assembly::InstanceId = crate::assembly::InstanceId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0921);
/// ex-dv4's Fastened mate (the block's bottom-face centre on the plate's top-face centre).
pub const BASE_MATE: crate::assembly::mate::MateId = crate::assembly::mate::MateId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0931);
pub const BASE_NAME: &str = "Block assembly";
/// The plate, mm.
pub const PLATE_SIDE: f64 = 100.0;
pub const PLATE_T: f64 = 10.0;

/// A mate connector feature at `offset` from the origin, owned by `owner` when given.
fn connector_at(offset: [f64; 3], owner: Option<PartId>) -> crate::document::FeatureKind {
    use crate::mate::{ConnectorOrigin, MateConnectorFeature};
    crate::document::FeatureKind::MateConnector(MateConnectorFeature {
        origin: Some(ConnectorOrigin::Origin),
        offset,
        offset_expr: offset.map(|x| format!("{x} mm")),
        owner_on: owner.is_some(),
        owner,
        ..MateConnectorFeature::default()
    })
}

/// Document A for the DV exercises (`fixtures/linked_block_standin.cadrs`, its history with V1
/// in `linked_block_standin.history.ron`): the block, with Sketch 1 fully defined (its corner on
/// the origin, the **50** length and **30** width dimensions, so ex-dv3 edits the length in the
/// sketch), and "Mate connector 1" on its top face.
pub fn source_document() -> Result<Document, CommandError> {
    use cadrs_sketch::constraint::{ConstraintOf, PointSpec, rectangle_constraints};
    use cadrs_sketch::{Dimension, DimensionKind};
    let mut a = document()?;
    let mut h = History::default();
    let corners = [Vec2::new(0.0, 0.0), Vec2::new(LENGTH, 0.0), Vec2::new(LENGTH, WIDTH), Vec2::new(0.0, WIDTH)];
    let mut specs = rectangle_constraints(corners);
    specs.push(ConstraintOf::Coincident(PointSpec::At(corners[0]), PointSpec::Origin));
    h.execute(&mut a, &EditSketch { element: STUDIO, feature: SKETCH, op: SketchOp::AddConstraints(specs) })?;
    let g = a.element(STUDIO).and_then(|e| e.feature(SKETCH)).and_then(|f| f.sketch()).map(|k| k.geometry.clone()).ok_or_else(|| CommandError::Invalid("Sketch 1 not found".into()))?;
    let pt = |q: Vec2| g.point_at(q, 1e-6).ok_or_else(|| CommandError::Invalid("a corner of Sketch 1 is missing".into()));
    let (p0, p1, p3) = (pt(corners[0])?, pt(corners[1])?, pt(corners[3])?);
    let dim = |kind: DimensionKind, value: f64, offset: f64| SketchOp::SetDimension { dimension: Dimension { kind, value, offset, along: 0.0, driven: false }, moves: vec![], radii: vec![] };
    h.execute(
        &mut a,
        &EditSketch {
            element: STUDIO,
            feature: SKETCH,
            op: SketchOp::Batch(vec![dim(DimensionKind::Horizontal { a: p0, b: p1 }, LENGTH, -10.0), dim(DimensionKind::Vertical { a: p0, b: p3 }, WIDTH, -10.0)]),
        },
    )?;
    h.execute(&mut a, &crate::commands::AddFeature { element: STUDIO, feature: TOP_CONNECTOR, base_name: "Mate connector".into(), kind: connector_at([LENGTH / 2.0, WIDTH / 2.0, HEIGHT], Some(PART)) })?;
    Ok(a)
}

/// Document B: "Block derived" with "Part Studio 1" holding "Mate connector 1" at (0, 0, 100).
pub fn host_document() -> Result<Document, CommandError> {
    let mut b = Document::empty(HOST_NAME);
    b.id = HOST_DOCUMENT;
    let mut el = Element::part_studio("Part Studio 1");
    el.id = HOST_STUDIO;
    b.elements.push(el);
    let mut h = History::default();
    h.execute(&mut b, &crate::commands::AddFeature { element: HOST_STUDIO, feature: HOST_CONNECTOR, base_name: "Mate connector".into(), kind: connector_at([0.0, 0.0, 100.0], None) })?;
    Ok(b)
}

/// Document C: "Block assembly" with the Part Studio "Base" and "Assembly 1" (Base <1> fixed).
pub fn base_document() -> Result<Document, CommandError> {
    use crate::assembly::commands::{InsertInstance, SetInstancesFixed};
    use crate::assembly::{Instance, InstanceSource, Pose};
    let mut c = Document::empty(BASE_NAME);
    c.id = BASE_DOCUMENT;
    let mut el = Element::part_studio("Base");
    el.id = BASE_STUDIO;
    c.elements.push(el);
    let mut asm = Element::assembly("Assembly 1");
    asm.id = BASE_ASSEMBLY;
    c.elements.push(asm);
    let mut h = History::default();
    let mut s = DocHistory(&mut c, &mut h);
    s.run(&AddSketch { element: BASE_STUDIO, feature: BASE_SKETCH, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch {
        element: BASE_STUDIO,
        feature: BASE_SKETCH,
        op: SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(PLATE_SIDE, 0.0), Vec2::new(PLATE_SIDE, PLATE_SIDE), Vec2::new(0.0, PLATE_SIDE)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    })?;
    let g = s
        .document()
        .element(BASE_STUDIO)
        .and_then(|e| e.feature(BASE_SKETCH))
        .and_then(|f| f.sketch())
        .map(|k| k.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("the base's sketch".into()))?;
    let regions = super::region_refs(BASE_SKETCH, &g, &[Vec2::new(PLATE_SIDE / 2.0, PLATE_SIDE / 2.0)]);
    s.run(&AddExtrude { element: BASE_STUDIO, feature: BASE_EXTRUDE, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: BASE_STUDIO, feature: BASE_EXTRUDE, extrude: super::extrude_of(regions, PLATE_T), label: "Extrude".into() })?;
    s.run(&crate::commands::RenamePart { element: BASE_STUDIO, part: BASE_PART, name: "Base".into() })?;
    s.run(&SetPartMaterial { element: BASE_STUDIO, parts: vec![BASE_PART], material: crate::material::library("Aluminum - 6061") })?;
    s.run(&InsertInstance { element: BASE_ASSEMBLY, instance: Instance::new(BASE_INSTANCE, InstanceSource::Part { element: BASE_STUDIO, part: BASE_PART }, Pose::IDENTITY) })?;
    s.run(&SetInstancesFixed { element: BASE_ASSEMBLY, instances: vec![BASE_INSTANCE], fixed: true })?;
    Ok(c)
}

/// ex-dv4's lost-face edit of A (a stand-in for a remodelled source): Sketch 1's rectangle
/// deleted and drawn again at the same place, and Extrude 1 pointed at the new region. The
/// block is the same size and the same part, but every face has a new name (they come from the
/// sketch's curves), so a mate on one of them loses its entity.
pub fn redraw(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let sketch = |s: &dyn Studio| {
        s.document().element(el).and_then(|e| e.feature(SKETCH)).and_then(|f| f.sketch()).map(|k| k.geometry.clone()).ok_or_else(|| CommandError::Invalid("Sketch 1 not found".into()))
    };
    let g = sketch(s)?;
    let curves: Vec<_> = g.curves.iter().map(|(id, _)| id).collect();
    s.run(&EditSketch { element: el, feature: SKETCH, op: SketchOp::Delete { curves, points: vec![], dimensions: vec![], constraints: vec![] } })?;
    let depth = s.document().element(el).and_then(|e| e.feature(EXTRUDE)).and_then(|f| f.extrude()).map(|e| e.depth).unwrap_or(HEIGHT);
    s.run(&EditSketch {
        element: el,
        feature: SKETCH,
        op: SketchOp::AddPolyline {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(LENGTH, 0.0), Vec2::new(LENGTH, WIDTH), Vec2::new(0.0, WIDTH)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    })?;
    let g = sketch(s)?;
    let regions = super::region_refs(SKETCH, &g, &[Vec2::new(LENGTH / 2.0, WIDTH / 2.0)]);
    let mut e = s.document().element(el).and_then(|x| x.feature(EXTRUDE)).and_then(|f| f.extrude()).cloned().ok_or_else(|| CommandError::Invalid("Extrude 1 not found".into()))?;
    e.regions = regions;
    e.depth = depth;
    s.run(&SetExtrude { element: el, feature: EXTRUDE, extrude: e, label: "Extrude".into() })
}

/// A stored file of `document` for the fixtures (created by "cadrs" at a fixed time).
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}

/// The history of a fixture document: its start and one version "V1" (`description`) with the
/// fixed id `version`.
pub fn history_with_v1(doc: &Document, version: crate::history_log::VersionId, description: &str) -> crate::history_log::HistoryLog {
    let t = 1_790_553_600;
    let mut log = crate::history_log::HistoryLog::start(doc, t, "cadrs");
    log.create_version_with_id(version, "V1", description, t + 60, "cadrs");
    log
}

/// A's V1 in `fixtures/linked_block_standin.history.ron`.
pub const SOURCE_V1: crate::history_log::VersionId = crate::history_log::VersionId(uuid::Uuid::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0a01));

/// ex-dv3: Sketch 1's length dimension (of [`source_document`]) set to `l` (50 → 60).
pub fn set_length(s: &mut dyn Studio, el: ElementId, l: f64) -> Result<(), CommandError> {
    use cadrs_sketch::DimensionKind;
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(SKETCH))
        .and_then(|f| f.sketch())
        .map(|k| k.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("Sketch 1 not found".into()))?;
    let id = g
        .dimensions
        .iter()
        .find_map(|(id, d)| matches!(d.kind, DimensionKind::Horizontal { .. }).then_some(id))
        .ok_or_else(|| CommandError::Invalid("Sketch 1 has no length dimension".into()))?;
    s.run(&EditSketch { element: el, feature: SKETCH, op: SketchOp::SetDimensionValue { id, value: l } })
}

/// ex-dv4: **Fastened 1** in C's Assembly 1 between the bottom-face centre of the Block
/// instance `block` and the top-face centre of Base <1> (the block moves onto the plate: its
/// bottom-face centre at (50, 50, 10)).
pub fn fasten_on_base(s: &mut dyn Studio, block: crate::assembly::InstanceId) -> Result<(), CommandError> {
    use crate::assembly::connector::MateConnector;
    use crate::assembly::mate::{Mate, MateFeature, MateKind, MateType};
    let solids = crate::assembly::document_occurrence_solids(s.document(), BASE_ASSEMBLY);
    let (Some(b), Some(p)) = (solids.get(&block), solids.get(&BASE_INSTANCE)) else {
        return Err(CommandError::Invalid("the block or the base has no part".into()));
    };
    let bottom = super::piston::face_connector(b, [0.0, 0.0, -1.0])?;
    let top = super::piston::face_connector(p, [0.0, 0.0, 1.0])?;
    let mut c1 = MateConnector::implicit(block, &bottom);
    // Face to face: the block's bottom (facing down) turned to agree with the plate's top.
    c1.flip = true;
    let m = Mate::new(MateType::Fastened, c1, MateConnector::implicit(BASE_INSTANCE, &top));
    super::piston::add_mate(s, BASE_ASSEMBLY, MateFeature::new(BASE_MATE, "Fastened 1", MateKind::Mate(m)), block)
}
