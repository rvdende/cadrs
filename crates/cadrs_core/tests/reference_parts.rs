//! Reference parts (issue #101): parts with expected values worked out independently of cadrs
//! (closed-form geometry, or an independent kernel), rebuilt through cadrs' feature list and
//! compared. See `tests/reference_parts/README.md`.
//!
//! The cases are SolidCraft's (`tests/reference_parts/solidcraft/<case>/`: `recipe.json` and
//! `expected.json`, MIT), and parts from cadrs' issues (`tests/reference_parts/issues/<case>/`:
//! the user's `document.ron`, and `target.step`, the part as another CAD system builds it).
//! [`Recipe`] turns a recipe into cadrs features through the document's
//! commands (the same ones the app and its MCP tools run); what the recipe format means
//! follows SolidCraft's reader (`apps/solidcraft-cli/src/recipe.rs`), and the comparison its
//! `oracle.rs`. A case using something not translated yet is reported as such, not failed.
//! (With a command registry, #102, this becomes a recipe → registry mapping, as SolidCraft's is.)
//!
//! Each case is a test of its own (`case_49_helical_sweep` for `49-helical-sweep`,
//! `issue_156_face_chamfer_fillet` for `issues/156-face-chamfer-fillet`), which fails if the
//! case doesn't pass and must: an issue's case always, SolidCraft's once `passing.txt` lists it. `CADRS_BLESS=1` records the cases passing
//! now and writes `docs/reference-parts.md`.
#![cfg(feature = "brep")]
#![allow(clippy::field_reassign_with_default)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use cadrs_core::advanced::{
    LoftFeature, LoftProfile, PathRef, SplitFeature, SplitToolRef, SweepFeature,
};
use cadrs_core::applied::{
    ChamferFeature, EdgeOrFace, EdgePoint, FilletFeature, FilletMeasurement, HoleFeature,
    HolePoint, ShellFeature, VertexRadius,
};
use cadrs_core::commands::{
    AddExtrude, AddFeature, AddRevolve, AddSketch, DeleteFeature, EditSketch, ReplaceFeature,
    SetExtrude, SetRevolve,
};
use cadrs_core::document::{
    AxisRef, BodyType, BooleanFeature, BooleanKind, BooleanOp, DeletePartFeature, DirectionRef,
    Document, EdgeRef, EndCondition, EndType, ExtrudeFeature, FaceRef, FeatureKind, RegionRef,
    RevolveFeature, RevolveType, ThinWall, VertexRef,
};
use cadrs_core::draft::{DraftFeature, ExtrudeDraft};
use cadrs_core::hole::{HoleEnd, HoleSpec, HoleStyle, Length};
use cadrs_core::import::{ImportAs, ImportFormat, ImportIds, imported_document};
use cadrs_core::mate::{ConnectorOrigin, ConnectorRef};
use cadrs_core::parts::PartKind;
use cadrs_core::pattern::{MirrorFeature, MirrorPlane, PatternFeature, PatternKind, PatternType};
use cadrs_core::plane::{PlaneEntity, PlaneFeature, PlaneType};
use cadrs_core::rebuild::Rebuilder;
use cadrs_core::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};
use cadrs_core::sheetmetal::{
    CurveRef as SmCurveRef, SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp,
};
use cadrs_core::sheetmetal_features::{FlangeFeature, HemFeature, SheetMetalFeature};
use cadrs_core::sheetmetal_flat::FlatExtrudeFeature;
use cadrs_core::surfacing::{FillEdge, FillFeature, ThickenFeature};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, PartId, Store};
use cadrs_sheetmetal::model::HemAlignment;
use cadrs_sheetmetal::sharp_edit::{FlangeAlignment, HemKind};
use cadrs_sketch::region::regions;
use cadrs_sketch::{
    ConstraintSpec, CurveId, CurveKind, CurveRef, CurveSpec, Dimension, DimensionKind,
    FeaturePlane, Orient, PlaneRef, PointId, PointRef, PointSpec, Sketch, SketchOp, Vec2,
};

/// Volume and area: relative tolerance unless the case gives one (SolidCraft's default).
const REL_TOL: f64 = 1e-3;
/// Centre of mass and bounding box: this fraction of the bounding box's diagonal.
const ABS_FRAC: f64 = 1e-3;
/// The mesh: each face's triangles cover its exact area within this fraction of it (curved
/// faces' chords keep the triangles a little short of it).
const MESH_TOL: f64 = 1e-2;

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    scale(a, 1.0 / norm(a).max(1e-300))
}

fn v3(v: &Value) -> Option<V3> {
    let a = v.as_array()?;
    Some([
        a.first()?.as_f64()?,
        a.get(1)?.as_f64()?,
        a.get(2)?.as_f64()?,
    ])
}
fn v2(v: &Value) -> Option<[f64; 2]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}

/// Why a case wasn't built: not translated yet, or cadrs failed on it.
enum Stop {
    Unsupported(String),
    Failed(String),
}

type R<T> = Result<T, Stop>;

fn unsupported<T>(what: impl Into<String>) -> R<T> {
    Err(Stop::Unsupported(what.into()))
}
fn failed<T>(what: impl Into<String>) -> R<T> {
    Err(Stop::Failed(what.into()))
}

/// A plane frame: origin, sketch axes and normal.
#[derive(Clone, Copy, Debug)]
struct Frame {
    o: V3,
    x: V3,
    y: V3,
    n: V3,
}

/// The default planes: Top (x, y), Front (x, z; normal −y), Right (y, z).
const STD: [(PlaneRef, V3, V3); 3] = [
    (PlaneRef::Top, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    (PlaneRef::Front, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    (PlaneRef::Right, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
];

/// A recipe sketch as built: its feature, the recipe's frame and the cadrs plane's, and each
/// recipe entity's cadrs curve or point.
struct SketchInfo {
    id: FeatureId,
    recipe: Frame,
    cadrs: Frame,
    ents: HashMap<String, Ent>,
}

/// A recipe sketch entity in cadrs: its curve (or point), and whether drawing it
/// counter-clockwise swapped an arc's start and end.
#[derive(Clone, Copy)]
struct Ent {
    curve: Option<CurveId>,
    point: Option<PointId>,
    swapped: bool,
}

struct Recipe {
    d: Document,
    h: History,
    el: ElementId,
    sketches: HashMap<String, SketchInfo>,
    features: HashMap<String, FeatureId>,
    bodies: HashMap<String, PartId>,
    /// Construction planes: name → (cadrs plane, frame).
    planes: HashMap<String, (PlaneRef, Frame)>,
    /// Offset planes made for sketches and sections, by (default plane, offset).
    offsets: Vec<(usize, f64, PlaneRef)>,
    params: HashMap<String, f64>,
    notes: Vec<String>,
    /// Sketches of a fitted spline through points on a helix, made a Helix feature instead.
    helices: HashMap<String, FeatureId>,
    /// The sheet metal model and its rule as now edited.
    sheet: Option<(FeatureId, Rule)>,
    /// Unfolded: sketches parallel to the flat go on the flat pattern's plane, and cuts of them
    /// are flat extrudes.
    flat: Option<(PlaneRef, Frame)>,
    flat_sketches: Vec<FeatureId>,
}

/// A sheet metal rule (SolidCraft's, as its recipes name and edit them).
#[derive(Clone, Copy)]
struct Rule {
    thickness: f64,
    /// The inner bend radius; `None`: the thickness.
    bend_radius: Option<f64>,
    k_factor: f64,
}

impl Rule {
    /// "Steel (mm)": 2.5 thick, the bend radius the thickness, K 0.44 (reference part 74's
    /// bend allowance (π/2)(R + K t) = 5.654866776), a minimal gap of 0.01 (73's flat hem,
    /// 75's flange beside another).
    fn named(name: &str) -> R<Rule> {
        match name {
            "Steel (mm)" => Ok(Rule {
                thickness: 2.5,
                bend_radius: None,
                k_factor: 0.44,
            }),
            other => unsupported(format!("sheet metal rule `{other}`")),
        }
    }

    fn params(&self) -> cadrs_sheetmetal::Params {
        let mut p = SheetMetalModelFeature::default_params();
        p.thickness = self.thickness;
        p.bend_radius = self.bend_radius.unwrap_or(self.thickness);
        p.k_factor = self.k_factor;
        p.minimal_gap = 0.01;
        p
    }
}

impl Recipe {
    fn new() -> Self {
        let d = Document::new("Reference part");
        let el = d.elements[0].id;
        Self {
            d,
            h: History::default(),
            el,
            sketches: HashMap::new(),
            features: HashMap::new(),
            bodies: HashMap::new(),
            planes: HashMap::new(),
            offsets: Vec::new(),
            params: HashMap::new(),
            notes: Vec::new(),
            helices: HashMap::new(),
            sheet: None,
            flat: None,
            flat_sketches: Vec::new(),
        }
    }

    fn features_now(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn exec(&mut self, c: &dyn cadrs_core::Command) -> R<()> {
        self.h
            .execute(&mut self.d, c)
            .map_err(|e| Stop::Failed(format!("command: {e}")))
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> R<FeatureId> {
        let feature = FeatureId::new();
        self.exec(&AddFeature {
            element: self.el,
            feature,
            base_name: base.into(),
            kind,
        })?;
        Ok(feature)
    }

    /// The parts now, failing if the last feature didn't build.
    fn parts(&self, after: FeatureId) -> R<Vec<Part>> {
        let b = cadrs_core::rebuild::build(&self.features_now());
        if let Some(e) = b.error(after) {
            return failed(e.to_string());
        }
        Ok(b.parts.clone())
    }

    fn num(&self, v: Option<&Value>) -> R<f64> {
        match v {
            Some(Value::Number(n)) => Ok(n.as_f64().unwrap_or(f64::NAN)),
            Some(Value::String(s)) => {
                eval(s, &self.params).map_or_else(|| unsupported(format!("expression `{s}`")), Ok)
            }
            _ => unsupported("a missing number"),
        }
    }

    /// The cadrs plane for a frame: a default plane, or an offset Plane feature parallel to one.
    fn cadrs_plane(&mut self, o: V3, n: V3) -> R<(PlaneRef, Frame)> {
        let n = unit(n);
        let Some((k, (pr, x, y))) = STD
            .iter()
            .enumerate()
            .find(|(_, (_, x, y))| dot(cross(*x, *y), n).abs() > 1.0 - 1e-9)
        else {
            return unsupported("a plane not parallel to a default plane");
        };
        let nc = cross(*x, *y);
        let off = dot(o, nc);
        let frame = Frame {
            o: scale(nc, off),
            x: *x,
            y: *y,
            n: nc,
        };
        if off.abs() < 1e-9 {
            return Ok((*pr, frame));
        }
        if let Some((.., p)) = self
            .offsets
            .iter()
            .find(|(kk, oo, _)| *kk == k && (oo - off).abs() < 1e-9)
        {
            return Ok((*p, frame));
        }
        let id = self.add(
            "Plane",
            FeatureKind::Plane(PlaneFeature {
                kind: PlaneType::Offset,
                entities: vec![PlaneEntity::Plane(*pr)],
                offset: off,
                offset_expr: format!("{off} mm"),
                ..PlaneFeature::default()
            }),
        )?;
        let p = cadrs_core::parts::plane_feature_ref(&self.features_now(), id)
            .ok_or_else(|| Stop::Failed("the offset plane didn't build".into()))?;
        self.offsets.push((k, off, p));
        Ok((p, frame))
    }

    /// A plane `angle` degrees from `base_plane` about a world axis, as a Line angle plane: the
    /// axis drawn as a construction line in a sketch on a default plane that holds it. The
    /// angle's way round is the one that gives the recipe's `result_normal` when it has one.
    fn angled_plane(&mut self, f: &Value) -> R<(PlaneRef, Frame)> {
        let (on, end) = match f["axis"].as_str().unwrap_or("") {
            "world X axis" => (PlaneRef::Top, Vec2::new(1.0, 0.0)),
            "world Y axis" => (PlaneRef::Top, Vec2::new(0.0, 1.0)),
            "world Z axis" => (PlaneRef::Front, Vec2::new(0.0, 1.0)),
            other => return unsupported(format!("a plane at an angle about `{other}`")),
        };
        let base = self.plane_ref(&f["base_plane"])?;
        let angle = self.num(f.get("angle"))?;
        let sketch = FeatureId::new();
        self.exec(&AddSketch {
            element: self.el,
            feature: sketch,
            plane: Some(on),
        })?;
        self.exec(&EditSketch {
            element: self.el,
            feature: sketch,
            op: SketchOp::AddPolyline {
                points: vec![Vec2::new(0.0, 0.0), end],
                closed: false,
                construction: true,
                label: "Add line",
            },
        })?;
        let curve = self.geometry(sketch).curves.keys().next().unwrap();
        let want = f.get("result_normal").and_then(v3).map(unit);
        for flip in [false, true] {
            let id = self.add(
                "Plane",
                FeatureKind::Plane(PlaneFeature {
                    kind: PlaneType::LineAngle,
                    entities: vec![
                        PlaneEntity::SketchCurve { sketch, curve },
                        PlaneEntity::Plane(base),
                    ],
                    angle,
                    angle_expr: format!("{angle} deg"),
                    flip,
                    ..PlaneFeature::default()
                }),
            )?;
            let Some(PlaneRef::Feature(fp)) =
                cadrs_core::parts::plane_feature_ref(&self.features_now(), id)
            else {
                return failed("the angled plane didn't build");
            };
            let n = unit(cross(fp.u, fp.v));
            if want.is_none_or(|w| dot(w, n).abs() > 1.0 - 1e-9) {
                let frame = Frame {
                    o: fp.origin,
                    x: fp.u,
                    y: fp.v,
                    n,
                };
                return Ok((PlaneRef::Feature(fp), frame));
            }
            self.exec(&DeleteFeature {
                element: self.el,
                feature: id,
                label: "Delete Plane".into(),
            })?;
        }
        failed("no way round the angle gives the recipe's plane")
    }

    /// A recipe plane (`{ref, origin, x_dir, y_dir, normal}`) as its frame.
    fn recipe_frame(&self, pl: &Value) -> R<Frame> {
        let r = pl.get("ref").and_then(Value::as_str).unwrap_or("");
        let std = |x: V3, y: V3| Frame {
            o: [0.0; 3],
            x,
            y,
            n: cross(x, y),
        };
        let (Some(x), Some(y)) = (pl.get("x_dir").and_then(v3), pl.get("y_dir").and_then(v3))
        else {
            // SolidCraft's default frames (`recipe.rs`).
            return match r.to_ascii_uppercase().as_str() {
                "XY" => Ok(std([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])),
                "XZ" => Ok(std([1.0, 0.0, 0.0], [0.0, 0.0, -1.0])),
                "YZ" => Ok(std([0.0, 1.0, 0.0], [0.0, 0.0, 1.0])),
                _ => unsupported(format!("plane `{r}` without axes")),
            };
        };
        let o = pl.get("origin").and_then(v3).unwrap_or([0.0; 3]);
        Ok(Frame {
            o,
            x,
            y,
            n: cross(x, y),
        })
    }

    fn sketch(&mut self, f: &Value) -> R<()> {
        let name = f["name"].as_str().unwrap_or("").to_string();
        // A spline fitted through points on a helix about the world Z axis (as SolidCraft
        // replays it): the true helix, a Helix feature.
        if let [e] = f["entities"].as_array().map_or(&[][..], Vec::as_slice)
            && e["type"] == "fitted_spline"
        {
            let pts: Vec<V3> = e["fit_points"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(v3)
                .collect();
            let Some(h) = helix_through(&pts) else {
                return unsupported("a fitted spline that isn't a helix");
            };
            let id = self.add("Helix", FeatureKind::Helix(h))?;
            self.helices.insert(name, id);
            return Ok(());
        }
        let rf = self.recipe_frame(&f["plane"])?;
        // While unfolded, on the flat pattern's plane (its coordinates the flat's, laid on the
        // stationary wall).
        let (pr, cf) = match self.flat {
            Some((pr, cf)) if dot(cf.n, unit(rf.n)).abs() > 1.0 - 1e-9 => (pr, cf),
            _ => self.cadrs_plane(rf.o, rf.n)?,
        };
        let id = FeatureId::new();
        self.exec(&AddSketch {
            element: self.el,
            feature: id,
            plane: Some(pr),
        })?;
        if self.flat.is_some_and(|(p, _)| p == pr) {
            self.flat_sketches.push(id);
        }
        // Recipe sketch coordinates → cadrs sketch coordinates, through world space.
        let map = |p: [f64; 2]| -> Vec2 {
            let w = add(add(rf.o, scale(rf.x, p[0])), scale(rf.y, p[1]));
            let d = sub(w, cf.o);
            Vec2::new(dot(d, cf.x), dot(d, cf.y))
        };
        let pt = |v: &Value| -> R<[f64; 2]> {
            v.get("at")
                .and_then(v2)
                .or_else(|| v2(v))
                .map_or_else(|| unsupported("a sketch point without `at`"), Ok)
        };
        let mut ents: HashMap<String, Ent> = HashMap::new();
        for e in f["entities"].as_array().into_iter().flatten() {
            let eid = e["id"].as_str().unwrap_or("").to_string();
            let construction = e["construction"].as_bool().unwrap_or(false);
            let (op, swapped) = match e["type"].as_str().unwrap_or("") {
                "line" => (
                    SketchOp::AddPolyline {
                        points: vec![map(pt(&e["start"])?), map(pt(&e["end"])?)],
                        closed: false,
                        construction,
                        label: "Add line",
                    },
                    false,
                ),
                "circle" => (
                    SketchOp::AddCircle {
                        center: map(pt(&e["center"])?),
                        radius: self.num(e.get("radius"))?,
                        construction,
                    },
                    false,
                ),
                "arc_three_point" | "arc_center_start_sweep" => {
                    // Three recipe points along the arc: start, middle, end.
                    let (s, m, e2) = if e["type"] == "arc_three_point" {
                        (
                            pt(&e["start"])?,
                            pt(e.get("mid").or(e.get("through")).unwrap_or(&Value::Null))?,
                            pt(&e["end"])?,
                        )
                    } else {
                        let c = pt(&e["center"])?;
                        let s = pt(&e["start"])?;
                        let sweep = self
                            .num(e.get("sweep_deg").or(e.get("sweep")))?
                            .to_radians();
                        let rot = |t: f64| {
                            let (dx, dy) = (s[0] - c[0], s[1] - c[1]);
                            [
                                c[0] + dx * t.cos() - dy * t.sin(),
                                c[1] + dx * t.sin() + dy * t.cos(),
                            ]
                        };
                        (s, rot(sweep / 2.0), rot(sweep))
                    };
                    let (s, m, e2) = (map(s), map(m), map(e2));
                    let Some(c) = circumcentre(s, m, e2) else {
                        return failed(format!(
                            "{name}/{eid}: an arc through three collinear points"
                        ));
                    };
                    // AddArc runs counter-clockwise: from the end whose way round passes the
                    // middle (the recipe's start and end then swap places).
                    let ang = |p: Vec2| (p.y - c.y).atan2(p.x - c.x);
                    let ccw = |a: f64, b: f64| (b - a).rem_euclid(std::f64::consts::TAU);
                    let swapped = ccw(ang(s), ang(m)) >= ccw(ang(s), ang(e2));
                    let (from, to) = if swapped { (e2, s) } else { (s, e2) };
                    (
                        SketchOp::AddArc {
                            center: c,
                            start: from,
                            end: to,
                            construction,
                        },
                        swapped,
                    )
                }
                "point" => (SketchOp::AddPoint { pos: map(pt(e)?) }, false),
                other => return unsupported(format!("sketch entity `{other}`")),
            };
            let before = self.geometry(id);
            let is_point = matches!(op, SketchOp::AddPoint { .. });
            self.exec(&EditSketch {
                element: self.el,
                feature: id,
                op,
            })?;
            let g = self.geometry(id);
            let ent = if is_point {
                let p = g.points.keys().find(|k| !before.points.contains_key(*k));
                Ent {
                    curve: None,
                    point: p,
                    swapped,
                }
            } else {
                let c = g.curves.keys().find(|k| !before.curves.contains_key(*k));
                if c.is_none() {
                    return failed(format!("{name}/{eid}: drawn over another curve"));
                }
                Ent {
                    curve: c,
                    point: None,
                    swapped,
                }
            };
            ents.insert(eid, ent);
        }
        // Constraints: all at once, on the geometry as drawn; then the dimensions.
        let info = SketchInfo {
            id,
            recipe: rf,
            cadrs: cf,
            ents,
        };
        // A recipe "horizontal" is along the recipe's x axis: cadrs' u, or its v when turned.
        let turned = dot(rf.x, cf.x).abs() < 0.5;
        let origin = map([0.0, 0.0]);
        if (origin.x.abs() + origin.y.abs()) > 1e-9
            && (f["constraints"].to_string().contains("origin")
                || f["dimensions"].to_string().contains("origin"))
        {
            return unsupported("a sketch origin away from the cadrs plane's");
        }
        let g = self.geometry(id);
        let mut specs = Vec::new();
        // An entity's point that references the origin is the sketch's fixed origin, as
        // SolidCraft's reader shares it: coincident with it, not only drawn there.
        for e in f["entities"].as_array().into_iter().flatten() {
            for end in ["start", "end", "center"] {
                if e[end]["ref"].as_str() == Some("origin") {
                    let eid = e["id"].as_str().unwrap_or("");
                    let c = serde_json::json!({
                        "type": "coincident",
                        "refs": [format!("{eid}.{end}"), "origin"],
                    });
                    specs.push(constraint_spec(&info, &g, &c, turned)?);
                }
            }
        }
        for c in f["constraints"].as_array().into_iter().flatten() {
            specs.push(constraint_spec(&info, &g, c, turned)?);
        }
        if !specs.is_empty() {
            self.exec(&EditSketch {
                element: self.el,
                feature: id,
                op: SketchOp::AddConstraints(specs),
            })?;
        }
        for d in f["dimensions"].as_array().into_iter().flatten() {
            let g = self.geometry(id);
            let value = self.num(d.get("value"))?;
            let kind = dimension_kind(&info, &g, d, turned, value)?;
            self.exec(&EditSketch {
                element: self.el,
                feature: id,
                op: SketchOp::SetDimension {
                    dimension: Dimension::new(kind, value, 10.0),
                    moves: vec![],
                    radii: vec![],
                },
            })?;
        }
        self.sketches.insert(name.clone(), info);
        self.features.insert(name, id);
        Ok(())
    }

    fn geometry(&self, id: FeatureId) -> Sketch {
        self.d
            .element(self.el)
            .unwrap()
            .feature(id)
            .unwrap()
            .sketch()
            .unwrap()
            .geometry
            .clone()
    }

    /// A profile (`{sketch, loops: [{outer, curves}]}`) as the cadrs regions it covers: each
    /// region whose inside lies within an odd number of the profile's loops.
    fn regions_of(&self, p: &Value) -> R<(FeatureId, Vec<RegionRef>)> {
        let sname = p["sketch"].as_str().unwrap_or("");
        let Some(s) = self.sketches.get(sname) else {
            return unsupported(format!("profile of unknown sketch `{sname}`"));
        };
        let g = self.geometry(s.id);
        let rs = regions(&g);
        // The arrangement's faces the loops bound (as the format means a profile): a region
        // whose outer boundary runs along one outer loop's curves and whose holes along the
        // inner loops'. Failing that, the regions inside an odd number of the loops' outlines.
        let curves_of = |l: &Value| -> R<Vec<CurveId>> {
            l["curves"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|id| curve_ref(s, id))
                .collect()
        };
        let mut outers = Vec::new();
        let mut inners = Vec::new();
        for l in p["loops"].as_array().into_iter().flatten() {
            if l["outer"].as_bool().unwrap_or(true) {
                outers.push(curves_of(l)?);
            } else {
                inners.extend(curves_of(l)?);
            }
        }
        let by_curves: Vec<RegionRef> = rs
            .iter()
            .filter(|r| {
                outers
                    .iter()
                    .any(|o| r.outer_curves.iter().all(|c| o.contains(c)))
                    && r.hole_curves
                        .iter()
                        .all(|h| h.iter().all(|c| inners.contains(c)))
            })
            .map(|r| RegionRef::new(s.id, r))
            .collect();
        if !by_curves.is_empty() {
            return Ok((s.id, by_curves));
        }
        let mut loops = Vec::new();
        for l in p["loops"].as_array().into_iter().flatten() {
            let mut ids: Vec<&str> = l["curves"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            ids.dedup();
            let mut pieces = Vec::new();
            for id in ids {
                match s.ents.get(id).and_then(|e| e.curve) {
                    Some(c) => pieces.push(polyline(&g, c)),
                    None => return unsupported(format!("loop curve `{id}`")),
                }
            }
            loops.push(chain(pieces));
        }
        if loops.is_empty() {
            return unsupported("a profile without loops");
        }
        let picked: Vec<RegionRef> = rs
            .iter()
            .filter(|r| {
                let q = inside_point(r);
                loops.iter().filter(|l| point_in_polygon(q, l)).count() % 2 == 1
            })
            .map(|r| RegionRef::new(s.id, r))
            .collect();
        if picked.is_empty() {
            return failed(format!("no region of `{sname}` inside the profile"));
        }
        Ok((s.id, picked))
    }

    fn profiles(&self, f: &Value) -> R<Vec<(FeatureId, Vec<RegionRef>)>> {
        let list: Vec<Value> = match (f.get("profiles"), f.get("profile")) {
            (Some(Value::Array(a)), _) => a.clone(),
            (None, Some(p)) => vec![p.clone()],
            _ => return unsupported("a feature without profiles"),
        };
        list.iter().map(|p| self.regions_of(p)).collect()
    }

    fn op_of(f: &Value) -> R<BooleanOp> {
        match f["operation"].as_str().unwrap_or("new") {
            "new" | "new_body" => Ok(BooleanOp::New),
            "join" => Ok(BooleanOp::Add),
            "cut" => Ok(BooleanOp::Remove),
            "intersect" => Ok(BooleanOp::Intersect),
            o => unsupported(format!("operation `{o}`")),
        }
    }

    /// Whether the recipe's "positive" (its sketch plane's normal) is against cadrs' normal.
    fn against(&self, sketch: FeatureId) -> bool {
        self.sketches
            .values()
            .find(|s| s.id == sketch)
            .is_some_and(|s| dot(s.recipe.n, s.cadrs.n) < 0.0)
    }

    /// Names the parts a feature made or kept: `result_bodies` in order.
    fn name_bodies(&mut self, f: &Value, id: FeatureId, before: &[PartId]) -> R<()> {
        let parts = self.parts(id)?;
        let names: Vec<String> = f["result_bodies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
        let mut fresh: Vec<PartId> = parts
            .iter()
            .filter(|p| !before.contains(&p.id))
            .map(|p| p.id)
            .collect();
        for n in names {
            if self
                .bodies
                .get(&n)
                .is_some_and(|p| parts.iter().any(|x| x.id == *p))
            {
                continue;
            }
            if !fresh.is_empty() {
                self.bodies.insert(n, fresh.remove(0));
            }
        }
        Ok(())
    }

    fn ids(&self) -> Vec<PartId> {
        let b = cadrs_core::rebuild::build(&self.features_now());
        b.parts.iter().map(|p| p.id).collect()
    }

    fn extrude(&mut self, f: &Value) -> R<FeatureId> {
        let mut e = ExtrudeFeature {
            op: Self::op_of(f)?,
            ..ExtrudeFeature::default()
        };
        let sketch;
        let thin = &f["thin"];
        if !thin.is_null() {
            // A thin wall along a sketch's curves (all of them: cadrs' thin extrude takes whole
            // sketches): `thickness` inside a closed profile, `thickness2` outside.
            let sname = thin["sketch"].as_str().unwrap_or("");
            let Some(info) = self.sketches.get(sname) else {
                return unsupported(format!("thin wall of unknown sketch `{sname}`"));
            };
            let listed = thin["curves"].as_array().map_or(0, Vec::len);
            if listed != self.geometry(info.id).curves.len() {
                return unsupported("a thin wall along some of a sketch's curves");
            }
            sketch = info.id;
            let t1 = self.num(thin.get("thickness"))?;
            let t2 = match thin["side"].as_str().unwrap_or("one_side") {
                "two_sides" => self.num(thin.get("thickness2"))?,
                "one_side" => 0.0,
                other => return unsupported(format!("a thin wall `{other}`")),
            };
            e.sketches = vec![sketch];
            e.body = BodyType::Thin;
            e.thin = ThinWall {
                thickness1: t1,
                thickness1_expr: format!("{t1} mm"),
                thickness2: t2,
                thickness2_expr: format!("{t2} mm"),
                ..ThinWall::default()
            };
        } else {
            let profiles = self.profiles(f)?;
            sketch = profiles[0].0;
            e.regions = profiles.into_iter().flat_map(|(_, r)| r).collect();
            // A cut of a flat pattern sketch, while unfolded: out of the flat, then refolded.
            if self.flat_sketches.contains(&sketch) {
                if e.op != BooleanOp::Remove {
                    return unsupported("a flat pattern extrude other than a cut");
                }
                return self.add(
                    "Extrude",
                    FeatureKind::FlatExtrude(FlatExtrudeFeature {
                        remove: true,
                        regions: e.regions,
                        sketches: Vec::new(),
                    }),
                );
            }
        }
        let x = &f["extent"];
        let negative = x["direction"].as_str() == Some("negative");
        let against = self.against(sketch);
        match x["type"].as_str().unwrap_or("distance") {
            "distance" => {
                e.depth = self.num(x.get("distance").or(f.get("distance")))?;
                e.flip = negative != against;
            }
            "through_all" => {
                e.end = EndType::ThroughAll;
                e.flip = negative != against;
            }
            "symmetric" => {
                let d = self.num(x.get("distance"))?;
                let full = x["is_full_length"].as_bool().unwrap_or(false);
                // cadrs' symmetric depth is the total.
                e.depth = if full { d } else { 2.0 * d };
                e.symmetric = true;
            }
            "two_sides" => {
                e.depth = self.num(x.get("side1"))?;
                let d2 = self.num(x.get("side2_opposite").or(x.get("side2")))?;
                e.second = Some(EndCondition {
                    end: EndType::Blind,
                    depth: d2,
                    depth_expr: format!("{d2} mm"),
                    up_to: None,
                    offset: None,
                });
                e.flip = against;
            }
            t => return unsupported(format!("extent `{t}`")),
        }
        e.depth_expr = format!("{} mm", e.depth);
        if let Some(t) = x.get("taper_deg") {
            let t = self.num(Some(t))?;
            // A positive taper grows outward (SolidCraft); cadrs' draft leans in unless flipped.
            e.draft = Some(ExtrudeDraft {
                angle: t.abs(),
                expr: format!("{} deg", t.abs()),
                flip: t > 0.0,
            });
        }
        let before = self.ids();
        let id = FeatureId::new();
        self.exec(&AddExtrude {
            element: self.el,
            feature: id,
            extrude: ExtrudeFeature::default(),
        })?;
        self.exec(&SetExtrude {
            element: self.el,
            feature: id,
            extrude: e,
            label: "Extrude".into(),
        })?;
        self.name_bodies(f, id, &before)?;
        Ok(id)
    }

    fn revolve(&mut self, f: &Value) -> R<FeatureId> {
        let a = &f["axis"];
        let (o, dir) = (
            a.get("origin").and_then(v3).unwrap_or([0.0; 3]),
            a.get("dir").and_then(v3),
        );
        if dir.is_none_or(|d| norm(sub(unit(d), [0.0, 0.0, 1.0])) > 1e-9) || norm(o) > 1e-9 {
            return unsupported("a revolve axis other than the world Z axis");
        }
        let profiles = self.profiles(f)?;
        let regions = profiles.into_iter().flat_map(|(_, r)| r).collect();
        let angle = self.num(f.get("angle"))?;
        let r = RevolveFeature {
            regions,
            op: Self::op_of(f)?,
            axis: Some(AxisRef::Connector(ConnectorRef::Implicit(
                ConnectorOrigin::Origin,
            ))),
            kind: if (angle - 360.0).abs() < 1e-9 {
                RevolveType::Full
            } else {
                RevolveType::Blind
            },
            angle,
            angle_expr: format!("{angle} deg"),
            ..RevolveFeature::default()
        };
        let before = self.ids();
        let id = FeatureId::new();
        self.exec(&AddRevolve {
            element: self.el,
            feature: id,
            revolve: RevolveFeature::default(),
        })?;
        self.exec(&SetRevolve {
            element: self.el,
            feature: id,
            revolve: r,
            label: "Revolve".into(),
        })?;
        self.name_bodies(f, id, &before)?;
        Ok(id)
    }

    /// The part edge nearest a world point.
    fn edge(&self, parts: &[Part], p: V3) -> R<EdgeRef> {
        let best = parts
            .iter()
            .flat_map(|part| part.solid.edges.iter().map(move |e| (part, e)))
            .map(|(part, e)| (e.distance(p), part, e))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((d, part, e)) if d < 1e-2 => Ok(EdgeRef {
                part: part.id,
                edge: e.name,
                seed: p,
            }),
            _ => failed(format!("no edge at {p:?}")),
        }
    }

    /// A fillet's radius law with fit "taper" (SolidCraft's): a clamped cubic B-spline in u
    /// (0 at the edge's recipe `start`, 1 at its end) on 8 equal spans, its control radii
    /// min(r, 0.9 room) at the Greville abscissae, where the room at a point of the edge is
    /// the narrower of its two faces across the edge (from the edge to the face's far side)
    /// times tan(θ/2), θ the angle between the faces inside the part. One straight edge between
    /// flat faces.
    fn taper_law(&self, entities: &[EdgeOrFace], f: &Value, r: f64) -> R<(Vec<f64>, Vec<f64>)> {
        let [EdgeOrFace::Edge(e)] = entities else {
            return unsupported("a taper fillet of several edges");
        };
        let parts = cadrs_core::rebuild::build(&self.features_now())
            .parts
            .clone();
        let part = parts.iter().find(|p| p.id == e.part).unwrap();
        let (Some(p0), Some(p1)) = (v3(&f["edges"][0]["start"]), v3(&f["edges"][0]["end"])) else {
            return unsupported("a taper fillet's edge without its ends");
        };
        let along = unit(sub(p1, p0));
        let faces: Vec<&cadrs_core::solid::SolidFace> = e
            .edge
            .faces
            .iter()
            .filter_map(|n| part.solid.face(n))
            .collect();
        let planes: Vec<_> = faces.iter().filter_map(|x| x.plane).collect();
        if faces.len() != 2 || planes.len() != 2 {
            return unsupported("a taper fillet between faces that aren't flat");
        }
        // Outward normals, and the angle between the faces inside the part.
        let normal = |pl: &cadrs_sketch::PlaneFrame| unit(cross(pl.u, pl.v));
        let (n1, n2) = (normal(&planes[0]), normal(&planes[1]));
        let theta = std::f64::consts::PI - dot(n1, n2).clamp(-1.0, 1.0).acos();
        // How far across a face from point q on the edge: along the face, square to the edge,
        // into it, to the first crossing of its boundary.
        let across = |face: &cadrs_core::solid::SolidFace, pl: &cadrs_sketch::PlaneFrame, q: V3| {
            let to2 = |p: V3| (dot(sub(p, pl.origin), pl.u), dot(sub(p, pl.origin), pl.v));
            let d3 = cross(normal(pl), along);
            let q2 = to2(q);
            let (dx, dy) = (dot(d3, pl.u), dot(d3, pl.v));
            let mut hits: Vec<f64> = Vec::new();
            for lp in &face.loops {
                for k in 0..lp.len() {
                    let (a, b) = (to2(lp[k]), to2(lp[(k + 1) % lp.len()]));
                    let (ex, ey) = (b.0 - a.0, b.1 - a.1);
                    let den = dx * ey - dy * ex;
                    if den.abs() < 1e-12 {
                        continue;
                    }
                    let (wx, wy) = (a.0 - q2.0, a.1 - q2.1);
                    let t = (wx * ey - wy * ex) / den;
                    let s = (wx * dy - wy * dx) / den;
                    if (-1e-9..=1.0 + 1e-9).contains(&s) && t.abs() > 1e-6 {
                        hits.push(t);
                    }
                }
            }
            // Into the face: the side with the nearer crossing beyond the edge.
            let fwd = hits
                .iter()
                .filter(|t| **t > 0.0)
                .fold(f64::INFINITY, |m, t| m.min(*t));
            let back = hits
                .iter()
                .filter(|t| **t < 0.0)
                .fold(f64::INFINITY, |m, t| m.min(-*t));
            fwd.min(back)
        };
        let room = |u: f64| {
            let q = add(p0, scale(sub(p1, p0), u));
            faces
                .iter()
                .zip(&planes)
                .map(|(x, pl)| across(x, pl, q))
                .fold(f64::INFINITY, f64::min)
                * (theta / 2.0).tan()
        };
        const SPANS: usize = 8;
        let mut knots = vec![0.0; 4];
        knots.extend((1..SPANS).map(|k| k as f64 / SPANS as f64));
        knots.extend([1.0; 4]);
        let n = knots.len() - 4;
        let cps = (0..n)
            .map(|i| {
                let xi = (knots[i + 1] + knots[i + 2] + knots[i + 3]) / 3.0;
                r.min(0.9 * room(xi))
            })
            .collect();
        Ok((cps, knots))
    }

    /// The part vertex at a world point.
    fn vertex(&self, parts: &[Part], p: V3) -> R<VertexRef> {
        parts
            .iter()
            .flat_map(|part| part.solid.vertices.iter().map(move |v| (part, v)))
            .find(|(_, v)| norm(sub(v.point, p)) < 1e-4)
            .map(|(part, v)| VertexRef {
                part: part.id,
                vertex: v.name,
                point: v.point,
            })
            .map_or_else(|| failed(format!("no vertex at {p:?}")), Ok)
    }

    /// The part face nearest a world point.
    fn face(&self, parts: &[Part], p: V3) -> R<FaceRef> {
        let mut best: Option<(f64, &Part, usize)> = None;
        for part in parts {
            let s = &part.solid;
            for (i, f) in s.faces.iter().enumerate() {
                let d = (f.first_triangle..f.first_triangle + f.triangle_count)
                    .map(|t| {
                        let [a, b, c] =
                            [0, 1, 2].map(|k| s.positions[s.indices[3 * t + k] as usize]);
                        point_triangle(p, a, b, c)
                    })
                    .fold(f64::INFINITY, f64::min);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, part, i));
                }
            }
        }
        match best {
            Some((d, part, i)) if d < 0.1 => Ok(FaceRef {
                part: part.id,
                face: part.solid.faces[i].name,
                seed: p,
            }),
            _ => failed(format!("no face at {p:?}")),
        }
    }

    fn edges_of(&self, f: &Value, after: Option<FeatureId>) -> R<Vec<EdgeOrFace>> {
        let parts = match after {
            Some(a) => self.parts(a)?,
            None => cadrs_core::rebuild::build(&self.features_now())
                .parts
                .clone(),
        };
        f["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let p = e
                    .get("point")
                    .or(e.get("mid"))
                    .and_then(v3)
                    .unwrap_or([f64::NAN; 3]);
                self.edge(&parts, p).map(EdgeOrFace::Edge)
            })
            .collect()
    }

    fn faces_of(&self, list: &Value) -> R<Vec<FaceRef>> {
        let parts = cadrs_core::rebuild::build(&self.features_now())
            .parts
            .clone();
        list.as_array()
            .into_iter()
            .flatten()
            .map(|x| {
                let p = x.get("point").and_then(v3).unwrap_or([f64::NAN; 3]);
                self.face(&parts, p)
            })
            .collect()
    }

    /// Features named by a recipe (`"Hole1"`, `"Hole1 (1)"`).
    fn feature_list(&self, v: &Value) -> R<Vec<FeatureId>> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|s| {
                let t = s.trim();
                let base = match t.rfind(" (") {
                    Some(i) if t.ends_with(')') => &t[..i],
                    _ => t,
                };
                self.features
                    .get(base)
                    .copied()
                    .map_or_else(|| unsupported(format!("feature `{s}`")), Ok)
            })
            .collect()
    }

    fn body(&self, v: &Value) -> R<PartId> {
        let n = v.as_str().unwrap_or("");
        self.bodies
            .get(n)
            .copied()
            .map_or_else(|| unsupported(format!("body `{n}`")), Ok)
    }

    /// A direction along a world axis, as a default plane's normal.
    fn axis_direction(d: V3) -> R<(DirectionRef, bool)> {
        let d = unit(d);
        for (pr, x, y) in STD {
            let n = cross(x, y);
            let c = dot(n, d);
            if c.abs() > 1.0 - 1e-9 {
                return Ok((DirectionRef::PlaneNormal(pr), c < 0.0));
            }
        }
        unsupported("a pattern direction off the world axes")
    }

    /// A recipe plane given by reference (`XY`, a construction plane's name) or frame.
    fn plane_ref(&mut self, v: &Value) -> R<PlaneRef> {
        if let Some(s) = v.as_str().or(v.get("ref").and_then(Value::as_str)) {
            if let Some((p, _)) = self.planes.get(s) {
                return Ok(*p);
            }
            match s.to_ascii_uppercase().as_str() {
                "XY" => return Ok(PlaneRef::Top),
                "XZ" => return Ok(PlaneRef::Front),
                "YZ" => return Ok(PlaneRef::Right),
                _ => {}
            }
        }
        match (v.get("origin").and_then(v3), v.get("normal").and_then(v3)) {
            (Some(o), Some(n)) => Ok(self.cadrs_plane(o, n)?.0),
            _ => unsupported(format!("plane {v}")),
        }
    }

    fn feature(&mut self, f: &Value) -> R<()> {
        let name = f["name"].as_str().unwrap_or("").to_string();
        let op = f["op"].as_str().unwrap_or("");
        let id = match op {
            "sketch" => return self.sketch(f),
            "extrude" => self.extrude(f)?,
            "revolve" => self.revolve(f)?,
            "construction_plane" if f["method"].as_str() == Some("at_angle") => {
                let plane = self.angled_plane(f)?;
                self.planes.insert(name, plane);
                return Ok(());
            }
            "construction_plane" => {
                if f["method"].as_str() != Some("offset") {
                    return unsupported(format!("construction plane `{}`", f["method"]));
                }
                let base = self.recipe_frame(&serde_json::json!({"ref": f["base"]}))?;
                let off = self.num(f.get("offset"))?;
                let o = add(base.o, scale(unit(base.n), off));
                let plane = self.cadrs_plane(o, base.n)?;
                self.planes.insert(name, plane);
                return Ok(());
            }
            "fillet" => {
                let entities = self.edges_of(f, None)?;
                let r = self.num(f.get("radius"))?;
                let mut x = FilletFeature {
                    entities: entities.clone(),
                    size: r,
                    size_expr: format!("{r} mm"),
                    tangent_propagation: f["tangent_chain"].as_bool().unwrap_or(false),
                    ..FilletFeature::default()
                };
                let kind = match f["type"].as_str().unwrap_or("constant_radius") {
                    "constant_radius" if f["fit"].as_str() == Some("taper") => "taper",
                    other => other,
                };
                match kind {
                    "constant_radius" => {}
                    // The round's width across, kept along the edge.
                    "chord_length" => x.measurement = FilletMeasurement::Width,
                    // The radius along the edge from its `start`: straight from `radius` to
                    // `radius2`, or a clamped B-spline of `radius` then `radii` on `knots`.
                    // Fit "taper": the radius where it fits, else what fits (see `taper_law`).
                    kind @ ("variable_radius" | "spline_radius" | "taper") => {
                        let law: Box<dyn Fn(f64) -> f64> = if kind == "taper" {
                            let (cps, knots) = self.taper_law(&entities, f, r)?;
                            Box::new(move |u| bspline(&cps, &knots, 3, u))
                        } else if kind == "variable_radius" {
                            let r2 = self.num(f.get("radius2"))?;
                            Box::new(move |u| r + (r2 - r) * u)
                        } else {
                            let mut cps = vec![r];
                            for v in f["radii"].as_array().into_iter().flatten() {
                                cps.push(self.num(Some(v))?);
                            }
                            let knots: Vec<f64> = f["knots"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_f64)
                                .collect();
                            if knots.len() != cps.len() + 4 {
                                return unsupported("a radius spline other than a cubic one");
                            }
                            Box::new(move |u| bspline(&cps, &knots, 3, u))
                        };
                        let [EdgeOrFace::Edge(e)] = entities.as_slice() else {
                            return unsupported("a variable fillet of several edges");
                        };
                        let parts = cadrs_core::rebuild::build(&self.features_now())
                            .parts
                            .clone();
                        let part = parts.iter().find(|p| p.id == e.part).unwrap();
                        let edge = part.solid.edges.iter().find(|x| x.name == e.edge).unwrap();
                        let (a, b) = (edge.points[0], *edge.points.last().unwrap());
                        // The edge's start as cadrs has it, at the recipe's start or not.
                        let start = v3(&f["start"])
                            .or_else(|| v3(&f["edges"][0]["start"]))
                            .unwrap_or(a);
                        let forward = norm(sub(a, start)) <= norm(sub(b, start));
                        let u = |t: f64| if forward { t } else { 1.0 - t };
                        x.variable = true;
                        x.smooth_transition = false;
                        for (t, at) in [(0.0, a), (1.0, b)] {
                            let radius = law(u(t));
                            x.vertices.push(VertexRadius {
                                vertex: self.vertex(&parts, at)?,
                                radius,
                                expr: format!("{radius} mm"),
                            });
                        }
                        if kind != "variable_radius" {
                            const N: usize = 48;
                            for k in 1..N {
                                let t = k as f64 / N as f64;
                                let radius = law(u(t));
                                x.edge_points.push(EdgePoint {
                                    edge: *e,
                                    location: t,
                                    radius,
                                    expr: format!("{radius} mm"),
                                });
                            }
                        }
                    }
                    other => return unsupported(format!("fillet `{other}`")),
                }
                self.add("Fillet", FeatureKind::Fillet(x))?
            }
            "chamfer" => {
                if f["type"].as_str() != Some("equal_distance") {
                    return unsupported(format!("chamfer `{}`", f["type"]));
                }
                let entities = self.edges_of(f, None)?;
                let d = self.num(f.get("distance"))?;
                self.add(
                    "Chamfer",
                    FeatureKind::Chamfer(ChamferFeature {
                        entities,
                        distance: d,
                        distance_expr: format!("{d} mm"),
                        tangent_propagation: f["tangent_chain"].as_bool().unwrap_or(false),
                        ..ChamferFeature::default()
                    }),
                )?
            }
            "shell" => {
                if f["outside_thickness"].as_f64().unwrap_or(0.0) != 0.0 {
                    return unsupported("an outside shell thickness");
                }
                let faces = self.faces_of(&f["faces_removed"])?;
                let t = self.num(f.get("inside_thickness"))?;
                self.add(
                    "Shell",
                    FeatureKind::Shell(ShellFeature {
                        faces,
                        thickness: t,
                        thickness_expr: format!("{t} mm"),
                        ..ShellFeature::default()
                    }),
                )?
            }
            "combine" => {
                let kind = match f["operation"].as_str().unwrap_or("") {
                    "join" => BooleanKind::Union,
                    "cut" => BooleanKind::Subtract,
                    "intersect" => BooleanKind::Intersect,
                    o => return unsupported(format!("combine `{o}`")),
                };
                let target = self.body(
                    f.get("target_body")
                        .or(f.get("target"))
                        .unwrap_or(&Value::Null),
                )?;
                let tools: Vec<PartId> = f
                    .get("tool_bodies")
                    .or(f.get("tools"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|t| self.body(t))
                    .collect::<R<_>>()?;
                // Union and Intersect take all their parts as tools in cadrs.
                let (targets, tools) = match kind {
                    BooleanKind::Union | BooleanKind::Intersect => {
                        (vec![], [vec![target], tools].concat())
                    }
                    _ => (vec![target], tools),
                };
                self.add(
                    "Boolean",
                    FeatureKind::Boolean(BooleanFeature {
                        op: kind,
                        tools,
                        targets,
                        keep_tools: f["keep_tools"].as_bool().unwrap_or(false),
                        offset: None,
                    }),
                )?
            }
            "rectangular_pattern" | "circular_pattern" => {
                if f["pattern_of"].as_str().unwrap_or("features") != "features" {
                    return unsupported(format!("{op} of {}", f["pattern_of"]));
                }
                let feats = self.feature_list(&f["features"])?;
                let mut p;
                if op == "rectangular_pattern" {
                    p = PatternFeature::new(PatternKind::Linear);
                    let d1 = &f["direction1"];
                    let (dir, flip) = Self::axis_direction(
                        d1["axis"]
                            .as_array()
                            .and_then(|_| v3(&d1["axis"]))
                            .unwrap_or([f64::NAN; 3]),
                    )?;
                    p.first.direction = Some(dir);
                    p.first.flip = flip;
                    p.first.count = d1["count"].as_u64().unwrap_or(1) as u32;
                    p.first.distance = self.num(d1.get("spacing"))?;
                    let d2 = &f["direction2"];
                    if !d2.is_null() {
                        let (dir, flip) =
                            Self::axis_direction(v3(&d2["axis"]).unwrap_or([f64::NAN; 3]))?;
                        p.second_on = true;
                        p.second.direction = Some(dir);
                        p.second.flip = flip;
                        p.second.count = d2["count"].as_u64().unwrap_or(1) as u32;
                        p.second.distance = self.num(d2.get("spacing"))?;
                    }
                } else {
                    let a = &f["axis"];
                    if a.get("dir")
                        .and_then(v3)
                        .is_none_or(|d| norm(sub(unit(d), [0.0, 0.0, 1.0])) > 1e-9)
                        || a.get("origin").and_then(v3).is_some_and(|o| norm(o) > 1e-9)
                    {
                        return unsupported("a circular pattern axis other than the world Z axis");
                    }
                    p = PatternFeature::new(PatternKind::Circular);
                    p.axis = Some(AxisRef::Connector(ConnectorRef::Implicit(
                        ConnectorOrigin::Origin,
                    )));
                    p.first.count = f["count"].as_u64().unwrap_or(1) as u32;
                    let total = self.num(f.get("total_angle")).unwrap_or(360.0);
                    p.angle = total;
                    p.angle_expr = format!("{total} deg");
                    p.equal_spacing = true;
                }
                p.pattern_type = PatternType::Feature;
                p.features = feats;
                self.add("Pattern", FeatureKind::Pattern(p))?
            }
            // A cosmetic thread is the hole's callout, no geometry (in cadrs a tapped Hole's).
            "thread" if f["modeled"].as_bool() == Some(false) => {
                self.notes
                    .push("cosmetic thread: no geometry, not built".into());
                return Ok(());
            }
            // A surface through a sketch's curves: a Fill.
            "boundary_surface" => {
                let sname = f["sketch"].as_str().unwrap_or("");
                let Some(info) = self.sketches.get(sname) else {
                    return unsupported(format!("boundary of unknown sketch `{sname}`"));
                };
                let sketch = info.id;
                let edges = f["curves"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|c| {
                        Ok(FillEdge::SketchCurve {
                            sketch,
                            curve: curve_ref(info, c)?,
                        })
                    })
                    .collect::<R<Vec<_>>>()?;
                let before = self.ids();
                let id = self.add(
                    "Fill",
                    FeatureKind::Fill(FillFeature {
                        edges,
                        ..FillFeature::default()
                    }),
                )?;
                self.name_bodies(f, id, &before)?;
                id
            }
            // Surfaces made solid; they stay (a recipe removes them itself).
            "thicken" => {
                let parts = f["bodies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|b| self.body(b))
                    .collect::<R<Vec<_>>>()?;
                let t = self.num(f.get("thickness"))?;
                let before = self.ids();
                let id = self.add(
                    "Thicken",
                    FeatureKind::Thicken(ThickenFeature {
                        parts,
                        mid_plane: f["symmetric"].as_bool().unwrap_or(false),
                        thickness1: t,
                        thickness1_expr: format!("{t} mm"),
                        op: Self::op_of(f)?,
                        keep_tools: true,
                        ..ThickenFeature::default()
                    }),
                )?;
                self.name_bodies(f, id, &before)?;
                id
            }
            "remove" => {
                let parts = f["bodies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|b| self.body(b))
                    .collect::<R<Vec<_>>>()?;
                self.add(
                    "Delete part",
                    FeatureKind::DeletePart(DeletePartFeature { parts }),
                )?
            }
            "sm_flange" => match f["type"].as_str().unwrap_or("") {
                // The sheet metal model: a sketch region thickened, or open curves extruded.
                kind @ ("base" | "contour") => {
                    if self.sheet.is_some() {
                        return unsupported("a second sheet metal model");
                    }
                    let rule = Rule::named(f["rule"].as_str().unwrap_or(""))?;
                    let params = rule.params();
                    let mut m = SheetMetalModelFeature {
                        params,
                        exprs: SheetMetalExprs::of(&params),
                        ..SheetMetalModelFeature::default()
                    };
                    if kind == "base" {
                        let profiles = self.profiles(f)?;
                        m.operation = SheetMetalOp::Thicken;
                        m.regions = profiles.into_iter().flat_map(|(_, r)| r).collect();
                    } else {
                        let sname = f["profile_sketch"].as_str().unwrap_or("");
                        let Some(info) = self.sketches.get(sname) else {
                            return unsupported(format!("contour of unknown sketch `{sname}`"));
                        };
                        let sketch = info.id;
                        m.operation = SheetMetalOp::Extrude;
                        m.curves = f["profile_curves"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(|c| {
                                Ok(SmCurveRef {
                                    sketch,
                                    curve: curve_ref(info, c)?,
                                })
                            })
                            .collect::<R<Vec<_>>>()?;
                        let d = self.num(f.get("distance"))?;
                        m.depth = d;
                        m.depth_expr = format!("{d} mm");
                        // The width runs along the recipe sketch's normal.
                        m.flip_extrude = self.against(sketch);
                    }
                    let before = self.ids();
                    let id = self.add("Sheet metal model", FeatureKind::SheetMetalModel(m))?;
                    self.sheet = Some((id, rule));
                    self.name_bodies(f, id, &before)?;
                    id
                }
                "edge" => {
                    let edges = self.edges_of(f, None)?;
                    let h = self.num(f.get("height"))?;
                    let a = self.num(f.get("angle"))?;
                    if f["height_datum"]
                        .as_str()
                        .is_some_and(|d| !d.starts_with("Outer"))
                        || f["bend_position"]
                            .as_str()
                            .is_some_and(|b| !b.starts_with("Inside"))
                    {
                        return unsupported("a flange other than outer-face height, bend inside");
                    }
                    self.add(
                        "Flange",
                        FeatureKind::SheetMetal(SheetMetalFeature::Flange(FlangeFeature {
                            edges,
                            // The flange's outside face on the edge; its height from the outer
                            // virtual sharp.
                            alignment: FlangeAlignment::Outer,
                            distance: h,
                            distance_expr: format!("{h} mm"),
                            angle: a,
                            angle_expr: format!("{a} deg"),
                            flip: f["flip"].as_bool().unwrap_or(false),
                            // Flanges meeting at a corner end square, their insides the
                            // thickness apart (SolidCraft's corners): a 45° miter with that gap.
                            auto_miter: false,
                            corner_gap: Some(self.sheet.map_or(0.0, |(_, r)| r.thickness)),
                            ..FlangeFeature::default()
                        })),
                    )?
                }
                other => return unsupported(format!("sm_flange `{other}`")),
            },
            "sm_hem" => {
                if f["type"].as_str() != Some("flat") {
                    return unsupported(format!("hem `{}`", f["type"]));
                }
                let edges = self.edges_of(&serde_json::json!({ "edges": [f["edge"]] }), None)?;
                let l = self.num(f.get("length"))?;
                self.add(
                    "Hem",
                    FeatureKind::SheetMetal(SheetMetalFeature::Hem(HemFeature {
                        edges,
                        kind: HemKind::Straight,
                        // Flat, the rule's minimal gap inside.
                        flattened: true,
                        total: l,
                        total_expr: format!("{l} mm"),
                        // Its bend starts at the edge ("StartEdge").
                        alignment: if f["bend_position"].as_str() == Some("StartEdge") {
                            HemAlignment::InPlace
                        } else {
                            HemAlignment::Outer
                        },
                        // Unflipped, the recipe folds the hem inward, over the part (cadrs'
                        // flip arrow).
                        flip: !f["flipped"].as_bool().unwrap_or(false),
                        ..HemFeature::default()
                    })),
                )?
            }
            "sm_unfold" => {
                let Some((model, _)) = self.sheet else {
                    return unsupported("an unfold with no sheet metal model");
                };
                if f["all_bends"].as_bool() != Some(true) {
                    return unsupported("unfolding some of the bends");
                }
                // The flat pattern laid out from the stationary face's wall: cadrs lays its
                // flat out from the model's anchor wall; check it is the same wall.
                let plane = FeatureId(cadrs_core::sheetmetal_flat::flat_plane_id(model, 0));
                let b = cadrs_core::rebuild::build(&self.features_now());
                let Some(fr) = b.planes.get(&plane).copied() else {
                    return failed("no flat pattern plane");
                };
                let n = unit(cross(fr.u, fr.v));
                let stationary = f["stationary_face"]["point"]
                    .as_array()
                    .and_then(|_| v3(&f["stationary_face"]["point"]));
                if let Some(q) = stationary
                    && dot(sub(q, fr.origin), n).abs()
                        > b.sheet_metal[0].model.params.thickness + 1e-6
                {
                    return unsupported("a flat pattern laid out from another wall");
                }
                self.flat = Some((
                    PlaneRef::Feature(FeaturePlane::new(plane.0, fr)),
                    Frame {
                        o: fr.origin,
                        x: fr.u,
                        y: fr.v,
                        n,
                    },
                ));
                return Ok(());
            }
            "sm_refold" => {
                self.flat = None;
                return Ok(());
            }
            "sheet_metal_rule_edit" => {
                let Some((id, mut rule)) = self.sheet else {
                    return unsupported("a rule edit with no sheet metal model");
                };
                let mm = |v: &str| v.trim().trim_end_matches("mm").trim().parse::<f64>().ok();
                let mut set = |k: &str, v: &str| -> R<()> {
                    let x = mm(v).map_or_else(|| unsupported(format!("rule value `{v}`")), Ok)?;
                    match k {
                        "thickness" => rule.thickness = x,
                        "bendRadius" => rule.bend_radius = Some(x),
                        "kFactor" => rule.k_factor = x,
                        other => return unsupported(format!("rule setting `{other}`")),
                    }
                    Ok(())
                };
                if let Some(m) = f["set"].as_object() {
                    for (k, v) in m {
                        set(k, v.as_str().unwrap_or(""))?;
                    }
                }
                // Edits one after another: the last state stands.
                for step in f["sequence"].as_array().into_iter().flatten() {
                    let step = step.as_str().unwrap_or("");
                    let (k, v) = step.split_once(' ').unwrap_or((step, ""));
                    set(k, v)?;
                }
                let mut feature = self
                    .features_now()
                    .into_iter()
                    .find(|x| x.id == id)
                    .unwrap();
                if let FeatureKind::SheetMetalModel(m) = &mut feature.kind {
                    m.params = rule.params();
                    m.exprs = SheetMetalExprs::of(&m.params);
                }
                self.exec(&ReplaceFeature {
                    element: self.el,
                    feature,
                    label: "Edit rule".into(),
                })?;
                self.sheet = Some((id, rule));
                id
            }
            "path_pattern" => {
                if f["pattern_of"].as_str().unwrap_or("features") != "features" {
                    return unsupported(format!("path_pattern of {}", f["pattern_of"]));
                }
                let sname = f["path"]["sketch"].as_str().unwrap_or("");
                let Some(info) = self.sketches.get(sname) else {
                    return unsupported(format!("path of unknown sketch `{sname}`"));
                };
                let sketch = info.id;
                let path = f["path"]["curves"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|c| {
                        Ok(PathRef::SketchCurve {
                            sketch,
                            curve: curve_ref(info, c)?,
                        })
                    })
                    .collect::<R<Vec<_>>>()?;
                let mut p = PatternFeature::new(PatternKind::Curve);
                p.pattern_type = PatternType::Feature;
                p.features = self.feature_list(&f["features"])?;
                p.path = path;
                p.first.count = f["count"].as_u64().unwrap_or(1) as u32;
                // The instances `spacing` apart along the path from the seed (at its start), or
                // spread evenly over it.
                match f.get("spacing") {
                    Some(d) => {
                        let d = self.num(Some(d))?;
                        p.equal_spacing = false;
                        p.first.distance = d;
                        p.first.distance_expr = format!("{d} mm");
                    }
                    None => p.equal_spacing = true,
                }
                p.tangent_to_curve = !f["orientation"]
                    .as_str()
                    .is_some_and(|o| o.starts_with("identical"));
                self.add("Pattern", FeatureKind::Pattern(p))?
            }
            "mirror" => {
                let plane = MirrorPlane::Plane(self.plane_ref(&f["plane"])?);
                let mut m = MirrorFeature {
                    plane: Some(plane),
                    ..MirrorFeature::default()
                };
                if f["mirror_of"].as_str() == Some("bodies") {
                    m.mirror_type = PatternType::Part;
                    m.parts = f["bodies"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|b| self.body(b))
                        .collect::<R<_>>()?;
                    m.op = if f["combine"].as_bool().unwrap_or(false) {
                        BooleanOp::Add
                    } else {
                        BooleanOp::New
                    };
                } else {
                    m.mirror_type = PatternType::Feature;
                    m.features = self.feature_list(&f["features"])?;
                }
                self.add("Mirror", FeatureKind::Mirror(m))?
            }
            "loft" => {
                if f["rails"].as_array().is_some_and(|r| !r.is_empty()) {
                    return unsupported("loft rails");
                }
                let mut profiles = Vec::new();
                for s in f["sections"].as_array().into_iter().flatten() {
                    if let Some(p) = s.get("point").and_then(Value::as_str) {
                        // A sketch point (first or last section).
                        let sname = s["sketch"].as_str().unwrap_or("");
                        let Some(info) = self.sketches.get(sname) else {
                            return unsupported(format!("loft point in `{sname}`"));
                        };
                        let Some(point) = info.ents.get(p).and_then(|e| e.point) else {
                            return unsupported(format!("loft point `{p}`"));
                        };
                        profiles.push(LoftProfile::SketchPoint {
                            sketch: info.id,
                            point,
                        });
                        continue;
                    }
                    let (sketch, regions) = self.regions_of(s)?;
                    profiles.push(LoftProfile::Regions { sketch, regions });
                }
                let before = self.ids();
                let id = self.add(
                    "Loft",
                    FeatureKind::Loft(LoftFeature {
                        profiles,
                        op: Self::op_of(f)?,
                        ..LoftFeature::default()
                    }),
                )?;
                self.name_bodies(f, id, &before)?;
                id
            }
            "sweep"
                if f["path"]["sketch"]
                    .as_str()
                    .is_some_and(|s| self.helices.contains_key(s)) =>
            {
                let helix = self.helices[f["path"]["sketch"].as_str().unwrap()];
                let profiles = self.profiles(f)?;
                let regions = profiles.into_iter().flat_map(|(_, r)| r).collect();
                let before = self.ids();
                let id = self.add(
                    "Sweep",
                    FeatureKind::Sweep(SweepFeature {
                        regions,
                        path: vec![PathRef::Curve(helix)],
                        op: Self::op_of(f)?,
                        ..SweepFeature::default()
                    }),
                )?;
                self.name_bodies(f, id, &before)?;
                id
            }
            "sweep" => {
                let path = &f["path"];
                let sname = path["sketch"].as_str().unwrap_or("");
                let Some(ps) = self.sketches.get(sname) else {
                    return unsupported(format!("sweep path `{sname}`"));
                };
                let wanted: Vec<&str> = path["curves"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                if wanted.len() != ps.ents.values().filter(|e| e.curve.is_some()).count() {
                    return unsupported("a sweep along some of a sketch's curves");
                }
                let path_id = ps.id;
                let profiles = self.profiles(f)?;
                let regions = profiles.into_iter().flat_map(|(_, r)| r).collect();
                let before = self.ids();
                let id = self.add(
                    "Sweep",
                    FeatureKind::Sweep(SweepFeature {
                        regions,
                        path: vec![PathRef::Sketch(path_id)],
                        op: Self::op_of(f)?,
                        ..SweepFeature::default()
                    }),
                )?;
                self.name_bodies(f, id, &before)?;
                id
            }
            "hole" => self.hole(f)?,
            "draft" => {
                let faces = self.faces_of(&f["faces"])?;
                let np = &f["neutral_plane"];
                let parts = cadrs_core::rebuild::build(&self.features_now())
                    .parts
                    .clone();
                let neutral = match np.get("point").and_then(v3) {
                    Some(p) => MirrorPlane::Face(self.face(&parts, p)?),
                    None => return unsupported("a draft's neutral plane"),
                };
                let a = self.num(f.get("angle"))?;
                self.add(
                    "Draft",
                    FeatureKind::Draft(DraftFeature {
                        neutral: Some(neutral),
                        faces,
                        angle: a,
                        angle_expr: format!("{a} deg"),
                        flip: f["flipped"].as_bool().unwrap_or(false),
                        ..DraftFeature::default()
                    }),
                )?
            }
            "split_body" => {
                let body = self.body(&f["body"])?;
                let tool = SplitToolRef::Plane(self.plane_ref(&f["tool"])?);
                let before = self.ids();
                let id = self.add(
                    "Split",
                    FeatureKind::Split(SplitFeature {
                        parts: vec![body],
                        tool: Some(tool),
                        ..SplitFeature::default()
                    }),
                )?;
                self.name_bodies(f, id, &before)?;
                id
            }
            other => return unsupported(format!("`{other}`")),
        };
        // Report a feature that failed here, not at the end.
        self.parts(id)?;
        if !name.is_empty() {
            self.features.insert(name, id);
        }
        Ok(())
    }

    /// A hole at a world position on a planar face, drilled into the material: a sketch point on
    /// the face's plane and a Hole feature.
    fn hole(&mut self, f: &Value) -> R<FeatureId> {
        // At a sketch's points ("sketch HolePoints points p1..p6", as SolidCraft reads it).
        if let Some(src) = f["positions_from"].as_str() {
            let w: Vec<&str> = src.split_whitespace().collect();
            let after = |k: &str| w.iter().position(|x| *x == k).and_then(|i| w.get(i + 1));
            let (Some(sk), Some(range)) = (after("sketch"), after("points")) else {
                return unsupported(format!("hole positions `{src}`"));
            };
            let ids: Vec<String> = match range.split_once("..") {
                Some((a, b)) => {
                    let pre: String = a.chars().take_while(|c| !c.is_ascii_digit()).collect();
                    let lo = a[pre.len()..].parse::<u32>().unwrap_or(1);
                    let hi = b
                        .trim_start_matches(pre.as_str())
                        .parse::<u32>()
                        .unwrap_or(0);
                    (lo..=hi).map(|k| format!("{pre}{k}")).collect()
                }
                None => range.split(',').map(str::to_string).collect(),
            };
            let Some(info) = self.sketches.get(*sk) else {
                return unsupported(format!("hole sketch `{sk}`"));
            };
            // cadrs drills against its sketch plane's normal; the recipe into the material.
            if f["direction"].as_str().is_some_and(|d| d.contains("-Z")) != (info.cadrs.n[2] > 0.5)
            {
                return unsupported("a hole drilled along its sketch plane's normal");
            }
            let points: Vec<HolePoint> = ids
                .iter()
                .map(|id| {
                    info.ents
                        .get(id)
                        .and_then(|e| e.point)
                        .map(|point| HolePoint {
                            sketch: info.id,
                            point,
                        })
                        .map_or_else(|| unsupported(format!("hole point `{id}`")), Ok)
                })
                .collect::<R<_>>()?;
            let spec = self.hole_spec(f)?;
            return self.add(
                "Hole",
                FeatureKind::Hole(HoleFeature {
                    points,
                    spec,
                    ..HoleFeature::default()
                }),
            );
        }
        let face = &f["face"];
        let Some(pos) = f.get("position").and_then(v3) else {
            return unsupported("a hole without a position");
        };
        // The face's outward normal at the hole: a plane's, or a cylinder's radial direction
        // there (the hole's sketch then lies on the plane touching the cylinder there).
        let n = match face["type"].as_str().unwrap_or("plane") {
            "plane" => face.get("plane_normal_outward").and_then(v3),
            "cylinder" => match (
                face.get("axis_origin").and_then(v3),
                face.get("axis").and_then(v3),
            ) {
                (Some(o), Some(a)) => {
                    let (a, d) = (unit(a), sub(pos, o));
                    Some(sub(d, scale(a, dot(d, a))))
                }
                _ => None,
            },
            other => return unsupported(format!("a hole on a {other} face")),
        };
        let Some(n) = n else {
            return unsupported("a hole without its face's normal");
        };
        let (pr, cf) = self.cadrs_plane(pos, n)?;
        // cadrs drills against its sketch plane's normal, or along it flipped: into the material.
        let flip = dot(cf.n, unit(n)) < 0.0;
        let s = FeatureId::new();
        self.exec(&AddSketch {
            element: self.el,
            feature: s,
            plane: Some(pr),
        })?;
        let d = sub(pos, cf.o);
        let at = Vec2::new(dot(d, cf.x), dot(d, cf.y));
        self.exec(&EditSketch {
            element: self.el,
            feature: s,
            op: SketchOp::AddPoint { pos: at },
        })?;
        let g = self.geometry(s);
        let point = g
            .points
            .iter()
            .min_by(|a, b| {
                let da = (a.1.pos.x - at.x).powi(2) + (a.1.pos.y - at.y).powi(2);
                let db = (b.1.pos.x - at.x).powi(2) + (b.1.pos.y - at.y).powi(2);
                da.total_cmp(&db)
            })
            .map(|(id, _)| id)
            .unwrap();
        let spec = self.hole_spec(f)?;
        self.add(
            "Hole",
            FeatureKind::Hole(HoleFeature {
                points: vec![HolePoint { sketch: s, point }],
                spec,
                flip,
                ..HoleFeature::default()
            }),
        )
    }

    fn hole_spec(&self, f: &Value) -> R<HoleSpec> {
        let mut spec = HoleSpec::default();
        spec.diameter = Length::mm(self.num(f.get("diameter"))?);
        match f["hole_type"].as_str().unwrap_or("simple") {
            "simple" => spec.style = HoleStyle::Simple,
            "counterbore" => {
                spec.style = HoleStyle::Counterbore;
                spec.cbore_diameter = Length::mm(self.num(f.get("counterbore_diameter"))?);
                spec.cbore_depth = Length::mm(self.num(f.get("counterbore_depth"))?);
            }
            "countersink" => {
                spec.style = HoleStyle::Countersink;
                spec.csink_diameter = Length::mm(self.num(f.get("countersink_diameter"))?);
                spec.csink_angle = Length::deg(self.num(f.get("countersink_angle"))?);
            }
            t => return unsupported(format!("hole `{t}`")),
        }
        if f["extent"].as_str() == Some("through_all") {
            spec.end = HoleEnd::ThroughAll;
        } else {
            spec.end = HoleEnd::Blind;
            spec.depth = Length::mm(self.num(f.get("depth"))?);
            if let Some(t) = f.get("tip_angle") {
                spec.tip_angle = Length::deg(self.num(Some(t))?);
            }
        }
        Ok(spec)
    }
}

/// A recipe expression: numbers, parameters, + − × ÷ and brackets, with an optional unit.
fn eval(s: &str, params: &HashMap<String, f64>) -> Option<f64> {
    let t = s.trim();
    let t = t
        .strip_suffix("mm")
        .or_else(|| t.strip_suffix("deg"))
        .unwrap_or(t)
        .trim();
    let toks: Vec<String> = {
        let mut out = Vec::new();
        let mut cur = String::new();
        for ch in t.chars() {
            if "+-*/()".contains(ch) {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
                out.push(ch.to_string());
            } else if ch.is_whitespace() {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            } else {
                cur.push(ch);
            }
        }
        if !cur.trim().is_empty() {
            out.push(cur.trim().to_string());
        }
        out
    };
    fn expr(t: &[String], i: &mut usize, p: &HashMap<String, f64>) -> Option<f64> {
        let mut v = term(t, i, p)?;
        while *i < t.len() && (t[*i] == "+" || t[*i] == "-") {
            let op = t[*i].clone();
            *i += 1;
            let r = term(t, i, p)?;
            v = if op == "+" { v + r } else { v - r };
        }
        Some(v)
    }
    fn term(t: &[String], i: &mut usize, p: &HashMap<String, f64>) -> Option<f64> {
        let mut v = atom(t, i, p)?;
        while *i < t.len() && (t[*i] == "*" || t[*i] == "/") {
            let op = t[*i].clone();
            *i += 1;
            let r = atom(t, i, p)?;
            v = if op == "*" { v * r } else { v / r };
        }
        Some(v)
    }
    fn atom(t: &[String], i: &mut usize, p: &HashMap<String, f64>) -> Option<f64> {
        let tok = t.get(*i)?.clone();
        *i += 1;
        match tok.as_str() {
            "(" => {
                let v = expr(t, i, p)?;
                (t.get(*i)? == ")").then(|| *i += 1)?;
                Some(v)
            }
            "-" => Some(-atom(t, i, p)?),
            s => s
                .trim_end_matches("mm")
                .parse::<f64>()
                .ok()
                .or_else(|| p.get(s).copied()),
        }
    }
    let mut i = 0;
    let v = expr(&toks, &mut i, params)?;
    (i == toks.len()).then_some(v)
}

/// A curve as a polyline in its sketch's coordinates (arcs counter-clockwise).
fn polyline(g: &Sketch, c: CurveId) -> Vec<Vec2> {
    let p = |id: PointId| g.points[id].pos;
    match g.curves[c].kind {
        CurveKind::Line { a, b } => vec![p(a), p(b)],
        CurveKind::Circle { center, radius } => {
            let c = p(center);
            (0..=96)
                .map(|i| {
                    let t = i as f64 / 96.0 * std::f64::consts::TAU;
                    Vec2::new(c.x + radius * t.cos(), c.y + radius * t.sin())
                })
                .collect()
        }
        CurveKind::Arc { center, start, end } => {
            let (c, s, e) = (p(center), p(start), p(end));
            let r = ((s.x - c.x).powi(2) + (s.y - c.y).powi(2)).sqrt();
            let a0 = (s.y - c.y).atan2(s.x - c.x);
            let sw = ((e.y - c.y).atan2(e.x - c.x) - a0).rem_euclid(std::f64::consts::TAU);
            (0..=64)
                .map(|i| {
                    let t = a0 + sw * i as f64 / 64.0;
                    Vec2::new(c.x + r * t.cos(), c.y + r * t.sin())
                })
                .collect()
        }
        _ => vec![],
    }
}

/// A recipe point reference (`origin`, `l1.start`, `c1.center`, a point entity) as a cadrs point.
fn point_ref(info: &SketchInfo, g: &Sketch, r: &str) -> R<PointRef> {
    if r == "origin" {
        return Ok(PointRef::Origin);
    }
    let (ent, part) = r.split_once('.').unwrap_or((r, ""));
    let Some(e) = info.ents.get(ent) else {
        return unsupported(format!("point `{r}`"));
    };
    if let Some(p) = e.point {
        return Ok(PointRef::Point(p));
    }
    let c = e.curve.unwrap();
    let id = match (g.curves[c].kind, part) {
        (CurveKind::Line { a, .. }, "start") => a,
        (CurveKind::Line { b, .. }, "end") => b,
        (CurveKind::Arc { start, end, .. }, "start") => {
            if e.swapped {
                end
            } else {
                start
            }
        }
        (CurveKind::Arc { start, end, .. }, "end") => {
            if e.swapped {
                start
            } else {
                end
            }
        }
        (CurveKind::Arc { center, .. } | CurveKind::Circle { center, .. }, "center") => center,
        _ => return unsupported(format!("point `{r}`")),
    };
    Ok(PointRef::Point(id))
}

fn curve_ref(info: &SketchInfo, r: &str) -> R<CurveId> {
    info.ents
        .get(r)
        .and_then(|e| e.curve)
        .map_or_else(|| unsupported(format!("curve `{r}`")), Ok)
}

fn is_point_ref(info: &SketchInfo, r: &str) -> bool {
    r == "origin" || r.contains('.') || info.ents.get(r).is_some_and(|e| e.point.is_some())
}

/// A recipe constraint (`{type, refs}`, as SolidCraft's `constraint_cmd` reads it) as a cadrs one.
fn constraint_spec(info: &SketchInfo, g: &Sketch, c: &Value, turned: bool) -> R<ConstraintSpec> {
    let refs: Vec<&str> = c["refs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let at = |i: usize| refs.get(i).copied().unwrap_or("");
    let pt = |r: &str| -> R<PointSpec> {
        Ok(match point_ref(info, g, r)? {
            PointRef::Origin => PointSpec::Origin,
            PointRef::Point(p) => PointSpec::Point(p),
        })
    };
    let cv = |r: &str| -> R<CurveSpec> { Ok(CurveSpec::Id(curve_ref(info, r)?)) };
    let ty = c["type"].as_str().unwrap_or("").to_ascii_lowercase();
    // Horizontal in the recipe's frame is vertical in cadrs' when the sketch is turned.
    let hv = |horizontal: bool, o: Orient<PointSpec, CurveSpec>| {
        if horizontal != turned {
            ConstraintSpec::Horizontal(o)
        } else {
            ConstraintSpec::Vertical(o)
        }
    };
    Ok(match ty.as_str() {
        "horizontal" | "vertical" if refs.len() >= 2 => {
            hv(ty == "horizontal", Orient::Points(pt(at(0))?, pt(at(1))?))
        }
        "horizontal" | "vertical" => hv(ty == "horizontal", Orient::Line(cv(at(0))?)),
        "horizontal_points" | "vertical_points" => hv(
            ty == "horizontal_points",
            Orient::Points(pt(at(0))?, pt(at(1))?),
        ),
        "coincident" | "point_on_curve" => {
            match (is_point_ref(info, at(0)), is_point_ref(info, at(1))) {
                (true, true) => ConstraintSpec::Coincident(pt(at(0))?, pt(at(1))?),
                (true, false) => ConstraintSpec::PointOnCurve(pt(at(0))?, cv(at(1))?),
                (false, true) => ConstraintSpec::PointOnCurve(pt(at(1))?, cv(at(0))?),
                _ => return unsupported("a coincident of two curves"),
            }
        }
        "midpoint" => {
            if is_point_ref(info, at(0)) {
                ConstraintSpec::Midpoint(pt(at(0))?, cv(at(1))?)
            } else {
                ConstraintSpec::Midpoint(pt(at(1))?, cv(at(0))?)
            }
        }
        "symmetric" | "symmetry" => {
            if is_point_ref(info, at(0)) {
                ConstraintSpec::SymmetricPoints(pt(at(0))?, pt(at(1))?, cv(at(2))?)
            } else {
                ConstraintSpec::SymmetricCurves(cv(at(0))?, cv(at(1))?, cv(at(2))?)
            }
        }
        "fix" => {
            if is_point_ref(info, at(0)) {
                ConstraintSpec::FixPoint(pt(at(0))?)
            } else {
                ConstraintSpec::FixCurve(cv(at(0))?)
            }
        }
        "tangent" => ConstraintSpec::Tangent(cv(at(0))?, cv(at(1))?),
        "equal" => ConstraintSpec::Equal(cv(at(0))?, cv(at(1))?),
        "parallel" => ConstraintSpec::Parallel(cv(at(0))?, cv(at(1))?),
        "perpendicular" => ConstraintSpec::Perpendicular(cv(at(0))?, cv(at(1))?),
        "concentric" => ConstraintSpec::Concentric(cv(at(0))?, cv(at(1))?),
        "collinear" => return unsupported("a collinear constraint"),
        other => return unsupported(format!("constraint `{other}`")),
    })
}

/// A recipe dimension as a cadrs one, on the geometry as it is now.
fn dimension_kind(
    info: &SketchInfo,
    g: &Sketch,
    d: &Value,
    turned: bool,
    value: f64,
) -> R<DimensionKind> {
    let refs: Vec<&str> = d["refs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let at = |i: usize| refs.get(i).copied().unwrap_or("");
    let id = |r: &str| -> R<PointId> {
        match point_ref(info, g, r)? {
            PointRef::Point(p) => Ok(p),
            PointRef::Origin => unsupported("the origin here"),
        }
    };
    Ok(match d["type"].as_str().unwrap_or("") {
        "radius" => DimensionKind::Radius {
            curve: curve_ref(info, at(0))?,
        },
        "diameter" => DimensionKind::Diameter {
            curve: curve_ref(info, at(0))?,
        },
        "offset" => {
            let (line, p) = if is_point_ref(info, at(0)) {
                (at(1), at(0))
            } else {
                (at(0), at(1))
            };
            DimensionKind::PointLine {
                p: point_ref(info, g, p)?,
                line: CurveRef::Curve(curve_ref(info, line)?),
            }
        }
        "distance" => {
            let o = d["orientation"].as_str().unwrap_or("aligned");
            let horizontal = match o {
                "horizontal" => Some(!turned),
                "vertical" => Some(turned),
                _ => None,
            };
            // From the origin: to the sketch axes (horizontal: the y axis), or straight.
            let (a, b) = (at(0), at(1));
            if a == "origin" || b == "origin" {
                let p = point_ref(info, g, if a == "origin" { b } else { a })?;
                return Ok(match horizontal {
                    Some(true) => DimensionKind::PointLine {
                        p,
                        line: CurveRef::YAxis,
                    },
                    Some(false) => DimensionKind::PointLine {
                        p,
                        line: CurveRef::XAxis,
                    },
                    None => match p {
                        PointRef::Point(p) => DimensionKind::OriginAligned { p },
                        PointRef::Origin => {
                            return unsupported("a distance from the origin to itself");
                        }
                    },
                });
            }
            let (a, b) = (id(a)?, id(b)?);
            match horizontal {
                Some(true) => DimensionKind::Horizontal { a, b },
                Some(false) => DimensionKind::Vertical { a, b },
                None => DimensionKind::Aligned { a, b },
            }
        }
        "angle" => {
            // The flips whose angle, as drawn, is nearest the value.
            let (ca, cb) = (curve_ref(info, at(0))?, curve_ref(info, at(1))?);
            let dir = |c: CurveId| -> R<Vec2> {
                match g.curves[c].kind {
                    CurveKind::Line { a, b } => {
                        let (p, q) = (g.points[a].pos, g.points[b].pos);
                        Ok(Vec2::new(q.x - p.x, q.y - p.y))
                    }
                    _ => unsupported("an angle to a curve that isn't a line"),
                }
            };
            let (u, v) = (dir(ca)?, dir(cb)?);
            let angle = |fa: bool, fb: bool| {
                let (u, v) = (
                    if fa { Vec2::new(-u.x, -u.y) } else { u },
                    if fb { Vec2::new(-v.x, -v.y) } else { v },
                );
                let c = (u.x * v.x + u.y * v.y) / (u.x.hypot(u.y) * v.x.hypot(v.y));
                c.clamp(-1.0, 1.0).acos().to_degrees()
            };
            let (fa, fb) = [(false, false), (true, false), (false, true), (true, true)]
                .into_iter()
                .min_by(|x, y| {
                    (angle(x.0, x.1) - value)
                        .abs()
                        .total_cmp(&(angle(y.0, y.1) - value).abs())
                })
                .unwrap();
            DimensionKind::Angle {
                a: CurveRef::Curve(ca),
                b: CurveRef::Curve(cb),
                flip_a: fa,
                flip_b: fb,
                ccw: None,
            }
        }
        t => return unsupported(format!("dimension `{t}`")),
    })
}

fn circumcentre(a: Vec2, b: Vec2, c: Vec2) -> Option<Vec2> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (
        a.x * a.x + a.y * a.y,
        b.x * b.x + b.y * b.y,
        c.x * c.x + c.y * c.y,
    );
    Some(Vec2::new(
        (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
        (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
    ))
}

/// Polylines joined end to end into one loop (each turned round where needed).
fn chain(mut pieces: Vec<Vec<Vec2>>) -> Vec<Vec2> {
    let close = |a: Vec2, b: Vec2| (a.x - b.x).abs() + (a.y - b.y).abs() < 1e-6;
    let mut out = pieces.remove(0);
    while !pieces.is_empty() {
        let end = *out.last().unwrap();
        let next = pieces
            .iter()
            .position(|p| close(p[0], end) || close(*p.last().unwrap(), end));
        let i = next.unwrap_or(0);
        let mut p = pieces.remove(i);
        if !close(p[0], end) && close(*p.last().unwrap(), end) {
            p.reverse();
        }
        out.extend(p);
    }
    out
}

fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// A point well inside a region: the middle of its largest triangle.
fn inside_point(r: &cadrs_sketch::region::Region) -> Vec2 {
    let (v, idx) = r.triangulate();
    idx.chunks(3)
        .map(|t| {
            let [a, b, c] = [v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]];
            let area = ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs();
            (
                area,
                Vec2::new((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0),
            )
        })
        .max_by(|x, y| x.0.total_cmp(&y.0))
        .map(|x| x.1)
        .unwrap_or(r.outer[0])
}

/// The distance from a point to a triangle.
fn point_triangle(p: V3, a: V3, b: V3, c: V3) -> f64 {
    let n = cross(sub(b, a), sub(c, a));
    let nn = dot(n, n);
    if nn < 1e-24 {
        return f64::INFINITY;
    }
    let inside = [(a, b), (b, c), (c, a)]
        .iter()
        .all(|(u, v)| dot(cross(sub(*v, *u), sub(p, *u)), n) >= 0.0);
    if inside {
        return (dot(sub(p, a), n) / nn.sqrt()).abs();
    }
    let seg = |u: V3, v: V3| {
        let d = sub(v, u);
        let t = (dot(sub(p, u), d) / dot(d, d).max(1e-300)).clamp(0.0, 1.0);
        norm(sub(p, add(u, scale(d, t))))
    };
    seg(a, b).min(seg(b, c)).min(seg(c, a))
}

/// What the part measures: totals over its solid bodies.
struct Measured {
    bodies: usize,
    volume: f64,
    area: f64,
    com: V3,
    bbox: (V3, V3),
    faces: usize,
    /// The worst relative difference between a face's triangles' area and its exact area: the
    /// mesh drawn covers each face, no more and no less.
    mesh: f64,
}

fn measure(parts: &[Part]) -> Measured {
    let solids: Vec<&Part> = parts.iter().filter(|p| p.kind == PartKind::Solid).collect();
    let (mut volume, mut area, mut faces, mut mesh) = (0.0, 0.0, 0, 0.0f64);
    let mut moment = [0.0; 3];
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in &solids {
        let m = p.mass.expect("mass properties");
        volume += m.volume;
        area += m.surface_area;
        faces += p.solid.faces.len();
        if std::env::var("DBG_FACES").is_ok() {
            for f in &p.solid.faces {
                eprintln!(
                    "DBG {:?} r {:?} area {:?} c {:?} name {:?}",
                    f.kind, f.radius, f.area, f.center, f.name
                );
            }
        }
        let s = &p.solid;
        for f in &s.faces {
            let Some(exact) = f.area else { continue };
            let triangles: f64 = (f.first_triangle..f.first_triangle + f.triangle_count)
                .map(|t| {
                    let q = |k: usize| s.positions[s.indices[3 * t + k] as usize];
                    0.5 * norm(cross(sub(q(1), q(0)), sub(q(2), q(0))))
                })
                .sum();
            mesh = mesh.max((triangles - exact).abs() / exact.max(1e-9));
        }
        let c = [m.center_of_mass.x, m.center_of_mass.y, m.center_of_mass.z];
        for k in 0..3 {
            moment[k] += m.volume * c[k];
        }
        for q in &p.solid.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    Measured {
        bodies: solids.len(),
        volume,
        area,
        com: moment.map(|x| x / volume),
        bbox: (lo, hi),
        faces,
        mesh,
    }
}

/// One comparison: what, the worst error (relative for totals, mm for positions) and whether
/// it passes.
struct Check {
    what: &'static str,
    error: f64,
    ok: bool,
}

struct Outcome {
    case: String,
    source: String,
    /// `Some(pass)` when built and compared; `None` when not translated (the reason in `note`).
    pass: Option<bool>,
    checks: Vec<Check>,
    note: String,
    /// What was measured, when it failed.
    measured: String,
}

fn run_case(dir: &Path) -> Outcome {
    let case = dir.file_name().unwrap().to_string_lossy().to_string();
    let read = |f: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap()
    };
    let mut want = read("expected.json");
    if dir.join("target.step").is_file() {
        target(dir, &mut want);
    }
    let source = want["source"].as_str().unwrap_or("").to_string();
    let mut outcome = Outcome {
        case,
        source,
        pass: None,
        checks: Vec::new(),
        note: String::new(),
        measured: String::new(),
    };
    // An issue's case is the user's own document; SolidCraft's, its recipe translated here.
    let (features, notes) = if dir.join("document.ron").is_file() {
        let d = Store::load_path(&dir.join("document.ron"))
            .expect("the case's document loads")
            .document;
        let studio = d
            .elements
            .iter()
            .find(|e| !e.features().is_empty())
            .expect("a Part Studio with features");
        (studio.features().to_vec(), Vec::new())
    } else {
        let recipe = read("recipe.json");
        let mut r = Recipe::new();
        for p in recipe["parameters"].as_array().into_iter().flatten() {
            if let (Some(n), Some(v)) = (
                p["name"].as_str(),
                p.get("value")
                    .or(p.get("expression"))
                    .and_then(|v| r.num(Some(v)).ok()),
            ) {
                r.params.insert(n.to_string(), v);
            }
        }
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> R<()> {
            for f in recipe["features"].as_array().into_iter().flatten() {
                r.feature(f)?;
            }
            Ok(())
        }));
        match built {
            Ok(Ok(())) => {}
            Ok(Err(Stop::Unsupported(why))) => {
                outcome.note = format!("not translated yet: {why}");
                return outcome;
            }
            Ok(Err(Stop::Failed(why))) => {
                outcome.pass = Some(false);
                outcome.note = format!("failed: {why}");
                return outcome;
            }
            Err(_) => {
                outcome.pass = Some(false);
                outcome.note = "failed: panicked".into();
                return outcome;
            }
        }
        (r.features_now(), r.notes)
    };
    let features = Arc::new(features);
    let mut rb = Rebuilder::new();
    let build = rb.rebuild(&features);
    if let Some((_, e)) = build.errors.first() {
        outcome.pass = Some(false);
        outcome.note = format!("failed: {e}");
        return outcome;
    }
    let got = measure(&build.parts);
    let only: Option<Vec<String>> = want["compare"].as_array().map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect()
    });
    let wanted = |k: &str| {
        want.get(k).is_some_and(|v| !v.is_null())
            && only.as_ref().is_none_or(|o| o.iter().any(|x| x == k))
    };
    let rel = want["tolerance"]["rel"].as_f64().unwrap_or(REL_TOL);
    let want_bbox = v3(&want["bbox"]["min"]).zip(v3(&want["bbox"]["max"]));
    let diag = |b: (V3, V3)| norm(sub(b.1, b.0));
    let abs = want["tolerance"]["abs_mm"].as_f64().unwrap_or_else(|| {
        ABS_FRAC
            * want_bbox
                .map(diag)
                .unwrap_or_else(|| diag(got.bbox))
                .max(1.0)
    });
    let mut checks = Vec::new();
    if wanted("body_count") {
        let ok = want["body_count"].as_u64() == Some(got.bodies as u64);
        checks.push(Check {
            what: "bodies",
            error: (got.bodies as f64 - want["body_count"].as_f64().unwrap_or(0.0)).abs(),
            ok,
        });
    }
    for (key, what, g) in [
        ("volume_mm3", "volume", got.volume),
        ("area_mm2", "area", got.area),
    ] {
        if wanted(key) {
            let w = want[key].as_f64().unwrap_or(f64::NAN);
            let e = (g - w).abs() / w.abs().max(1.0);
            checks.push(Check {
                what,
                error: e,
                ok: e <= rel,
            });
        }
    }
    let far = |g: V3, w: V3| (0..3).map(|k| (g[k] - w[k]).abs()).fold(0.0, f64::max);
    if wanted("center_of_mass") {
        let e = far(
            got.com,
            v3(&want["center_of_mass"]).unwrap_or([f64::NAN; 3]),
        );
        checks.push(Check {
            what: "centre of mass",
            error: e,
            ok: e <= abs,
        });
    }
    if wanted("bbox")
        && let Some((lo, hi)) = want_bbox
    {
        let e = far(got.bbox.0, lo).max(far(got.bbox.1, hi));
        checks.push(Check {
            what: "bounding box",
            error: e,
            ok: e <= abs,
        });
    }
    if wanted("faces") {
        let w = want["faces"].as_u64().unwrap_or(0) as usize;
        checks.push(Check {
            what: "faces",
            error: (got.faces as f64 - w as f64).abs(),
            ok: got.faces == w,
        });
    }
    // The mesh drawn: each face's triangles cover the face.
    checks.push(Check {
        what: "mesh",
        error: got.mesh,
        ok: got.mesh <= MESH_TOL,
    });
    // STEP: written and read back with the same volume and area.
    let (ok, e) = step_round_trip(&features, &mut rb, &got);
    checks.push(Check {
        what: "STEP round trip",
        error: e,
        ok,
    });
    outcome.pass = Some(checks.iter().all(|c| c.ok));
    if outcome.pass == Some(false) {
        outcome.measured = format!(
            "got  bodies {} volume {:.4} area {:.4} com {:.4?} bbox {:.4?} faces {} mesh {:.1e}\n  \
             want bodies {} volume {:.4} area {:.4} com {:.4?} bbox {:.4?} faces {}",
            got.bodies,
            got.volume,
            got.area,
            got.com,
            got.bbox,
            got.faces,
            got.mesh,
            want["body_count"],
            want["volume_mm3"].as_f64().unwrap_or(f64::NAN),
            want["area_mm2"].as_f64().unwrap_or(f64::NAN),
            v3(&want["center_of_mass"]).unwrap_or([f64::NAN; 3]),
            want_bbox.unwrap_or(([f64::NAN; 3], [f64::NAN; 3])),
            want["faces"],
        );
    }
    let bad: Vec<String> = checks
        .iter()
        .filter(|c| !c.ok)
        .map(|c| format!("{} off by {:.3e}", c.what, c.error))
        .collect();
    let mut notes = notes;
    notes.extend(bad);
    outcome.note = notes.join("; ");
    outcome.checks = checks;
    outcome
}

/// An issue case's expected values from its `target.step` (the part as another CAD system
/// builds it), measured as cadrs reads it: the ones `expected.json` doesn't give itself.
fn target(dir: &Path, want: &mut Value) {
    let text = std::fs::read_to_string(dir.join("target.step")).unwrap();
    let plan = Rebuilder::new()
        .plan_import(ImportFormat::Step, text.as_bytes())
        .expect("the target reads");
    let doc = imported_document(
        &plan,
        "target.step",
        text,
        ImportAs::PartStudio,
        ImportIds::fresh(),
    );
    let build = Rebuilder::new().rebuild(doc.elements[0].features());
    assert!(
        build.errors.is_empty(),
        "the target builds: {:?}",
        build.errors
    );
    let m = measure(&build.parts);
    let measured = serde_json::json!({
        "body_count": m.bodies,
        "volume_mm3": m.volume,
        "area_mm2": m.area,
        "center_of_mass": m.com,
        "bbox": { "min": m.bbox.0, "max": m.bbox.1 },
        "faces": m.faces,
    });
    for (k, v) in measured.as_object().unwrap() {
        if want.get(k).is_none() {
            want[k] = v.clone();
        }
    }
}

/// The part exported as STEP and imported again: the worst relative change of volume or area.
fn step_round_trip(
    features: &Arc<Vec<Feature>>,
    rb: &mut Rebuilder,
    got: &Measured,
) -> (bool, f64) {
    let build = rb.rebuild(features);
    let items: Vec<ExportItem> = build
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::Solid)
        .map(|p| ExportItem {
            features: features.clone(),
            part: p.id,
            name: p.name.clone(),
            pose: None,
            source: (ElementId::from_u128(1), p.id),
            source_name: p.name.clone(),
        })
        .collect();
    let Ok(files) = rb.export_files(&ExportRequest::new(ModelFormat::Step, "part", items)) else {
        return (false, f64::INFINITY);
    };
    let text = String::from_utf8_lossy(&files[0].bytes).into_owned();
    let Ok(plan) = rb.plan_import(ImportFormat::Step, text.as_bytes()) else {
        return (false, f64::INFINITY);
    };
    let doc = imported_document(
        &plan,
        "part.step",
        text,
        ImportAs::PartStudio,
        ImportIds::fresh(),
    );
    let back = Rebuilder::new().rebuild(doc.elements[0].features());
    if !back.errors.is_empty() {
        return (false, f64::INFINITY);
    }
    let m = measure(&back.parts);
    let e = ((m.volume - got.volume).abs() / got.volume.abs().max(1.0))
        .max((m.area - got.area).abs() / got.area.abs().max(1.0));
    (e <= REL_TOL, e)
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reference_parts")
}

fn table(outcomes: &[Outcome]) -> String {
    let passed = outcomes.iter().filter(|o| o.pass == Some(true)).count();
    let failed = outcomes.iter().filter(|o| o.pass == Some(false)).count();
    let skipped = outcomes.iter().filter(|o| o.pass.is_none()).count();
    let mut s = String::new();
    s.push_str("# Reference parts\n\n");
    s.push_str(
        "Generated by `CADRS_BLESS=1 cargo test -r -p cadrs_core --test reference_parts` (issue \
         #101). Each case is a part described by a recipe, with expected values worked out \
         independently of cadrs: closed-form geometry (**analytic**) or an independent kernel \
         (**freecad**). The cases are [SolidCraft's](https://github.com/tangency-labs/solidcraft) \
         reference parts (MIT; see `crates/cadrs_core/tests/reference_parts/README.md`) and parts \
         from cadrs' issues (the user's document, against the part as **onshape** or \
         **solidcraft** builds it), rebuilt through cadrs' feature list and compared as SolidCraft compares them: body \
         count, volume and area within a relative 1e-3, centre of mass and bounding box within \
         1e-3 of the part's diagonal, face count where the geometry fixes it, and a STEP export \
         read back with the same volume and area; and the mesh drawn, each face's triangles \
         covering its exact area within 1e-2. The error columns give how close cadrs comes.\n\n",
    );
    s.push_str(&format!(
        "**{passed} / {} cases pass**, {failed} fail, {skipped} not translated yet.\n\n",
        outcomes.len()
    ));
    s.push_str("| Case | Source | Result | Volume error | Area error | Notes |\n|---|---|---|---|---|---|\n");
    for o in outcomes {
        let result = match o.pass {
            Some(true) => "pass",
            Some(false) => "**fail**",
            None => "not translated",
        };
        let err = |w: &str| {
            o.checks
                .iter()
                .find(|c| c.what == w)
                .map_or(String::new(), |c| format!("{:.1e}", c.error))
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            o.case,
            o.source,
            result,
            err("volume"),
            err("area"),
            o.note.replace('|', "\\|")
        ));
    }
    s
}

/// The cases' folders: SolidCraft's (`solidcraft/<case>/`), then those from issues
/// (`issues/<case>/`), each in order.
fn case_dirs() -> Vec<PathBuf> {
    ["solidcraft", "issues"]
        .iter()
        .flat_map(|set| {
            let mut cases: Vec<PathBuf> = std::fs::read_dir(root().join(set))
                .unwrap()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.join("expected.json").is_file())
                .collect();
            cases.sort();
            cases
        })
        .collect()
}

/// A test's case: `case_49_helical_sweep` is `solidcraft/49-helical-sweep/`, and
/// `issue_156_face_chamfer_fillet` is `issues/156-face-chamfer-fillet/`.
fn case_of(test: &str) -> PathBuf {
    let (set, name) = match test.strip_prefix("issue_") {
        Some(name) => ("issues", name),
        None => ("solidcraft", test.trim_start_matches("case_")),
    };
    root().join(set).join(name.replace('_', "-"))
}

/// The cases that must pass (`passing.txt`).
fn must_pass() -> Vec<String> {
    std::fs::read_to_string(root().join("passing.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .collect()
}

/// One case's test: it fails if the case doesn't pass and must. A case from an issue always
/// must (it's a bug to fix); SolidCraft's once `passing.txt` lists them. (While blessing,
/// `bless` runs them all instead.)
fn check(test: &str) {
    if std::env::var("CADRS_BLESS").is_ok() {
        return;
    }
    let dir = case_of(test);
    let o = run_case(&dir);
    let case = &o.case;
    let must = dir.parent().is_some_and(|p| p.ends_with("issues")) || must_pass().contains(case);
    match o.pass {
        Some(false) if must => panic!("{case} fails: {}\n  {}", o.note, o.measured),
        Some(false) => println!("FAIL {case} {} (not in passing.txt)", o.note),
        None if must => panic!("{case} {}", o.note),
        None => println!("SKIP {case} {}", o.note),
        Some(true) => println!("PASS {case} {}", o.note),
    }
}

/// A test per case, named after its folder; `every_case_has_a_test` keeps the list whole.
macro_rules! cases {
    ($($test:ident)*) => {
        $(
            #[test]
            fn $test() {
                check(stringify!($test));
            }
        )*
        const CASES: &[&str] = &[$(stringify!($test)),*];
    };
}

cases! {
    case_01_box
    case_02_cylinder
    case_03_box_through_hole
    case_04_revolve_vase
    case_05_taper_extrude
    case_06_symmetric_extrude
    case_07_fillet_all_edges
    case_08_chamfer
    case_09_shell
    case_10_rect_pattern_holes
    case_11_circular_pattern_flange
    case_12_mirror_feature
    case_13_combine_join
    case_14_combine_cut
    case_15_combine_intersect
    case_16_sweep
    case_17_loft
    case_18_hole_features
    case_19_l_bracket_constrained
    case_20_slot_constrained
    case_21_draft
    case_22_split_body
    case_23_bracket_multifeature
    case_24_user_parameters
    case_25_sketch_triangle
    case_26_sketch_belt
    case_27_sketch_trapezoid
    case_28_blind_pocket_fillets
    case_29_two_sided_and_partial_revolve
    case_30_fillet_after_shell
    case_31_fillet_l_all_edges
    case_32_boss_plate_fillets
    case_33_fillet_tangent_chain
    case_34_chamfer_then_fillet
    case_35_fillet_curved_curved
    case_36_corner_blend_two_radii
    case_37_pyramid_apex_blend
    case_38_coincident_cylinders
    case_39_tee_pipe
    case_40_box_sphere_torus
    case_41_tube_radial_holes
    case_42_overlapping_patterns
    case_43_cut_splits_body
    case_44_shell_l_bracket
    case_45_shell_boss_plate
    case_46_shell_two_open_faces
    case_47_draft_curved_pocket
    case_48_pulley
    case_49_helical_sweep
    case_50_sweep_tight_bend
    case_51_loft_rect_circle_offset
    case_52_circular_pattern_fillet
    case_53_path_pattern
    case_54_mirror_body_fillets
    case_55_tapped_hole_cosmetic
    case_56_holes_on_curved_face
    case_57_hole_pattern_sketch_points
    case_58_phone_stand
    case_59_enclosure_base
    case_60_pipe_flange
    case_61_shaft_keyway
    case_62_heat_sink
    case_63_hinge_knuckle
    case_64_sketch_tangent_arc_chain
    case_65_sketch_gear_tooth
    case_66_sketch_linkage
    case_67_sm_base_flange
    case_68_sm_edge_flange
    case_69_sm_tray
    case_70_sm_flange_45
    case_71_sm_custom_rule
    case_72_sm_contour_flange
    case_73_sm_hem
    case_74_sm_unfold_refold
    case_75_sm_bracket_holes
    case_76_sm_thickness_variant
    case_77_fillet_variable_linear
    case_78_fillet_variable_to_zero
    case_79_fillet_chord_length
    case_80_fillet_taper_to_fit
    case_81_fillet_spline_radius
    case_82_thin_extrude_frame
    case_83_boundary_loop_thicken
    issue_156_face_chamfer_fillet
    issue_156_wall_as_thick_as_chamfer
}

/// A case copied in needs its line in `cases!`, and a test its folder.
#[test]
fn every_case_has_a_test() {
    let tested: Vec<PathBuf> = CASES.iter().map(|t| case_of(t)).collect();
    assert_eq!(
        case_dirs(),
        tested,
        "the case folders and the tests in `cases!` differ"
    );
}

/// With `CADRS_BLESS=1`: every case run, the ones passing now recorded in `passing.txt` and
/// the table written to `docs/reference-parts.md`.
#[test]
fn bless() {
    if std::env::var("CADRS_BLESS").is_err() {
        return;
    }
    let outcomes: Vec<Outcome> = case_dirs().iter().map(|c| run_case(c)).collect();
    for o in &outcomes {
        let result = match o.pass {
            Some(true) => "PASS",
            Some(false) => "FAIL",
            None => "SKIP",
        };
        println!("{result} {} {}", o.case, o.note);
    }
    let now: Vec<&str> = outcomes
        .iter()
        .filter(|o| o.pass == Some(true))
        .map(|o| o.case.as_str())
        .collect();
    std::fs::write(root().join("passing.txt"), now.join("\n") + "\n").unwrap();
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("reference-parts.md"), table(&outcomes)).unwrap();
}

/// A clamped B-spline of degree `p` with control values `cps` on `knots`, at u (de Boor).
fn bspline(cps: &[f64], knots: &[f64], p: usize, u: f64) -> f64 {
    let n = cps.len();
    let u = u.clamp(knots[p], knots[n]);
    // The span: knots[k] <= u < knots[k + 1], the last one at the end.
    let k = (p..n)
        .rfind(|&i| knots[i] <= u && (u < knots[i + 1] || i == n - 1))
        .unwrap_or(p);
    let mut d: Vec<f64> = (0..=p).map(|j| cps[j + k - p]).collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let den = knots[i + p + 1 - r] - knots[i];
            let a = if den > 0.0 { (u - knots[i]) / den } else { 0.0 };
            d[j] = (1.0 - a) * d[j - 1] + a * d[j];
        }
    }
    d[p]
}

/// The helix about the world Z axis through `pts` in order, if they lie on one (a constant
/// radius, the height rising evenly with the angle): a Helix feature of that radius, pitch,
/// turns and start, turning the way the points do.
fn helix_through(pts: &[V3]) -> Option<cadrs_core::surfacing::HelixFeature> {
    use cadrs_core::surfacing::{HelixFeature, HelixPath, HelixType};
    if pts.len() < 4 {
        return None;
    }
    let r = pts[0][0].hypot(pts[0][1]);
    // The angles unwrapped along the points.
    let mut a = vec![pts[0][1].atan2(pts[0][0])];
    for w in pts.windows(2) {
        let d = (w[1][1].atan2(w[1][0]) - w[0][1].atan2(w[0][0]) + std::f64::consts::PI)
            .rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        a.push(a.last().unwrap() + d);
    }
    let (a0, a1) = (a[0], *a.last().unwrap());
    let (z0, z1) = (pts[0][2], pts.last().unwrap()[2]);
    if r < 1e-9 || (a1 - a0).abs() < 1e-9 || (z1 - z0).abs() < 1e-9 {
        return None;
    }
    let rise = (z1 - z0) / (a1 - a0);
    let on = pts.iter().zip(&a).all(|(p, t)| {
        (p[0].hypot(p[1]) - r).abs() < 1e-6 * r
            && (p[2] - (z0 + rise * (t - a0))).abs() < 1e-6 * (1.0 + r)
    });
    if !on || z1 < z0 {
        return None;
    }
    let turns = (a1 - a0).abs() / std::f64::consts::TAU;
    let height = z1 - z0;
    let pitch = height / turns;
    Some(HelixFeature {
        helix_type: HelixType::Axis,
        axis: Some(AxisRef::Connector(ConnectorRef::Implicit(
            ConnectorOrigin::Origin,
        ))),
        path: HelixPath::TurnsAndPitch,
        revolutions: turns,
        revolutions_expr: format!("{turns}"),
        pitch,
        pitch_expr: format!("{pitch} mm"),
        height,
        height_expr: format!("{height} mm"),
        radius: r,
        radius_expr: format!("{r} mm"),
        start_angle: a0.to_degrees(),
        start_angle_expr: format!("{} deg", a0.to_degrees()),
        // Seen from its start looking up the axis, counter-clockwise from above is clockwise.
        clockwise: a1 > a0,
        ..HelixFeature::default()
    })
}
