//! "CNC machines" (issue #278): the fixture for multi-axis CAM. Three milling machines modelled
//! as assemblies whose mates are their axes, a test piece with features for each of them, and
//! a CAM Studio per machine.
//!
//! | Tab | Kind | What |
//! |---|---|---|
//! | Machine parts | Part Studio | every machine's parts ([`parts`]), each at its home position |
//! | Workholding | Part Studio | a vise, a chuck and a fixture plate round the Test piece ([`work`]) |
//! | Test piece | Part Studio | the part to mill ([`work`]) |
//! | Worm | Part Studio | a worm on a chucking spigot, for the 4th axis ([`work`]) |
//! | 3-axis mill | Assembly | sliders `X`, `Y`, `Z` ([`assemblies::mill3`]) |
//! | 4-axis mill | Assembly | the same with a rotary chuck `A` on the table and a tool `Magazine` ([`assemblies::mill4`]) |
//! | 5-axis mill | Assembly | sliders `X`, `Y`, `Z`, a rotary table `C` and a tilting head `B` ([`assemblies::mill5`]) |
//! | CAM 3-axis, CAM 4-axis, CAM 5-axis | CAM Studio | jobs on the Test piece and the Worm ([`cam`]) |
//!
//! Units are mm. The machines' origin is the centre of the table's top at home, Z up, the
//! column behind the table (+Y). The Test piece and its workholding have the piece's own
//! origin (its axis along X), as CAM components are not placed. The workholding is sized to
//! the machines, so the piece goes on each like this:
//!
//! | Machine | Held by | The piece's origin and axes on the machine |
//! |---|---|---|
//! | 3-axis | the Vise, its underside (the piece's z −45) on the table | 45 over the table's top, axes as the machine's |
//! | 4-axis | the Chuck, which is the machine's own chuck moved 70 along X | (−70, 0, 100): on the `A` axis, the block against the jaws, axes as the machine's |
//! | 5-axis | the Fixture plate, its underside (the piece's x −25) on the platter | (0, 0, 135): on the `C` axis, the piece's +X up the machine's +Z |
//!
//! Every id is fixed, so `fixtures/cnc_machines.cadrs` is regenerated exactly
//! (`cadrs_core/tests/cnc_machines.rs`).

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::{CommandError, History};
use crate::commands::{
    AddElement, AddExtrude, AddRevolve, AddSketch, EditSketch, NewElementKind, SetExtrude,
};
use crate::document::{
    AxisRef, BooleanOp, Document, ExtrudeFeature, Offset, RevolveFeature, RevolveType,
};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::studio::{DocHistory, Studio};

pub mod assemblies;
pub mod cam;
pub mod machines;
pub mod parts;
pub mod work;

/// What every id of the document starts with.
const PREFIX: u128 = 0xc4c0_0278_0000_0000_0000_0000_0000_0000;

const fn element(n: u128) -> ElementId {
    ElementId::from_u128(PREFIX | n)
}

/// A feature of the Part Studio numbered `studio` (1 Machine parts, 2 Workholding, 3 Test
/// piece, 4 Worm).
const fn feature(studio: u128, n: u128) -> FeatureId {
    FeatureId::from_u128(PREFIX | (studio << 92) | n)
}

pub const DOCUMENT: DocumentId = DocumentId::from_u128(PREFIX | 0x100);
pub const DOCUMENT_NAME: &str = "CNC machines";

/// The Part Studios.
pub const MACHINE_PARTS: ElementId = element(0x101);
pub const WORKHOLDING: ElementId = element(0x102);
pub const TEST_PIECE: ElementId = element(0x103);
pub const WORM: ElementId = element(0x104);
/// The assemblies.
pub const MILL_3: ElementId = element(0x105);
pub const MILL_4: ElementId = element(0x106);
pub const MILL_5: ElementId = element(0x107);
/// The CAM Studios.
pub const CAM_3: ElementId = element(0x108);
pub const CAM_4: ElementId = element(0x109);
pub const CAM_5: ElementId = element(0x10a);

/// The tabs after the first, in order: id, kind, name.
const TABS: [(ElementId, NewElementKind, &str); 9] = [
    (WORKHOLDING, NewElementKind::PartStudio, "Workholding"),
    (TEST_PIECE, NewElementKind::PartStudio, "Test piece"),
    (WORM, NewElementKind::PartStudio, "Worm"),
    (MILL_3, NewElementKind::Assembly, "3-axis mill"),
    (MILL_4, NewElementKind::Assembly, "4-axis mill"),
    (MILL_5, NewElementKind::Assembly, "5-axis mill"),
    (CAM_3, NewElementKind::CamStudio, "CAM 3-axis"),
    (CAM_4, NewElementKind::CamStudio, "CAM 4-axis"),
    (CAM_5, NewElementKind::CamStudio, "CAM 5-axis"),
];

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    poly(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

fn poly(points: &[(f64, f64)]) -> SketchOp {
    SketchOp::AddPolyline {
        points: points.iter().map(|(x, y)| v(*x, *y)).collect(),
        closed: true,
        construction: false,
        label: "Add polygon",
    }
}

fn circle(x: f64, y: f64, d: f64) -> SketchOp {
    SketchOp::AddCircle {
        center: v(x, y),
        radius: d / 2.0,
        construction: false,
    }
}

/// A rectangle lying along the direction at `angle` (degrees) from `centre`: from `r0` to `r1`
/// out along it, `width` across (a chuck's jaw, a radial slot, a flat's cutter).
fn radial_rect(centre: (f64, f64), angle: f64, r0: f64, r1: f64, width: f64) -> SketchOp {
    let (s, c) = angle.to_radians().sin_cos();
    let h = width / 2.0;
    let at = |r: f64, w: f64| (centre.0 + r * c - w * s, centre.1 + r * s + w * c);
    poly(&[at(r0, -h), at(r1, -h), at(r1, h), at(r0, h)])
}

/// The point `r` from `centre` along the direction at `angle` (degrees).
fn polar(centre: (f64, f64), angle: f64, r: f64) -> Vec2 {
    let (s, c) = angle.to_radians().sin_cos();
    v(centre.0 + r * c, centre.1 + r * s)
}

/// Which plane a sketch is on: its 2D axes are (X, Y) on Top, (X, Z) on Front and (Y, Z) on
/// Right; extrudes run along Z, Y and X.
#[derive(Clone, Copy)]
enum On {
    Top,
    Front,
    Right,
}

impl On {
    fn plane(self) -> PlaneRef {
        match self {
            On::Top => PlaneRef::Top,
            On::Front => PlaneRef::Front,
            On::Right => PlaneRef::Right,
        }
    }
}

fn mm(x: f64) -> String {
    format!("{} mm", (x * 1000.0).round() / 1000.0)
}

fn sketch(
    s: &mut dyn Studio,
    el: ElementId,
    id: FeatureId,
    on: On,
    ops: Vec<SketchOp>,
) -> Result<cadrs_sketch::Sketch, CommandError> {
    s.run(&AddSketch {
        element: el,
        feature: id,
        plane: Some(on.plane()),
    })?;
    s.run(&EditSketch {
        element: el,
        feature: id,
        op: SketchOp::Batch(ops),
    })?;
    s.document()
        .element(el)
        .and_then(|x| x.feature(id))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

/// A sketch of `ops` (the feature `ids.0`) and an extrude (`ids.1`) of its regions at `seeds`
/// over `range` along the plane's axis (Z on Top, Y on Front, X on Right; (low, high) in that
/// axis).
#[allow(clippy::too_many_arguments)]
fn solid(
    s: &mut dyn Studio,
    el: ElementId,
    ids: (FeatureId, FeatureId),
    on: On,
    ops: Vec<SketchOp>,
    seeds: &[Vec2],
    range: (f64, f64),
    op: BooleanOp,
    scope: &[PartId],
) -> Result<(), CommandError> {
    let g = sketch(s, el, ids.0, on, ops)?;
    let regions = super::region_refs(ids.0, &g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid(format!(
            "a region of the CNC machines sample is missing ({:?})",
            ids.1
        )));
    }
    let (lo, hi) = range;
    // Where the extrude starts along the plane's normal, and whether against it (Front's
    // normal is −Y: it starts at the high end).
    let (start, depth) = match on {
        On::Front => (-hi, hi - lo),
        _ => (lo, hi - lo),
    };
    let mut x = super::extrude_of(regions, depth);
    x.depth_expr = mm(depth);
    x.op = op;
    x.merge_scope = scope.to_vec();
    if start.abs() > 1e-12 {
        x.start_offset = Some(Offset {
            value: start.abs(),
            expr: mm(start.abs()),
            flip: start < 0.0,
        });
    }
    s.run(&AddExtrude {
        element: el,
        feature: ids.1,
        extrude: ExtrudeFeature::default(),
    })?;
    s.run(&SetExtrude {
        element: el,
        feature: ids.1,
        extrude: x,
        label: "Extrude".into(),
    })
}

/// A sketch of `ops` and a construction line from `axis.0` to `axis.1` (the feature `ids.0`),
/// and a full revolve (`ids.1`) of the region at `seed` about that line.
#[allow(clippy::too_many_arguments)]
fn turned(
    s: &mut dyn Studio,
    el: ElementId,
    ids: (FeatureId, FeatureId),
    on: On,
    axis: (Vec2, Vec2),
    mut ops: Vec<SketchOp>,
    seed: Vec2,
    op: BooleanOp,
    scope: &[PartId],
) -> Result<(), CommandError> {
    ops.insert(
        0,
        SketchOp::AddPolyline {
            points: vec![axis.0, axis.1],
            closed: false,
            construction: true,
            label: "Add line",
        },
    );
    let g = sketch(s, el, ids.0, on, ops)?;
    let line = g
        .curves
        .iter()
        .find(|(_, c)| c.construction)
        .map(|(k, _)| k)
        .ok_or_else(|| CommandError::Invalid("a revolve's axis is missing".into()))?;
    let regions = super::region_refs(ids.0, &g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid(format!(
            "a profile of the CNC machines sample is missing ({:?})",
            ids.1
        )));
    }
    s.run(&AddRevolve {
        element: el,
        feature: ids.1,
        revolve: RevolveFeature {
            regions,
            axis: Some(AxisRef::SketchCurve {
                sketch: ids.0,
                curve: line,
            }),
            kind: RevolveType::Full,
            angle: 360.0,
            angle_expr: "360 deg".into(),
            op,
            merge_scope: scope.to_vec(),
            ..RevolveFeature::default()
        },
    })
}

/// Adds everything to a document whose first tab is the empty Part Studio [`MACHINE_PARTS`]:
/// the other tabs, the parts, the assemblies and the CAM Studios.
pub fn build_in(s: &mut dyn Studio) -> Result<(), CommandError> {
    let mut after = MACHINE_PARTS;
    for (id, kind, name) in TABS {
        s.run(&AddElement {
            id,
            kind,
            name: Some(name.into()),
            after: Some(after),
        })?;
        after = id;
    }
    parts::build_in(s, MACHINE_PARTS)?;
    work::test_piece(s, TEST_PIECE)?;
    work::worm(s, WORM)?;
    work::workholding(s, WORKHOLDING)?;
    assemblies::mill3::build_in(s)?;
    assemblies::mill4::build_in(s)?;
    assemblies::mill5::build_in(s)?;
    machines::build_in(s)?;
    cam::cam3::build_in(s)?;
    cam::cam4::build_in(s)?;
    cam::cam5::build_in(s)
}

/// A new document holding the sample, as `fixtures/cnc_machines.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(DOCUMENT_NAME);
    doc.id = DOCUMENT;
    doc.units.length = cadrs_sketch::units::LengthUnit::Millimeter;
    let mut el = crate::document::Element::part_studio("Machine parts");
    el.id = MACHINE_PARTS;
    doc.elements.push(el);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h))?;
    Ok(doc)
}

/// The sample as a document file (`fixtures/cnc_machines.cadrs`), dated 2026-11-18.
pub fn file(document: Document) -> crate::store::DocumentFile {
    let now = 1_794_960_000;
    crate::store::DocumentFile {
        version: crate::store::SCHEMA_VERSION,
        meta: crate::library::DocumentMeta::new("cadrs", now),
        document,
    }
}
