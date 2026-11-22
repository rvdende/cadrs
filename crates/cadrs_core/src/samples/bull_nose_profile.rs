//! "Bull-nose profile" (`fixtures/cnc/bull-nose-profile.cadrs`): brep-bench's case
//! `cnc/bull-nose-profile`, a groove milled round a six-lobed flower by a bull-nose mill in
//! three step-downs, as a part and the CAM job that mills it.
//!
//! | Tab | Kind | What |
//! |---|---|---|
//! | Plate | Part Studio | the plate (0, 0, −16) to (120, 120, 0) less the groove: 8 wide and 10 deep round the flower, its floor's edges rounded to 1.5 |
//! | CAM | CAM Studio | the plate as its own stock on a 3-axis mill; T1 a bull-nose mill ⌀8, corner 1.5: a 2-axis profile inside the groove's outer edge at −4, −8 and −10 |
//!
//! **The flower** is the tip's path: six lobes, arcs of radius 12 about points 30 from
//! (60, 60) at 0°, 60°, …, and six valleys between them, arcs of radius d − 12 about points 40
//! from it at 30°, 90°, …, d = √(2500 − 1200√3) the distance from a lobe's centre to a
//! neighbouring valley's (each valley touches the two lobes beside it). The groove's walls are
//! the flower offset 4 either way (the mill's radius: the same centres, lobes 16 and 8,
//! valleys d − 16 and d − 8), its floor at −10.
//!
//! **The cut.** The profile runs inside the outer wall's top edges, the mill's radius in from
//! them: on the flower, its arcs the edges' own circles ([`cadrs_cam::strategy::arcs`]), so
//! the tool's tip follows the flower's arcs exactly. Simulated, the plate less what the mill
//! passes through is the case's solid: volume
//! 120·120·16 − A·L, A = 80 − (4 − π)·1.5²/2, L the flower's length.
//!
//! Every id is fixed, so the fixture is regenerated exactly (`cadrs_core/tests/bull_nose.rs`).

use cadrs_cam::strategy::{Entry, Height, HeightRef, LeadKind, Pattern, Side};
use cadrs_cam::{Params, Tool, ToolKind};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::applied::{EdgeOrFace, FilletFeature};
use crate::cam::{
    CamId, Edit, EditCam, Job, Mating, OpKind, Operation, Role, Setup, StockDef, ToolOp,
    ToolpathOp, WorkPlane, component_feature, geometry, workspace_component,
};
use crate::command::{CommandError, History};
use crate::commands::{
    AddElement, AddExtrude, AddFeature, AddSketch, EditSketch, NewElementKind, SetExtrude,
};
use crate::document::{BooleanOp, Document, ExtrudeFeature, FaceRef, Offset};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::studio::{DocHistory, Studio};

/// What every id of the document starts with.
const PREFIX: u128 = 0xb011_0005_0000_0000_0000_0000_0000_0000;

const fn element(n: u128) -> ElementId {
    ElementId::from_u128(PREFIX | n)
}

const fn feature(n: u128) -> FeatureId {
    FeatureId::from_u128(PREFIX | (1 << 92) | n)
}

const fn cam_id(n: u64) -> CamId {
    CamId(0xb011_0005_0000_0000 | n)
}

pub const DOCUMENT: DocumentId = DocumentId::from_u128(PREFIX | 0x100);
pub const DOCUMENT_NAME: &str = "Bull-nose profile";

/// The tabs.
pub const PLATE: ElementId = element(0x101);
pub const CAM: ElementId = element(0x102);

/// The Plate's features.
pub const PLATE_SKETCH: FeatureId = feature(1);
pub const PLATE_EXTRUDE: FeatureId = feature(2);
pub const GROOVE_SKETCH: FeatureId = feature(3);
pub const GROOVE_EXTRUDE: FeatureId = feature(4);
pub const FLOOR_FILLET: FeatureId = feature(5);
/// The plate (in its Part Studio).
pub const PART: PartId = PartId {
    feature: PLATE_EXTRUDE,
    index: 0,
};

/// The CAM Studio's component, job, setup and operations.
pub const COMPONENT: CamId = cam_id(0x01);
pub const JOB: CamId = cam_id(0x10);
pub const SETUP: CamId = cam_id(0x20);
pub const MILL: CamId = cam_id(0x31);
pub const PROFILE: CamId = cam_id(0x32);

/// The plate (mm): its corners, the groove's depth and the mill's.
pub const PLATE_MIN: [f64; 3] = [0.0, 0.0, -16.0];
pub const PLATE_MAX: [f64; 3] = [120.0, 120.0, 0.0];
pub const DEPTH: f64 = 10.0;
/// The mill: radius, corner radius, the length of its cutting part.
pub const MILL_RADIUS: f64 = 4.0;
pub const CORNER: f64 = 1.5;
pub const FLUTES: f64 = 25.0;
/// The step-downs: the laps' depths are −4, −8 and −10.
pub const STEP_DOWN: f64 = 4.0;

/// The case's settled answer (brep-bench `cases/cnc/bull-nose-profile.md`).
pub const VOLUME: f64 = 208_489.648_628_807_07;
pub const AREA: f64 = 41_667.558_571_763_68;

/// The distance from a lobe's centre to a neighbouring valley's.
pub fn lobe_to_valley() -> f64 {
    (2500.0 - 1200.0 * 3f64.sqrt()).sqrt()
}

/// The point `r` from (60, 60) at `degrees`.
fn about(r: f64, degrees: f64) -> Vec2 {
    let a = degrees.to_radians();
    Vec2::new(60.0 + r * a.cos(), 60.0 + r * a.sin())
}

/// Lobe `k`'s centre and valley `k`'s (the valley between lobes k and k + 1).
fn lobe(k: usize) -> Vec2 {
    about(30.0, 60.0 * k as f64)
}

fn valley(k: usize) -> Vec2 {
    about(40.0, 30.0 + 60.0 * k as f64)
}

/// The flower offset by `o` outward (lobes 12 + o, valleys d − 12 − o) as a sketch's arcs:
/// lobe k anticlockwise from where valley k − 1 touches it to where valley k does, valley k
/// clockwise on to lobe k + 1 (drawn anticlockwise from that end).
fn flower(o: f64) -> Vec<SketchOp> {
    let d = lobe_to_valley();
    let (rl, rv) = (12.0 + o, d - 12.0 - o);
    // Where lobe k and valley k touch: along the line of their centres (their radii sum to d).
    let touch = |l: usize, v: usize| {
        let (cl, cv) = (lobe(l), valley(v));
        cl + (cv - cl) * (rl / d)
    };
    let mut ops = Vec::new();
    for k in 0..6 {
        let (before, after) = (touch(k, (k + 5) % 6), touch(k, k));
        ops.push(SketchOp::AddArc {
            center: lobe(k),
            start: before,
            end: after,
            construction: false,
        });
        let next = touch((k + 1) % 6, k);
        debug_assert!(((next - valley(k)).length() - rv).abs() < 1e-9);
        ops.push(SketchOp::AddArc {
            center: valley(k),
            start: next,
            end: after,
            construction: false,
        });
    }
    ops
}

fn invalid(what: impl Into<String>) -> CommandError {
    CommandError::Invalid(what.into())
}

fn mm(x: f64) -> String {
    format!("{} mm", (x * 1000.0).round() / 1000.0)
}

/// A sketch on Top of `ops`, and an extrude of its region at `seed` from `lo` to `hi` (Z).
#[allow(clippy::too_many_arguments)]
fn extrude(
    s: &mut dyn Studio,
    ids: (FeatureId, FeatureId),
    ops: Vec<SketchOp>,
    seed: Vec2,
    (lo, hi): (f64, f64),
    op: BooleanOp,
    scope: &[PartId],
) -> Result<(), CommandError> {
    s.run(&AddSketch {
        element: PLATE,
        feature: ids.0,
        plane: Some(PlaneRef::Top),
    })?;
    s.run(&EditSketch {
        element: PLATE,
        feature: ids.0,
        op: SketchOp::Batch(ops),
    })?;
    let g = s
        .document()
        .element(PLATE)
        .and_then(|x| x.feature(ids.0))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| invalid("sketch not found"))?;
    let regions = super::region_refs(ids.0, &g, &[seed]);
    if regions.len() != 1 {
        return Err(invalid(
            "a region of the bull-nose profile sample is missing",
        ));
    }
    let mut x = super::extrude_of(regions, hi - lo);
    x.depth_expr = mm(hi - lo);
    x.op = op;
    x.merge_scope = scope.to_vec();
    if lo.abs() > 1e-12 {
        x.start_offset = Some(Offset {
            value: lo.abs(),
            expr: mm(lo.abs()),
            flip: lo < 0.0,
        });
    }
    s.run(&AddExtrude {
        element: PLATE,
        feature: ids.1,
        extrude: ExtrudeFeature::default(),
    })?;
    s.run(&SetExtrude {
        element: PLATE,
        feature: ids.1,
        extrude: x,
        label: "Extrude".into(),
    })
}

/// The Plate: the block, the groove cut 10 deep between the flower's offsets 4 either way, and
/// the groove's floor's edges rounded to the mill's corner.
pub fn plate(s: &mut dyn Studio) -> Result<(), CommandError> {
    let v = Vec2::new;
    let rect = SketchOp::AddPolyline {
        points: vec![
            v(PLATE_MIN[0], PLATE_MIN[1]),
            v(PLATE_MAX[0], PLATE_MIN[1]),
            v(PLATE_MAX[0], PLATE_MAX[1]),
            v(PLATE_MIN[0], PLATE_MAX[1]),
        ],
        closed: true,
        construction: false,
        label: "Add rectangle",
    };
    extrude(
        s,
        (PLATE_SKETCH, PLATE_EXTRUDE),
        vec![rect],
        v(5.0, 5.0),
        (PLATE_MIN[2], PLATE_MAX[2]),
        BooleanOp::New,
        &[],
    )?;
    let mut groove = flower(MILL_RADIUS);
    groove.extend(flower(-MILL_RADIUS));
    // (The middle of the band: the flower's outermost point.)
    extrude(
        s,
        (GROOVE_SKETCH, GROOVE_EXTRUDE),
        groove,
        v(102.0, 60.0),
        (PLATE_MAX[2] - DEPTH, PLATE_MAX[2]),
        BooleanOp::Remove,
        &[PART],
    )?;
    let features = s
        .document()
        .element(PLATE)
        .map(|e| e.active_features())
        .unwrap_or_default();
    let build = crate::rebuild::build(&features);
    let part = build
        .part(PART)
        .ok_or_else(|| invalid("the plate didn't build"))?;
    let floor = [102.0, 60.0, PLATE_MAX[2] - DEPTH];
    let face = part
        .solid
        .faces
        .iter()
        .find(|f| {
            f.plane
                .is_some_and(|pl| pl.distance(floor).abs() < 1e-6 && pl.normal()[2].abs() > 0.9)
        })
        .ok_or_else(|| invalid("the groove's floor wasn't found"))?;
    s.run(&AddFeature::fillet(
        PLATE,
        FLOOR_FILLET,
        FilletFeature {
            entities: vec![EdgeOrFace::Face(FaceRef {
                part: PART,
                face: face.name,
                seed: floor,
            })],
            size: CORNER,
            size_expr: mm(CORNER),
            ..FilletFeature::default()
        },
    ))
}

/// The CAM Studio: the plate as its own stock on a 3-axis mill, and the profile.
pub fn cam(s: &mut dyn Studio) -> Result<(), CommandError> {
    let edit = |s: &mut dyn Studio, e: Edit| s.run(&EditCam::new(CAM, e));
    let mut c = workspace_component(s.document(), PLATE, PART, Role::Part).map_err(invalid)?;
    c.id = COMPONENT;
    edit(
        s,
        Edit::AddComponents {
            components: vec![c],
            links: vec![],
        },
    )?;
    let parts = || -> Result<Vec<crate::parts::Part>, CommandError> {
        let cam = s
            .document()
            .element(CAM)
            .and_then(|e| e.cam())
            .ok_or_else(|| invalid("not a CAM Studio"))?;
        Ok(crate::rebuild::build(cam.part_features()).parts.clone())
    };
    let part = parts()?
        .into_iter()
        .find(|p| p.id.feature == component_feature(COMPONENT))
        .ok_or_else(|| invalid("the plate's component didn't build"))?;
    edit(
        s,
        Edit::AddJob(Box::new(Job {
            id: JOB,
            name: "Plate".into(),
            bodies: vec![part.id],
            // The plate's own box: the stock is the plate before the groove.
            stock: StockDef {
                offsets: [0.0; 6],
                ..StockDef::default()
            },
            workholdings: vec![],
            machine: None,
            machine_source: None,
            setups: vec![],
            suppressed: false,
        })),
    )?;
    let machine = cadrs_cam::machine::catalog()
        .into_iter()
        .find(|m| m.kind == cadrs_cam::MachineKind::Mill && m.rotary_axes() == 0)
        .ok_or_else(|| invalid("no 3-axis mill in the catalog"))?;
    edit(
        s,
        Edit::SetMachine {
            job: JOB,
            machine: Some(Box::new(machine)),
        },
    )?;
    edit(
        s,
        Edit::AddSetup {
            job: JOB,
            setup: Box::new(Setup {
                id: SETUP,
                name: "Setup 1".into(),
                program_number: 1001,
                mating: Mating::Automatic,
                wcs: WorkPlane::default(),
                operations: vec![],
                suppressed: false,
            }),
        },
    )?;
    let mut tool = Tool::new(ToolKind::BullMill, 2.0 * MILL_RADIUS);
    tool.corner_radius = CORNER;
    tool.cutting_length = FLUTES;
    tool.name = "⌀8 bull-nose, corner 1.5".into();
    let op = |id, name: String, kind| Operation {
        id,
        name,
        kind,
        suppressed: false,
        hidden: false,
    };
    edit(
        s,
        Edit::AddOperation {
            setup: SETUP,
            after: None,
            op: Box::new(op(
                MILL,
                tool.name.clone(),
                OpKind::Tool(Box::new(ToolOp {
                    tool: tool.clone(),
                    number: 1,
                })),
            )),
        },
    )?;
    // The outer wall's top edges: the flower offset 4 outward at the plate's top (lobes 16
    // about the lobes' centres, valleys d − 16 about the valleys').
    let d = lobe_to_valley();
    let on = |c: &crate::solid::EdgeCircle, centre: Vec2, r: f64| {
        (c.center[0] - centre.x).hypot(c.center[1] - centre.y) < 1e-6
            && (c.radius - r).abs() < 1e-6
            && (c.center[2] - PLATE_MAX[2]).abs() < 1e-6
    };
    let edges: Vec<String> = part
        .solid
        .edges
        .iter()
        .filter(|e| {
            e.circle.as_ref().is_some_and(|c| {
                (0..6).any(|k| {
                    on(c, lobe(k), 12.0 + MILL_RADIUS) || on(c, valley(k), d - 12.0 - MILL_RADIUS)
                })
            })
        })
        .map(|e| geometry::edge_key(part.id, &e.name))
        .collect();
    if edges.len() != 12 {
        return Err(invalid(format!(
            "the groove's outer top edges: {} found, not 12",
            edges.len()
        )));
    }
    let mut p = Params::new(Pattern::Profile2D, &tool, cadrs_cam::MachineKind::Mill);
    p.drive.edges = edges;
    p.side = Side::Inside;
    p.wall_stock = 0.0;
    p.floor_stock = 0.0;
    p.passes = 1;
    p.step_down = STEP_DOWN;
    p.first_step_down = 0.0;
    p.heights.top = Height::new(HeightRef::StockTop, 0.0);
    p.heights.bottom = Height::new(HeightRef::StockTop, -DEPTH);
    // Straight down onto the flower, round it, and up (no leads, no tabs).
    p.links.entry = Entry::Plunge;
    p.links.lead_in = LeadKind::None;
    p.links.lead_out = LeadKind::None;
    p.tabs.enabled = false;
    p.smooth_corners = 0.0;
    p.rest = false;
    let name = s
        .document()
        .element(CAM)
        .and_then(|e| e.cam())
        .map(|c| c.next_name(Pattern::Profile2D.label()))
        .unwrap_or_else(|| "2-axis profile 1".into());
    edit(
        s,
        Edit::AddOperation {
            setup: SETUP,
            after: None,
            op: Box::new(op(
                PROFILE,
                name,
                OpKind::Toolpath(Box::new(ToolpathOp {
                    params: p,
                    name_edited: false,
                })),
            )),
        },
    )
}

/// Adds the CAM tab and fills both tabs of a document whose first tab is the empty Part Studio
/// [`PLATE`].
pub fn build_in(s: &mut dyn Studio) -> Result<(), CommandError> {
    s.run(&AddElement {
        id: CAM,
        kind: NewElementKind::CamStudio,
        name: Some("CAM".into()),
        after: Some(PLATE),
    })?;
    plate(s)?;
    cam(s)
}

/// A new document holding the sample, as `fixtures/cnc/bull-nose-profile.cadrs` stores it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(DOCUMENT_NAME);
    doc.id = DOCUMENT;
    doc.units.length = cadrs_sketch::units::LengthUnit::Millimeter;
    let mut el = crate::document::Element::part_studio("Plate");
    el.id = PLATE;
    doc.elements.push(el);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h))?;
    Ok(doc)
}

/// The sample as a document file, dated 2026-11-20.
pub fn file(document: Document) -> crate::store::DocumentFile {
    let now = 1_795_132_800;
    crate::store::DocumentFile {
        version: crate::store::SCHEMA_VERSION,
        meta: crate::library::DocumentMeta::new("cadrs", now),
        document,
    }
}
