//! The top-down sheet metal stand-in (P3I.8, SM18; lesson 18 "Sheet metal and top-down
//! design"): the **Heating Mantle**'s space allocation part ("master model") in its own Part
//! Studio, derived at the workspace into a second Part Studio where two Sheet metal models are
//! made around it, each named after its part (the context dropdown lists them by those names):
//!
//! - Part Studio **Space Envelope**: a Front-plane profile ([`PROFILE`]: 200 wide, 120 high, the
//!   top-right corner cut at 45° by a 40 × 40 angled face) extruded [`DEPTH`] (along −Y). The
//!   lesson's envelope is T-notched (`t0044.7.png`); a T's notch walls unfold onto the walls
//!   beside them in cadrs's Convert (a flat pattern collision), so the stand-in is a plain box.
//! - Part Studio **Part Studio 1**: **Derived 1** (Space Envelope at the workspace, so edits of
//!   the master follow at once), then
//!   - **Enclosure**: Sheet metal model → Convert of the derived part, the top and the angled
//!     face excluded, the bottom's four edges bent, Keep input part; its part renamed
//!     "Enclosure";
//!   - **Cover**: Sheet metal model → Thicken of the top and the angled face, the edge between
//!     them bent; its part "Cover".
//!
//! Both are 1.5 mm thick, inner bend radius 1.5 mm. [`set_depth`] edits the master's depth, as
//! the lesson makes the enclosure wider. [`master_document`] is the document the
//! `sm_p3i8_topdown` scenario starts from: the master and an empty Part Studio 1.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use super::gear_cover::{DocHistory, Studio};
use crate::applied::EdgeOrFace;
use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, RenameFeature, RenamePart, SetExtrude};
use crate::derived::{AddDerived, DerivedFeature};
use crate::document::{Document, EdgeRef, ExtrudeFeature, FaceRef, FeatureKind};
use crate::external::{RefAt, SourceRef};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
use crate::solid::Solid;

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x5318_0000_0000_0000_0000_0000_0000_0000 + n)
}

pub const DOCUMENT: DocumentId = DocumentId::from_u128(0x5318_0000_0000_0000_0000_0000_0000_0100);
pub const MASTER: ElementId = ElementId::from_u128(0x5318_0000_0000_0000_0000_0000_0000_0101);
pub const STUDIO: ElementId = ElementId::from_u128(0x5318_0000_0000_0000_0000_0000_0000_0102);

pub const MASTER_SKETCH: FeatureId = id(0x11);
pub const MASTER_EXTRUDE: FeatureId = id(0x12);
pub const MASTER_PART: PartId = PartId::new(MASTER_EXTRUDE, 0);
pub const DERIVED: FeatureId = id(0x21);
pub const ENCLOSURE: FeatureId = id(0x22);
pub const COVER: FeatureId = id(0x23);
pub const ENCLOSURE_PART: PartId = PartId::new(ENCLOSURE, 0);
pub const COVER_PART: PartId = PartId::new(COVER, 0);

/// The master's profile on Front (x, z).
pub const PROFILE: [(f64, f64); 5] = [(0.0, 0.0), (200.0, 0.0), (200.0, 80.0), (160.0, 120.0), (0.0, 120.0)];
pub const DEPTH: f64 = 150.0;
pub const THICKNESS: f64 = 1.5;
pub const RADIUS: f64 = 1.5;

fn params() -> cadrs_sheetmetal::Params {
    cadrs_sheetmetal::Params { thickness: THICKNESS, bend_radius: RADIUS, ..SheetMetalModelFeature::default_params() }
}

fn sm(op: SheetMetalOp) -> SheetMetalModelFeature {
    let p = params();
    SheetMetalModelFeature { operation: op, params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() }
}

/// The derived part's face whose outward normal is `n` and whose centre lies on the plane through
/// `at` (`None` if there is none).
pub fn face(s: &Solid, part: PartId, n: [f64; 3], at: [f64; 3]) -> Option<FaceRef> {
    let i = (0..s.faces.len()).find(|i| {
        let f = &s.faces[*i];
        let (Some(pl), Some(c)) = (f.plane, f.center) else { return false };
        let m = pl.normal();
        let dot = m[0] * n[0] + m[1] * n[1] + m[2] * n[2];
        let off = (0..3).map(|k| (c[k] - at[k]) * n[k]).sum::<f64>();
        dot > 1.0 - 1e-6 && off.abs() < 1e-6
    })?;
    let f = &s.faces[i];
    Some(FaceRef { part, face: f.name, seed: f.center? })
}

/// The edge both faces share.
pub fn edge_between(s: &Solid, part: PartId, a: &FaceRef, b: &FaceRef) -> Option<EdgeRef> {
    let e = s.edges.iter().find(|e| e.name.faces.contains(&a.face) && e.name.faces.contains(&b.face))?;
    let p = e.points[e.points.len() / 2];
    Some(EdgeRef { part, edge: e.name, seed: p })
}

/// The Space Envelope studio's features.
fn build_master(s: &mut dyn Studio) -> Result<(), CommandError> {
    s.run(&AddSketch { element: MASTER, feature: MASTER_SKETCH, plane: Some(PlaneRef::Front) })?;
    let op = SketchOp::AddPolyline { points: PROFILE.iter().map(|(x, y)| Vec2::new(*x, *y)).collect(), closed: true, construction: false, label: "Add polygon" };
    s.run(&EditSketch { element: MASTER, feature: MASTER_SKETCH, op })?;
    let g = s.document().element(MASTER).and_then(|e| e.feature(MASTER_SKETCH)).and_then(|f| f.sketch()).map(|k| k.geometry.clone()).ok_or_else(|| CommandError::Invalid("no sketch".into()))?;
    let regions = super::region_refs(MASTER_SKETCH, &g, &[Vec2::new(100.0, 60.0)]);
    let mut e = super::extrude_of(regions, DEPTH);
    e.depth_expr = format!("{DEPTH} mm");
    s.run(&AddExtrude { element: MASTER, feature: MASTER_EXTRUDE, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: MASTER, feature: MASTER_EXTRUDE, extrude: e, label: "Extrude".into() })?;
    s.run(&RenamePart { element: MASTER, part: MASTER_PART, name: "Space Envelope".into() })?;
    Ok(())
}

/// Sets the master's depth (how wide the enclosure is), as an edit in its own studio.
pub fn set_depth(s: &mut dyn Studio, depth: f64) -> Result<(), CommandError> {
    let mut e = match s.document().element(MASTER).and_then(|el| el.feature(MASTER_EXTRUDE)).map(|f| &f.kind) {
        Some(FeatureKind::Extrude(e)) => e.clone(),
        _ => return Err(CommandError::Invalid("no master extrude".into())),
    };
    e.depth = depth;
    e.depth_expr = format!("{depth} mm");
    s.run(&SetExtrude { element: MASTER, feature: MASTER_EXTRUDE, extrude: e, label: "Edit Extrude 1".into() })
}

/// The derived part's id in Part Studio 1.
pub fn derived_part() -> PartId {
    crate::derived::derived_part(DERIVED, MASTER_PART, 0)
}

/// Part Studio 1: Derived 1, then the Enclosure and the Cover.
fn build_studio(s: &mut dyn Studio) -> Result<(), CommandError> {
    let mut d = DerivedFeature::default();
    let el = s.document().element(MASTER).ok_or_else(|| CommandError::Invalid("no master".into()))?.clone();
    d.fill_from(&el, "", "");
    d.source = Some(SourceRef { document: None, at: RefAt::Workspace, element: MASTER, pinned: false });
    s.run(&AddDerived { element: STUDIO, feature: DERIVED, derived: d, links: Vec::new() })?;
    let features = s.document().element(STUDIO).map(|e| e.active_features()).unwrap_or_default();
    let b = crate::rebuild::build(&features);
    let pid = derived_part();
    let part = b.parts.iter().find(|p| p.id == pid).ok_or_else(|| CommandError::Invalid("the derived part doesn't build".into()))?;
    let sol = &part.solid;
    let (d1, d2) = ((45f64).to_radians().cos(), (45f64).to_radians().sin());
    let missing = || CommandError::Invalid("a face of the derived part is missing".into());
    let top = face(sol, pid, [0.0, 0.0, 1.0], [0.0, 0.0, 120.0]).ok_or_else(missing)?;
    let angled = face(sol, pid, [d1, 0.0, d2], [200.0, 0.0, 80.0]).ok_or_else(missing)?;
    let bottom = face(sol, pid, [0.0, 0.0, -1.0], [0.0, 0.0, 0.0]).ok_or_else(missing)?;
    // Every edge round the bottom.
    let bends: Vec<EdgeOrFace> = sol
        .face_edges(&bottom.face)
        .into_iter()
        .filter_map(|e| sol.edge(&e))
        .map(|e| EdgeOrFace::Edge(EdgeRef { part: pid, edge: e.name, seed: e.points[e.points.len() / 2] }))
        .collect();
    let mut x = sm(SheetMetalOp::Convert);
    x.parts = vec![pid];
    x.exclude = vec![top, angled];
    x.bends = bends;
    x.keep_input = true;
    s.run(&AddFeature { element: STUDIO, feature: ENCLOSURE, base_name: "Sheet metal model".into(), kind: FeatureKind::SheetMetalModel(x) })?;
    let mut x = sm(SheetMetalOp::Thicken);
    x.faces = vec![top, angled];
    x.bends = vec![EdgeOrFace::Edge(edge_between(sol, pid, &top, &angled).ok_or_else(missing)?)];
    s.run(&AddFeature { element: STUDIO, feature: COVER, base_name: "Sheet metal model".into(), kind: FeatureKind::SheetMetalModel(x) })?;
    // Each model named after its part (SM18.2): the context dropdown lists these names.
    s.run(&RenameFeature { element: STUDIO, feature: ENCLOSURE, name: "Enclosure".into() })?;
    s.run(&RenameFeature { element: STUDIO, feature: COVER, name: "Cover".into() })?;
    s.run(&RenamePart { element: STUDIO, part: ENCLOSURE_PART, name: "Enclosure".into() })?;
    s.run(&RenamePart { element: STUDIO, part: COVER_PART, name: "Cover".into() })?;
    Ok(())
}

/// The document the lesson starts from: the Space Envelope master and an empty Part Studio 1.
pub fn master_document() -> Result<(Document, History), CommandError> {
    let (mut doc, mut h) = empty_document();
    build_master(&mut DocHistory(&mut doc, &mut h))?;
    Ok((doc, h))
}

/// The stand-in document, "Heating Mantle (stand-in)" (mm).
pub fn document() -> Result<(Document, History), CommandError> {
    let (mut doc, mut h) = master_document()?;
    build_studio(&mut DocHistory(&mut doc, &mut h))?;
    Ok((doc, h))
}

fn empty_document() -> (Document, History) {
    let mut doc = Document::empty("Heating Mantle (stand-in)");
    doc.id = DOCUMENT;
    let mut m = crate::document::Element::part_studio("Space Envelope");
    m.id = MASTER;
    let mut p = crate::document::Element::part_studio("Part Studio 1");
    p.id = STUDIO;
    doc.elements.push(m);
    doc.elements.push(p);
    (doc, History::default())
}
