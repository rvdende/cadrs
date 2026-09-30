//! **Standard content** (P3B.5, `intro-to-assemblies.md` A19, A21.2, A21.14, X12): a bundled,
//! data-driven fastener library, and its parts placed in assemblies.
//!
//! - **The library** ([`library`]) is `data/standard_content.ron`: Standard / Category / Class /
//!   Component, each component with its sizes (every row cites the standard table its
//!   dimensions come from), lengths and options (material, bearing face, finish). ANSI inch:
//!   hex cap screws (ASME B18.2.1), hex nuts (B18.2.2, Chamfered or Washer faced), pan head
//!   machine screws (B18.6.3), plain washers (B18.22.1); ISO: socket head cap screws (ISO 4762),
//!   hex nuts (ISO 4032) and thin nuts (ISO 4035), plain washers (ISO 7089). Adding rows or
//!   components to the data file is all it takes to extend it: the generator works from the
//!   component's kind and the row's dimensions.
//! - **A configuration** ([`StandardSpec`]: the component and its options) is **generated**
//!   ([`generate`]) as a Part Studio of ordinary parametric features (sketches, extrudes, a
//!   revolve that chamfers a hex), rebuilt by the kernel like any other studio. Threads are
//!   drawn as plain cylinders of the nominal diameter. The part's origin is its bearing face
//!   (a screw's head underside, a nut's or washer's base), its Z the fastener's axis pointing
//!   out of the hole (a screw's shank runs along −Z).
//! - **In a document** each configuration used is a [`StandardPart`] of
//!   [`Document::standard_content`]: the generated studio (not a tab; [`Document::element`]
//!   finds it), with its **Part number** and **Description** (A19.3, per document). An instance
//!   of it is an ordinary part instance of that studio, so drawing, picking, mates and mass
//!   properties work unchanged. Two instances have the **same configuration** when they have
//!   the same source.
//! - **Placement** (A19.5–A19.7): [`site_of_edge`] / [`sites_of_face`] find where fasteners go
//!   (a hole's or shaft's circular edge, its outward side), [`plan_insert`] places one per site
//!   with a **Fastened** mate (optionally stacked on what is already mated there), and
//!   [`InsertStandardContent`] applies it as one undo step. [`auto_size`] picks the size for a
//!   hole or shaft (A19.2): bolts and screws round down, nuts and washers round up.
//! - **Edit standard content instance** (A19.9, bulk edit): [`EditStandardContent`] changes the
//!   size or length of several instances at once; their mates stay.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};

use super::connector::{ConnectorAnchor, ConnectorFrame, EntityRef, ImplicitPoint, MateConnector, canonical_axis};
use super::mate::{Mate, MateFeature, MateId, MateKind, MateOffset, MateType, next_name};
use super::{Assembly, Instance, InstanceId, InstanceSource, Pose};
use crate::appearance::Appearance;
use crate::command::{Command, CommandError, History, Scope};
use crate::commands::{AddExtrude, AddRevolve, AddSketch, EditSketch, RenamePart, SetExtrude, SetPartAppearance, SetPartMaterial};
use crate::document::{AxisRef, BooleanOp, Document, Element, ExtrudeFeature, Offset, RevolveFeature, RevolveType};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::samples::gear_cover::{DocHistory, Studio};
use crate::solid::{EdgeName, FaceName, Solid};

// ---------------------------------------------------------------------------------------------
// The library

/// The unit a standard's dimensions are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum LibUnit {
    Inch,
    Millimeter,
}

impl LibUnit {
    /// Millimetres per unit.
    pub fn mm(self) -> f64 {
        match self {
            LibUnit::Inch => 25.4,
            LibUnit::Millimeter => 1.0,
        }
    }
}

/// What a component is, which sets how it is generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Kind {
    HexCapScrew,
    PanHeadScrew,
    SocketHeadCapScrew,
    HexNut,
    PlainWasher,
}

impl Kind {
    /// A bolt or screw (externally threaded: it goes into a hole, auto-size rounds down).
    pub fn is_screw(self) -> bool {
        matches!(self, Kind::HexCapScrew | Kind::PanHeadScrew | Kind::SocketHeadCapScrew)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Library {
    pub standards: Vec<Standard>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Standard {
    pub name: String,
    pub unit: LibUnit,
    pub categories: Vec<Category>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Category {
    pub name: String,
    pub classes: Vec<Class>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Class {
    pub name: String,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Component {
    pub name: String,
    pub kind: Kind,
    /// The standard it follows ("ASME B18.2.1").
    pub standard: String,
    /// The part number's prefix ("HCS").
    pub prefix: String,
    /// Catalogue lengths (screws), in the standard's unit.
    #[serde(default)]
    pub lengths: Vec<f64>,
    #[serde(default)]
    pub default_length: f64,
    pub materials: Vec<String>,
    #[serde(default)]
    pub bearing_faces: Vec<String>,
    #[serde(default)]
    pub finishes: Vec<String>,
    pub sizes: Vec<Size>,
}

/// A size row (see the data file's header for the fields).
#[derive(Debug, Clone, Deserialize)]
pub struct Size {
    pub name: String,
    pub d: f64,
    #[serde(default)]
    pub f: f64,
    #[serde(default)]
    pub g: f64,
    pub h: f64,
    #[serde(default)]
    pub head: Option<f64>,
    #[serde(default)]
    pub inner: Option<f64>,
    #[serde(default)]
    pub socket: Option<f64>,
    #[serde(default)]
    pub slot: Option<f64>,
    /// Where the dimensions come from.
    pub source: String,
}

/// The bundled library (`data/standard_content.ron`).
pub fn library() -> &'static Library {
    static LIB: OnceLock<Library> = OnceLock::new();
    LIB.get_or_init(|| ron::from_str(include_str!("../../data/standard_content.ron")).expect("data/standard_content.ron is valid"))
}

impl Library {
    pub fn standard(&self, name: &str) -> Option<&Standard> {
        self.standards.iter().find(|s| s.name == name)
    }
}

impl Standard {
    pub fn category(&self, name: &str) -> Option<&Category> {
        self.categories.iter().find(|c| c.name == name)
    }
}

impl Category {
    pub fn class(&self, name: &str) -> Option<&Class> {
        self.classes.iter().find(|c| c.name == name)
    }
}

impl Class {
    pub fn component(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }
}

impl Component {
    pub fn size(&self, name: &str) -> Option<&Size> {
        self.sizes.iter().find(|s| s.name == name)
    }

    /// Whether it has a length (screws).
    pub fn has_length(&self) -> bool {
        !self.lengths.is_empty()
    }
}

/// A length as the dropdowns show it: "0.75", "1", "16".
pub fn fmt_len(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

// ---------------------------------------------------------------------------------------------
// A configuration

/// A standard content configuration: the component and its options (A19.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StandardSpec {
    pub standard: String,
    pub category: String,
    pub class: String,
    pub component: String,
    pub size: String,
    /// Screws: the length, in the standard's unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_length: Option<f64>,
    pub material: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearing_face: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
}

impl StandardSpec {
    /// The first configuration of a component (its first size, default length and options).
    pub fn new(standard: &str, category: &str, class: &str, component: &str) -> Option<Self> {
        let mut s = Self {
            standard: standard.into(),
            category: category.into(),
            class: class.into(),
            component: component.into(),
            size: String::new(),
            length: None,
            thread_length: None,
            material: String::new(),
            bearing_face: None,
            finish: None,
        };
        s.component_def()?;
        s.normalize();
        Some(s)
    }

    /// The standard and component it is of.
    pub fn component_def(&self) -> Option<(&'static Standard, &'static Component)> {
        let st = library().standard(&self.standard)?;
        let c = st.category(&self.category)?.class(&self.class)?.component(&self.component)?;
        Some((st, c))
    }

    /// Its size row.
    pub fn size_row(&self) -> Option<&'static Size> {
        self.component_def()?.1.size(&self.size)
    }

    /// Makes every option valid for the component: an unknown size, length or option becomes
    /// the first (or default) one; the thread length follows the length.
    pub fn normalize(&mut self) {
        let Some((_, c)) = self.component_def() else { return };
        if c.size(&self.size).is_none() {
            self.size = c.sizes.first().map(|s| s.name.clone()).unwrap_or_default();
        }
        if c.has_length() {
            let l = self.length.filter(|l| c.lengths.iter().any(|x| (x - l).abs() < 1e-9));
            self.length = Some(l.unwrap_or(if c.default_length > 0.0 { c.default_length } else { c.lengths[0] }));
            self.thread_length = Some(self.default_thread_length());
        } else {
            self.length = None;
            self.thread_length = None;
        }
        if !c.materials.contains(&self.material) {
            self.material = c.materials.first().cloned().unwrap_or_default();
        }
        let pick = |v: &Option<String>, list: &[String]| -> Option<String> {
            if list.is_empty() {
                return None;
            }
            Some(v.clone().filter(|x| list.contains(x)).unwrap_or_else(|| list[0].clone()))
        };
        self.bearing_face = pick(&self.bearing_face, &c.bearing_faces);
        self.finish = pick(&self.finish, &c.finishes);
    }

    /// The thread length the standard gives for the length: ASME B18.2.1 LT = 2D + 1/4 (screws
    /// up to 6 in), ISO 4762 b = 2d + 12; a screw shorter than that is threaded full length.
    pub fn default_thread_length(&self) -> f64 {
        let (Some((st, _)), Some(row), Some(l)) = (self.component_def(), self.size_row(), self.length) else { return 0.0 };
        let lt = match st.unit {
            LibUnit::Inch => 2.0 * row.d + 0.25,
            LibUnit::Millimeter => 2.0 * row.d + 12.0,
        };
        lt.min(l)
    }

    /// The part's name: "Hex cap screw 1/4-28 x 0.75", "Hex nut 3/8-16".
    pub fn part_name(&self) -> String {
        match self.length {
            Some(l) => format!("{} {} x {}", self.component, self.size, fmt_len(l)),
            None => format!("{} {}", self.component, self.size),
        }
    }

    /// The Description (A19.3): "Hex cap screw 1/4-28 x 0.75 Stainless Steel", with a finish
    /// other than Plain.
    pub fn description(&self) -> String {
        let mut s = format!("{} {}", self.part_name(), self.material);
        if let Some(f) = self.finish.as_ref().filter(|f| f.as_str() != "Plain") {
            s.push_str(&format!(", {f}"));
        }
        s
    }

    /// The Part number the library suggests: "HCS-1/4-28-0.75-SS".
    pub fn part_number(&self) -> String {
        let prefix = self.component_def().map(|(_, c)| c.prefix.replace(' ', "-")).unwrap_or_default();
        let mat: String = self.material.split_whitespace().filter_map(|w| w.chars().next()).collect::<String>().to_uppercase();
        let mut s = format!("{prefix}-{}", self.size.replace(' ', ""));
        if let Some(l) = self.length {
            s.push_str(&format!("-{}", fmt_len(l)));
        }
        s.push_str(&format!("-{mat}"));
        if let Some(f) = self.finish.as_ref().filter(|f| f.as_str() != "Plain") {
            s.push_str(&format!("-{}", f.split_whitespace().filter_map(|w| w.chars().next()).collect::<String>().to_uppercase()));
        }
        if self.bearing_face.as_deref() == Some("Washer faced") {
            s.push_str("-WF");
        }
        s
    }

    /// The id its generated studio has in any document: derived from the configuration, so the
    /// same configuration is one studio.
    pub fn element_id(&self) -> ElementId {
        let text = ron::to_string(self).unwrap_or_default();
        // FNV-1a, 128 bit.
        let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
        for b in b"cadrs standard content\0".iter().chain(text.as_bytes()) {
            h ^= *b as u128;
            h = h.wrapping_mul(0x0000000001000000000000000000013B);
        }
        ElementId::from_u128(h)
    }

    /// The same configuration with another size and/or length (Edit standard content instance,
    /// A19.9): the rest is kept, made valid for the new size.
    pub fn with(&self, size: Option<&str>, length: Option<f64>) -> Self {
        let mut s = self.clone();
        if let Some(z) = size {
            s.size = z.to_string();
        }
        if let Some(l) = length {
            s.length = Some(l);
        }
        s.normalize();
        s
    }

    /// How far it stacks along its axis (a nut's or washer's thickness, a head's height), mm.
    pub fn stack_height(&self) -> f64 {
        match (self.component_def(), self.size_row()) {
            (Some((st, _)), Some(row)) => row.h * st.unit.mm(),
            _ => 0.0,
        }
    }
}

/// A standard content configuration used by a document: its generated Part Studio, with the
/// Part number and Description (A19.3; kept per document).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StandardPart {
    pub spec: StandardSpec,
    #[serde(default)]
    pub part_number: String,
    #[serde(default)]
    pub description: String,
    /// The generated Part Studio (its id is [`StandardSpec::element_id`]).
    pub element: Element,
}

impl StandardPart {
    /// The configuration generated, with the library's part number and description.
    pub fn new(spec: &StandardSpec) -> Result<Self, CommandError> {
        Ok(Self { spec: spec.clone(), part_number: spec.part_number(), description: spec.description(), element: generate(spec)? })
    }

    /// Its instance source.
    pub fn source(&self) -> InstanceSource {
        InstanceSource::Part { element: self.element.id, part: PART }
    }
}

/// Adds `part` to the document's standard content unless its configuration is there (then its
/// Part number and Description are updated).
pub fn upsert(doc: &mut Document, part: &StandardPart) {
    match doc.standard_content.iter_mut().find(|p| p.element.id == part.element.id) {
        Some(p) => {
            p.part_number = part.part_number.clone();
            p.description = part.description.clone();
        }
        None => doc.standard_content.push(part.clone()),
    }
}

/// The standard content configuration an instance source is of.
pub fn standard_of<'a>(doc: &'a Document, source: &InstanceSource) -> Option<&'a StandardPart> {
    match source {
        InstanceSource::Part { element, .. } => doc.standard_part(*element),
        InstanceSource::Assembly { .. } | InstanceSource::Studio { .. } => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Auto-size

/// **Auto-size** (A19.2): the size of `component` for a hole or shaft of `diameter` (mm).
/// Bolts and screws round **down** (the largest nominal diameter that fits the hole); nuts and
/// washers round **up** (the smallest that goes over the shaft). Among the sizes of that
/// diameter the current one is kept (its thread series), else the first (coarse) one.
pub fn auto_size(unit: LibUnit, component: &Component, diameter: f64, current: &str) -> Option<String> {
    let tol = 1e-3;
    let d = |s: &Size| s.d * unit.mm();
    let pick = if component.kind.is_screw() {
        component.sizes.iter().filter(|s| d(s) <= diameter + tol).map(d).fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))))
    } else {
        component.sizes.iter().filter(|s| d(s) >= diameter - tol).map(d).fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.min(x))))
    }?;
    let same: Vec<&Size> = component.sizes.iter().filter(|s| (d(s) - pick).abs() < 1e-9).collect();
    same.iter().find(|s| s.name == current).or(same.first()).map(|s| s.name.clone())
}

// ---------------------------------------------------------------------------------------------
// The generator

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x5c0e_0000_0000_0000_0000_0000_0000_0000 + n)
}

/// The feature that makes the fastener's body: its part is [`PART`] in every configuration.
pub const BODY: FeatureId = fid(0x02);
/// The one part of a generated studio.
pub const PART: PartId = PartId::new(BODY, 0);

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn circle(r: f64) -> SketchOp {
    SketchOp::AddCircle { center: v(0.0, 0.0), radius: r, construction: false }
}

/// A hexagon `f` across flats, its corners on ±X.
fn hexagon(f: f64) -> SketchOp {
    let r = f / 3f64.sqrt();
    let points = (0..6).map(|k| {
        let a = k as f64 * std::f64::consts::PI / 3.0;
        v(r * a.cos(), r * a.sin())
    });
    SketchOp::AddPolyline { points: points.collect(), closed: true, construction: false, label: "Add polygon" }
}

fn sketch(s: &mut dyn Studio, el: ElementId, id: FeatureId, plane: PlaneRef, ops: Vec<SketchOp>) -> Result<cadrs_sketch::Sketch, CommandError> {
    s.run(&AddSketch { element: el, feature: id, plane: Some(plane) })?;
    s.run(&EditSketch { element: el, feature: id, op: SketchOp::Batch(ops) })?;
    s.document()
        .element(el)
        .and_then(|x| x.feature(id))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

fn region(sketch_id: FeatureId, g: &cadrs_sketch::Sketch, seed: Vec2) -> Result<Vec<crate::document::RegionRef>, CommandError> {
    let r = crate::samples::region_refs(sketch_id, g, &[seed]);
    if r.len() != 1 {
        return Err(CommandError::Invalid("a region of the fastener is missing".into()));
    }
    Ok(r)
}

/// Extrudes the region of a Top sketch under `seed` from z0 to z1 (mm).
#[allow(clippy::too_many_arguments)]
fn extrude(s: &mut dyn Studio, el: ElementId, sk: FeatureId, g: &cadrs_sketch::Sketch, feature: FeatureId, seed: Vec2, z: (f64, f64), op: BooleanOp) -> Result<(), CommandError> {
    let (z0, z1) = z;
    let mut x = crate::samples::extrude_of(region(sk, g, seed)?, z1 - z0);
    x.op = op;
    if op != BooleanOp::New {
        x.merge_scope = vec![PART];
    }
    if z0 != 0.0 {
        x.start_offset = Some(Offset { value: z0.abs(), expr: format!("{} mm", fmt_len(z0.abs())), flip: z0 < 0.0 });
    }
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: x, label: "Extrude".into() })
}

/// Revolves a profile (Front sketch: x = radius, y = height; its points in order) a full turn
/// about Z.
fn revolve(s: &mut dyn Studio, el: ElementId, sk: FeatureId, feature: FeatureId, profile: &[(f64, f64)], op: BooleanOp) -> Result<(), CommandError> {
    let lo = profile.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    let hi = profile.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let pad = (hi - lo) * 0.2 + 1.0;
    let g = sketch(
        s,
        el,
        sk,
        PlaneRef::Front,
        vec![
            SketchOp::AddPolyline { points: vec![v(0.0, lo - pad), v(0.0, hi + pad)], closed: false, construction: true, label: "Add line" },
            SketchOp::AddPolyline { points: profile.iter().map(|p| v(p.0, p.1)).collect(), closed: true, construction: false, label: "Add line" },
        ],
    )?;
    let axis = g.curves.iter().find(|(_, c)| c.construction).map(|(k, _)| k).ok_or_else(|| CommandError::Invalid("revolve axis".into()))?;
    // A seed just inside the profile: near its widest point at mid height.
    let mid = (lo + hi) / 2.0;
    let seed = v(profile.iter().map(|p| p.0).fold(0.0, f64::max) * 0.25, mid);
    let mut r = RevolveFeature {
        regions: region(sk, &g, seed)?,
        axis: Some(AxisRef::SketchCurve { sketch: sk, curve: axis }),
        kind: RevolveType::Full,
        angle: 360.0,
        angle_expr: "360 deg".into(),
        op,
        ..RevolveFeature::default()
    };
    if op != BooleanOp::New {
        r.merge_scope = vec![PART];
    }
    s.run(&AddRevolve { element: el, feature, revolve: r })
}

/// The chamfer of a hex (a 30° cone cutting its corners down to a circle `rc` across) at its
/// top, and at its bottom too when `both`; `wf`: a washer face of that height under it (a
/// disc of radius `rc`). `g2`: just past the hex's corners.
fn hex_chamfer_profile(h: f64, rc: f64, g2: f64, both: bool, wf: f64) -> Vec<(f64, f64)> {
    let c = (g2 - rc) * (30f64).to_radians().tan();
    let mut p = vec![(0.0, 0.0)];
    if wf > 0.0 {
        p.extend([(rc, 0.0), (rc, wf), (g2, wf)]);
    } else if both {
        p.extend([(rc, 0.0), (g2, c)]);
    } else {
        p.push((g2, 0.0));
    }
    p.extend([(g2, h - c), (rc, h), (0.0, h)]);
    p
}

/// Generates a configuration as a Part Studio (its id [`StandardSpec::element_id`]) whose one
/// part [`PART`] is the fastener, named, with its material and look.
pub fn generate(spec: &StandardSpec) -> Result<Element, CommandError> {
    let (st, comp) = spec.component_def().ok_or_else(|| CommandError::Invalid(format!("no standard content component {}", spec.component)))?;
    let row = spec.size_row().ok_or_else(|| CommandError::Invalid(format!("no size {} of {}", spec.size, spec.component)))?;
    let u = st.unit.mm();
    let mut doc = Document::empty("Standard content");
    let mut element = Element::part_studio(spec.part_name());
    element.id = spec.element_id();
    let el = element.id;
    doc.elements.push(element);
    let mut h = History::default();
    build_fastener(&mut DocHistory(&mut doc, &mut h), el, spec, comp, row, u)?;
    let mut element = doc.elements.pop().ok_or_else(|| CommandError::Invalid("generated studio missing".into()))?;
    element.name = "Standard content".into();
    Ok(element)
}

fn build_fastener(s: &mut dyn Studio, el: ElementId, spec: &StandardSpec, comp: &Component, row: &Size, u: f64) -> Result<(), CommandError> {
    let d = row.d * u;
    let hh = row.h * u;
    let length = spec.length.unwrap_or(0.0) * u;
    let top = PlaneRef::Top;
    match comp.kind {
        Kind::HexCapScrew => {
            let f = row.f * u;
            let g = sketch(s, el, fid(0x01), top, vec![hexagon(f)])?;
            extrude(s, el, fid(0x01), &g, BODY, v(0.0, 0.0), (0.0, hh), BooleanOp::New)?;
            // The head's top chamfer (a 30° cone down to about F across).
            let g2 = f / 3f64.sqrt() * 1.02;
            revolve(s, el, fid(0x03), fid(0x04), &hex_chamfer_profile(hh, 0.95 * f / 2.0, g2, false, 0.0), BooleanOp::Intersect)?;
            let g = sketch(s, el, fid(0x05), top, vec![circle(d / 2.0)])?;
            extrude(s, el, fid(0x05), &g, fid(0x06), v(0.0, 0.0), (-length, 0.0), BooleanOp::Add)?;
        }
        Kind::HexNut => {
            let f = row.f * u;
            let g = sketch(s, el, fid(0x01), top, vec![hexagon(f), circle(d / 2.0)])?;
            extrude(s, el, fid(0x01), &g, BODY, v((d / 2.0 + f / 2.0) / 2.0, 0.0), (0.0, hh), BooleanOp::New)?;
            let g2 = f / 3f64.sqrt() * 1.02;
            let washer_faced = spec.bearing_face.as_deref() == Some("Washer faced");
            // ASME B18.2.2's washer face: 0.015 in thick.
            let wf = if washer_faced { (0.015f64 * 25.4).min(hh / 4.0) } else { 0.0 };
            revolve(s, el, fid(0x03), fid(0x04), &hex_chamfer_profile(hh, 0.95 * f / 2.0, g2, true, wf), BooleanOp::Intersect)?;
        }
        Kind::PanHeadScrew => {
            let a = row.head.unwrap_or(2.0 * row.d) * u / 2.0;
            let profile = [(0.0, -length), (d / 2.0, -length), (d / 2.0, 0.0), (a, 0.0), (a, 0.45 * hh), (0.88 * a, 0.82 * hh), (0.68 * a, hh), (0.0, hh)];
            revolve(s, el, fid(0x01), BODY, &profile, BooleanOp::New)?;
            if let Some(j) = row.slot {
                let (j, w) = (j * u / 2.0, a * 1.2);
                let rect = SketchOp::AddPolyline { points: vec![v(-w, -j), v(w, -j), v(w, j), v(-w, j)], closed: true, construction: false, label: "Add rectangle" };
                let g = sketch(s, el, fid(0x03), top, vec![rect])?;
                extrude(s, el, fid(0x03), &g, fid(0x04), v(0.0, 0.0), (0.6 * hh, 1.3 * hh), BooleanOp::Remove)?;
            }
        }
        Kind::SocketHeadCapScrew => {
            let dk = row.head.unwrap_or(1.5 * row.d) * u;
            let g = sketch(s, el, fid(0x01), top, vec![circle(dk / 2.0)])?;
            extrude(s, el, fid(0x01), &g, BODY, v(0.0, 0.0), (0.0, hh), BooleanOp::New)?;
            if let Some(sk) = row.socket {
                let g = sketch(s, el, fid(0x03), top, vec![hexagon(sk * u)])?;
                extrude(s, el, fid(0x03), &g, fid(0x04), v(0.0, 0.0), (0.5 * hh, 1.2 * hh), BooleanOp::Remove)?;
            }
            let g = sketch(s, el, fid(0x05), top, vec![circle(d / 2.0)])?;
            extrude(s, el, fid(0x05), &g, fid(0x06), v(0.0, 0.0), (-length, 0.0), BooleanOp::Add)?;
        }
        Kind::PlainWasher => {
            let od = row.head.unwrap_or(2.0 * row.d) * u;
            let id = row.inner.unwrap_or(row.d) * u;
            let g = sketch(s, el, fid(0x01), top, vec![circle(od / 2.0), circle(id / 2.0)])?;
            extrude(s, el, fid(0x01), &g, BODY, v((od + id) / 4.0, 0.0), (0.0, hh), BooleanOp::New)?;
        }
    }
    s.run(&RenamePart { element: el, part: PART, name: spec.part_name() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![PART], material: material(&spec.material) })?;
    s.run(&SetPartAppearance { element: el, parts: vec![PART], appearance: Some(appearance(spec)) })
}

/// The library material for a material option ("Stainless Steel" is AISI 304).
pub fn material(name: &str) -> Option<crate::material::Material> {
    let lib = match name {
        "Stainless Steel" => "Stainless Steel - 304",
        "Aluminum" => "Aluminum - 6061",
        other => other,
    };
    crate::material::library(lib)
}

/// How a configuration looks: its material, darkened by a black oxide finish, yellowed by zinc.
pub fn appearance(spec: &StandardSpec) -> Appearance {
    match (spec.finish.as_deref(), spec.material.as_str()) {
        (Some("Black oxide"), _) => Appearance::rgb(52, 52, 56),
        (Some("Zinc plated"), _) => Appearance::rgb(196, 190, 150),
        (_, "Brass") => Appearance::rgb(201, 164, 84),
        (_, "Steel") => Appearance::rgb(120, 124, 130),
        _ => Appearance::rgb(178, 181, 186),
    }
}

// ---------------------------------------------------------------------------------------------
// Where fasteners go

/// A place for a fastener (A19.5, A19.6): a circular edge of a hole or shaft of an occurrence,
/// with the side the fastener goes on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoleSite {
    /// The occurrence (in the assembly's solver model) the edge is on.
    pub instance: InstanceId,
    pub edge: EdgeName,
    /// The circle's diameter, mm.
    pub diameter: f64,
    /// A shaft's edge (convex), not a hole's.
    pub shaft: bool,
    /// The edge's connector Z (its axis, pointed the canonical way) is flipped so that it
    /// points out of the part, away from the face the edge bounds.
    pub flip: bool,
}

impl HoleSite {
    /// Its mate connector: the circle's centre, Z out of the part (A flips it).
    pub fn connector(&self, solid: &Solid, flip: bool) -> MateConnector {
        let point = ImplicitPoint::CircleCenter(self.edge);
        let owner = EntityRef::Edge(self.edge);
        let frame = super::connector::resolve_implicit(solid, &point, &owner).unwrap_or_default();
        MateConnector {
            instance: self.instance,
            anchor: ConnectorAnchor::Implicit { point, owner },
            frame,
            flip: self.flip != flip,
            reorient: 0,
            edit: Default::default(),
        }
    }
}

fn v3(a: [f64; 3]) -> Vector3<f64> {
    Vector3::from(a)
}

/// Whether a cylindrical face is convex (a shaft) rather than concave (a hole): its mesh
/// normals point away from its axis.
fn is_convex(s: &Solid, face: &FaceName) -> Option<bool> {
    let f = s.face(face)?;
    let (p0, dir) = f.axis?;
    let (p0, dir) = (v3(p0), v3(dir).normalize());
    let mut sum = 0.0;
    for t in f.first_triangle..f.first_triangle + f.triangle_count {
        for k in 0..3 {
            let i = s.indices[3 * t + k] as usize;
            let (Some(p), Some(n)) = (s.positions.get(i), s.normals.get(i)) else { continue };
            let r = v3(*p) - p0;
            let radial = r - dir * r.dot(&dir);
            sum += radial.dot(&v3(*n));
        }
    }
    Some(sum > 0.0)
}

/// The site of a circular edge: `None` if it isn't a whole circle.
pub fn site_of_edge(s: &Solid, instance: InstanceId, edge: &EdgeName) -> Option<HoleSite> {
    let e = s.edge(edge)?;
    let c = e.circle?;
    let closed = e.points.len() > 2 && {
        let (a, b) = (v3(e.points[0]), v3(*e.points.last()?));
        (a - b).norm() < 1e-3 * c.radius.max(1e-9)
    };
    if !closed {
        return None;
    }
    let z = canonical_axis(c.normal);
    // Out of the part: the outward normal of the planar face the edge bounds.
    let outward = edge.faces.iter().find_map(|f| super::connector::face_normal(s, f)).unwrap_or(z);
    let shaft = edge.faces.iter().find_map(|f| is_convex(s, f)).unwrap_or(false);
    Some(HoleSite { instance, edge: *edge, diameter: 2.0 * c.radius, shaft, flip: v3(z).dot(&v3(outward)) < 0.0 })
}

/// The sites of a face (A19.6): a planar face gives every hole in it (whole circles, not convex); a
/// hole's or shaft's cylindrical face gives its upper end (along the canonical axis).
pub fn sites_of_face(s: &Solid, instance: InstanceId, face: &FaceName) -> Vec<HoleSite> {
    let Some(f) = s.face(face) else { return Vec::new() };
    let sites: Vec<HoleSite> = s.edges.iter().filter(|e| e.name.faces.contains(face)).filter_map(|e| site_of_edge(s, instance, &e.name)).collect();
    if f.plane.is_some() {
        // Its holes (not its outer boundary, nor a boss's edge).
        return sites.into_iter().filter(|s| !s.shaft).collect();
    }
    let top = sites.iter().max_by(|a, b| {
        let at = |x: &HoleSite| s.edge(&x.edge).and_then(|e| e.circle).map(|c| v3(c.center).dot(&v3(canonical_axis(c.normal)))).unwrap_or(0.0);
        at(a).total_cmp(&at(b))
    });
    top.copied().into_iter().collect()
}

// ---------------------------------------------------------------------------------------------
// Inserting

/// Where a new instance goes relative to what is already mated on a site (A19.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stacking {
    /// On the site (plain Insert): the fastener sits on the face.
    #[default]
    Plain,
    /// **Insert closest to selection**: against the face, the stack already there moved out
    /// by its height.
    Closest,
    /// **Insert furthest from selection**: on top of the stack.
    Furthest,
}

/// One new instance: its placement and its Fastened mate to a site.
#[derive(Debug, Clone, PartialEq)]
pub struct StdInsert {
    pub instance: InstanceId,
    pub mate: MateId,
    pub pose: Pose,
    /// The site's connector (the mate's second connector), if it is placed on one.
    pub hole: Option<MateConnector>,
    /// How far out along the site's Z (mm): the mate's offset.
    pub offset: f64,
}

/// **Insert standard content** (A19.5–A19.7, A21.2, A21.14): the configuration added to the
/// document (if new), one instance per [`StdInsert`], each with its **Fastened** mate
/// ("Fastened n"), and a stack moved out (`restack`: a mate's new offset and its instance's new
/// placement). One undo step ([`Scope::Whole`]: it edits the document's standard content and
/// the assembly).
#[derive(Debug, Clone)]
pub struct InsertStandardContent {
    pub element: ElementId,
    pub part: StandardPart,
    pub inserts: Vec<StdInsert>,
    pub restack: Vec<(MateId, f64, InstanceId, Pose)>,
}

fn offset_of(z: f64) -> Option<MateOffset> {
    (z.abs() > 1e-12).then_some(MateOffset { translation: [0.0, 0.0, z], axis: 2, angle: 0.0 })
}

impl Command for InsertStandardContent {
    fn label(&self) -> String {
        format!("Insert {}", self.part.spec.part_name())
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.inserts.is_empty() {
            return Err(CommandError::Invalid("nothing to insert".into()));
        }
        upsert(doc, &self.part);
        let source = self.part.source();
        let asm = super::commands::assembly_mut(doc, self.element)?;
        for ins in &self.inserts {
            if asm.instance(ins.instance).is_some() {
                return Err(CommandError::Invalid("instance id already in use".into()));
            }
            let mut inst = Instance::new(ins.instance, source, ins.pose);
            inst.index = asm.next_index(&source);
            asm.instances.push(inst);
            if let Some(hole) = ins.hole {
                let mut m = Mate::new(MateType::Fastened, MateConnector::at(ins.instance, ConnectorFrame::default()), hole);
                m.offset = offset_of(ins.offset);
                let name = next_name(&asm.mates, "Fastened");
                asm.mates.push(MateFeature::new(ins.mate, name, MateKind::Mate(m)));
            }
        }
        for (mate, z, inst, pose) in &self.restack {
            if let Some(m) = asm.mates.iter_mut().find(|m| m.id == *mate).and_then(|m| m.mate_mut()) {
                m.offset = offset_of(*z);
            }
            if let Some(i) = asm.instance_mut(*inst) {
                i.pose = *pose;
            }
        }
        Ok(())
    }
}

/// What is stacked on a site: the Fastened mates of standard content instances to the same
/// connector, as (mate, instance, offset, height), nearest first.
pub fn stack_on(doc: &Document, asm: &Assembly, hole: &MateConnector) -> Vec<(MateId, InstanceId, f64, f64)> {
    let mut out = Vec::new();
    for f in &asm.mates {
        let Some(m) = f.mate() else { continue };
        let b = &m.connectors[1];
        if m.mate_type != MateType::Fastened || b.instance != hole.instance || b.anchor != hole.anchor || b.flip != hole.flip {
            continue;
        }
        let a = m.connectors[0].instance;
        let Some(sp) = asm.instance(a).and_then(|i| standard_of(doc, &i.source)) else { continue };
        let z = m.offset.map(|o| o.translation[2]).unwrap_or(0.0);
        out.push((f.id, a, z, sp.spec.stack_height()));
    }
    out.sort_by(|x, y| x.2.total_cmp(&y.2));
    out
}

/// Plans inserting `part` on `sites` of the assembly `element` (A19.5–A19.7): one instance per
/// site, at the site's connector (flipped when `flip`, A), stacked as `stacking` says. `solids`
/// are the occurrences' source solids ([`super::occurrence_solids`]).
pub fn plan_insert(
    doc: &Document,
    element: ElementId,
    part: StandardPart,
    sites: &[HoleSite],
    flip: bool,
    stacking: Stacking,
    solids: &HashMap<InstanceId, Arc<Solid>>,
) -> Result<InsertStandardContent, CommandError> {
    let asm = doc.element(element).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(element))?;
    let occ: HashMap<InstanceId, Pose> = super::structure::occurrences(doc, asm).into_iter().map(|o| (o.id, o.pose)).collect();
    let height = part.spec.stack_height();
    let mut inserts = Vec::new();
    let mut restack = Vec::new();
    for site in sites {
        let solid = solids.get(&site.instance).ok_or_else(|| CommandError::Invalid("the site's part is missing".into()))?;
        let pose = occ.get(&site.instance).copied().ok_or_else(|| CommandError::Invalid("the site's instance is missing".into()))?;
        let hole = site.connector(solid, flip);
        let world = hole.local_frame(Some(solid)).moved(&pose);
        let stack = stack_on(doc, asm, &hole);
        let offset = match stacking {
            Stacking::Plain => 0.0,
            Stacking::Furthest => stack.iter().map(|s| s.2 + s.3).fold(0.0, f64::max),
            Stacking::Closest => {
                for (m, i, z, _) in &stack {
                    let z = z + height;
                    restack.push((*m, z, *i, world.then_local(&Pose::translation([0.0, 0.0, z])).pose()));
                }
                0.0
            }
        };
        let at = world.then_local(&Pose::translation([0.0, 0.0, offset]));
        inserts.push(StdInsert { instance: InstanceId::new(), mate: MateId::new(), pose: at.pose(), hole: Some(hole), offset });
    }
    Ok(InsertStandardContent { element, part, inserts, restack })
}

// ---------------------------------------------------------------------------------------------
// Editing

/// **Edit standard content instance** (A19.9): the Size and/or Length of standard content
/// instances changed, all at once (bulk edit). Each instance keeps its id, placement and mates
/// (its connector is its origin, which stays the bearing face) and takes the next number of its
/// new configuration. One undo step.
#[derive(Debug, Clone)]
pub struct EditStandardContent {
    pub element: ElementId,
    /// Each instance and its new configuration.
    pub changes: Vec<(InstanceId, ElementId)>,
    /// The configurations, generated.
    pub parts: Vec<StandardPart>,
}

impl EditStandardContent {
    /// The edit of `instances` of the assembly `element` to `size` and / or `length` (options
    /// the new size doesn't have fall back to its first ones).
    pub fn new(doc: &Document, element: ElementId, instances: &[InstanceId], size: Option<&str>, length: Option<f64>) -> Result<Self, CommandError> {
        let asm = doc.element(element).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(element))?;
        let mut parts: Vec<StandardPart> = Vec::new();
        let mut changes = Vec::new();
        for id in instances {
            let inst = asm.instance(*id).ok_or_else(|| CommandError::Invalid(format!("instance {id} not found")))?;
            let old = standard_of(doc, &inst.source).ok_or_else(|| CommandError::Invalid("not a standard content instance".into()))?;
            let spec = old.spec.with(size, length);
            let el = spec.element_id();
            if !parts.iter().any(|p| p.element.id == el) {
                match doc.standard_part(el) {
                    Some(p) => parts.push(p.clone()),
                    None => parts.push(StandardPart::new(&spec)?),
                }
            }
            changes.push((*id, el));
        }
        Ok(Self { element, changes, parts })
    }
}

impl Command for EditStandardContent {
    fn label(&self) -> String {
        "Edit standard content".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.changes.is_empty() {
            return Err(CommandError::Invalid("no instances".into()));
        }
        for p in &self.parts {
            upsert(doc, p);
        }
        let asm = super::commands::assembly_mut(doc, self.element)?;
        for (id, el) in &self.changes {
            let source = InstanceSource::Part { element: *el, part: PART };
            let Some(inst) = asm.instance(*id) else { return Err(CommandError::Invalid(format!("instance {id} not found"))) };
            if inst.source == source {
                continue;
            }
            let index = asm.next_index(&source);
            if let Some(i) = asm.instance_mut(*id) {
                i.source = source;
                i.index = index;
            }
        }
        Ok(())
    }
}

/// Sets the Part number and Description of a configuration used by the document (A19.3).
#[derive(Debug, Clone)]
pub struct SetStandardProperties {
    pub element: ElementId,
    pub part_number: String,
    pub description: String,
}

impl Command for SetStandardProperties {
    fn label(&self) -> String {
        "Set standard content properties".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let p = doc.standard_content.iter_mut().find(|p| p.element.id == self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        p.part_number = self.part_number.clone();
        p.description = self.description.clone();
        Ok(())
    }
}

/// The instances of `asm` with the same configuration as `instance` (Select instances with same
/// configuration, A19.9): the same standard content configuration (or, for other parts, the
/// same Part Studio). With `same_part`, also the same part.
pub fn same_configuration(asm: &Assembly, instance: InstanceId, same_part: bool) -> Vec<InstanceId> {
    let Some(src) = asm.instance(instance).map(|i| i.source) else { return Vec::new() };
    asm.instances
        .iter()
        .filter(|i| i.source.element() == src.element() && (!same_part || i.source == src))
        .map(|i| i.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_loads_and_specs_normalize() {
        let lib = library();
        assert_eq!(lib.standards.len(), 2);
        let s = StandardSpec::new("ANSI inch", "Bolts & screws", "Hex bolts", "Hex cap screw").unwrap();
        assert_eq!(s.size, "1/4-20");
        assert_eq!(s.length, Some(0.75));
        assert_eq!(s.thread_length, Some(0.75));
        let s2 = s.with(Some("1/4-28"), Some(2.0));
        assert_eq!(s2.thread_length, Some(0.75));
        assert_ne!(s.element_id(), s2.element_id());
        assert_eq!(s.element_id(), s.clone().element_id());
        let n = StandardSpec::new("ANSI inch", "Nuts", "Hex nuts", "Hex nut").unwrap();
        assert_eq!(n.bearing_face.as_deref(), Some("Chamfered"));
        assert_eq!(n.finish.as_deref(), Some("Plain"));
        assert_eq!(n.part_name(), "Hex nut 1/4-20");
    }
}
