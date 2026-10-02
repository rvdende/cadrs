//! cadrs's own **sheet metal forms library** (P3I.9, SM20): "cadrs sheet metal forms", one Part
//! Studio per form, each built from ordinary features as a user would build a form:
//!
//! - Variable features `thickness` (driven by the sheet metal model when the form is placed) and
//!   the form's sizes (Length, Width, Height, Diameter);
//! - **Form profile**: a construction-only sketch on Top, the outline the flat pattern shows;
//! - **Add** and **Remove**: the parts it adds to and removes from the sheet (extrudes);
//! - **Form origin**: a mate connector at the origin (Z out of the sheet; the sheet lies below,
//!   `z` from `−thickness` to 0);
//! - **Tag 1**: the Tag (Form) feature naming them.
//!
//! The forms (our own shapes, none of Onshape's library):
//! - **Louver**: a hood pressed up out of a slot, rising along a quarter sine from one long side
//!   of the slot to its open lip at the Height;
//! - **Bridge lance**: a strip cut free along both long sides and raised the Height as a bridge
//!   with 45° ramps;
//! - **Dimple**: a round, flat-topped boss, hollow underneath;
//! - **Emboss**: a rectangular, flat-topped boss, hollow underneath;
//! - **Extruded hole**: a hole with a collar of the Height drawn down out of the sheet.
//!
//! [`studio`] builds one form's features for given variables and thickness (what the Form
//! feature rebuilds from); [`document`] is the whole library as a document.

use std::f64::consts::{FRAC_PI_2, PI};

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use crate::document::{Document, Element, ExtrudeFeature, Feature, FeatureKind, Offset};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::mate::{ConnectorOrigin, ConnectorRef, MateConnectorFeature};
use crate::sheetmetal::plain;
use crate::sheetmetal_form::{FormVariable, LIBRARY_NAME, LibraryForm, TagFormFeature};
use crate::variables::VariableFeature;

use super::gear_cover::{DocHistory, Studio};

/// The library document.
pub const DOCUMENT: DocumentId = DocumentId::from_u128(0x5390_0000_0000_0000_0000_0000_0000_0100);

fn index(form: LibraryForm) -> u128 {
    LibraryForm::ALL.iter().position(|f| *f == form).unwrap_or(0) as u128
}

/// The form's Part Studio in the library document.
pub fn studio_id(form: LibraryForm) -> ElementId {
    ElementId::from_u128(0x5390_0000_0000_0000_0000_0000_0001_0000 | (index(form) << 8))
}

fn fid(form: LibraryForm, k: u128) -> FeatureId {
    FeatureId::from_u128(0x5390_0000_0000_0000_0000_0000_0002_0000 | (index(form) << 8) | k)
}

/// The form's parts: the add part and the remove part.
pub fn add_part(form: LibraryForm) -> PartId {
    PartId::new(fid(form, 0x21), 0)
}

pub fn remove_part(form: LibraryForm) -> PartId {
    PartId::new(fid(form, 0x31), 0)
}

/// The form's flat-view sketch and origin connector.
pub fn profile_sketch(form: LibraryForm) -> FeatureId {
    fid(form, 0x10)
}

pub fn origin_connector(form: LibraryForm) -> FeatureId {
    fid(form, 0x40)
}

fn value(vars: &[FormVariable], name: &str, form: LibraryForm) -> f64 {
    vars.iter()
        .find(|v| v.name == name)
        .map(|v| v.value)
        .unwrap_or_else(|| form.variables().into_iter().find(|v| v.name == name).map_or(1.0, |v| v.value))
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64, construction: bool) -> SketchOp {
    SketchOp::AddPolyline {
        points: vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)],
        closed: true,
        construction,
        label: "Add rectangle",
    }
}

fn poly(points: Vec<Vec2>) -> SketchOp {
    SketchOp::AddPolyline { points, closed: true, construction: false, label: "Add polyline" }
}

fn circle(r: f64, construction: bool) -> SketchOp {
    SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: r, construction }
}

/// A sketch on `plane` with `ops`.
fn sketch(s: &mut dyn Studio, el: ElementId, id: FeatureId, plane: PlaneRef, ops: Vec<SketchOp>) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: id, plane: Some(plane) })?;
    for op in ops {
        s.run(&EditSketch { element: el, feature: id, op })?;
    }
    Ok(())
}

/// An extrude (New) of the regions of `sketch` under `seeds`.
fn extrude(s: &mut dyn Studio, el: ElementId, sk: FeatureId, id: FeatureId, seeds: &[Vec2], set: impl FnOnce(&mut ExtrudeFeature)) -> Result<(), CommandError> {
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(sk))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("form sketch not found".into()))?;
    let regions = super::region_refs(sk, &g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid("a region of the form is missing".into()));
    }
    let mut e = super::extrude_of(regions, 1.0);
    set(&mut e);
    s.run(&AddExtrude { element: el, feature: id, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: id, extrude: e, label: "Extrude".into() })?;
    Ok(())
}

/// A Top-plane extrude from `z0` to `z1`.
fn between(z0: f64, z1: f64) -> impl FnOnce(&mut ExtrudeFeature) {
    move |e: &mut ExtrudeFeature| {
        let d = z1 - z0;
        e.depth = d;
        e.depth_expr = format!("{} mm", plain(d));
        if z0.abs() > 1e-12 {
            e.start_offset = Some(Offset { value: z0.abs(), expr: format!("{} mm", plain(z0.abs())), flip: z0 < 0.0 });
        }
    }
}

/// A symmetric extrude `width` wide.
fn symmetric(width: f64) -> impl FnOnce(&mut ExtrudeFeature) {
    move |e: &mut ExtrudeFeature| {
        e.symmetric = true;
        e.depth = width;
        e.depth_expr = format!("{} mm", plain(width));
    }
}

/// Adds the form's features to the Part Studio `el` (see the module docs).
pub fn build_in(s: &mut dyn Studio, el: ElementId, form: LibraryForm, vars: &[FormVariable], t: f64) -> Result<(), CommandError> {
    let v = |n: &str| value(vars, n, form);
    // The variables.
    let mut all = vec![FormVariable::length("thickness", t)];
    for d in form.variables() {
        all.push(FormVariable::length(&d.name, v(&d.name)));
    }
    for (k, x) in all.iter().enumerate() {
        let mut var = VariableFeature::length(&x.name, &x.expr);
        if var.value == 0.0 {
            var.value = x.value;
        }
        s.run(&AddFeature { element: el, feature: fid(form, 0x01 + k as u128), base_name: "Variable".into(), kind: FeatureKind::Variable(var) })?;
    }
    let (sk_add, sk_rem) = (fid(form, 0x20), fid(form, 0x30));
    let (add, rem) = (add_part(form).feature, remove_part(form).feature);
    let gap = 0.5;
    match form {
        LibraryForm::Louver => {
            let (l, w, h) = (v("Length"), v("Width"), v("Height").max(t * 1.05));
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![rect(-l / 2.0, -w / 2.0, l / 2.0, w / 2.0, true)])?;
            // The hood's section on Right (y, z), extruded along x.
            let n = 16;
            let rise = |y: f64| h * (FRAC_PI_2 * (y + w / 2.0) / w).sin();
            let ys: Vec<f64> = (0..=n).map(|i| -w / 2.0 + w * i as f64 / n as f64).collect();
            let mut pts: Vec<Vec2> = ys.iter().map(|y| Vec2::new(*y, rise(*y))).collect();
            pts.extend(ys.iter().rev().map(|y| Vec2::new(*y, rise(*y) - t)));
            sketch(s, el, sk_add, PlaneRef::Right, vec![poly(pts)])?;
            extrude(s, el, sk_add, add, &[Vec2::new(0.0, rise(0.0) - t / 2.0)], symmetric(l))?;
            // The opening: from where the hood clears the sheet to its lip.
            let c = w * (2.0 / PI) * (t / h).min(1.0).asin() + 0.02 * w;
            sketch(s, el, sk_rem, PlaneRef::Top, vec![rect(-l / 2.0, -w / 2.0 + c, l / 2.0, w / 2.0, false)])?;
            extrude(s, el, sk_rem, rem, &[Vec2::new(0.0, (c + w) / 2.0 - w / 2.0 + 0.001)], between(-t - gap, 0.0))?;
        }
        LibraryForm::Lance => {
            let (l, w) = (v("Length"), v("Width"));
            let h = v("Height").max(t * 1.05).min(l / 2.0 - t);
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![rect(-l / 2.0, -w / 2.0, l / 2.0, w / 2.0, true)])?;
            let pts = vec![
                Vec2::new(-l / 2.0, 0.0),
                Vec2::new(-l / 2.0 + h, h),
                Vec2::new(l / 2.0 - h, h),
                Vec2::new(l / 2.0, 0.0),
                Vec2::new(l / 2.0, -t),
                Vec2::new(l / 2.0 - h, h - t),
                Vec2::new(-l / 2.0 + h, h - t),
                Vec2::new(-l / 2.0, -t),
            ];
            sketch(s, el, sk_add, PlaneRef::Front, vec![poly(pts)])?;
            extrude(s, el, sk_add, add, &[Vec2::new(0.0, h - t / 2.0)], symmetric(w))?;
            let c = t * 1.5;
            sketch(s, el, sk_rem, PlaneRef::Top, vec![rect(-l / 2.0 + c, -w / 2.0, l / 2.0 - c, w / 2.0, false)])?;
            extrude(s, el, sk_rem, rem, &[Vec2::new(0.0, 0.0)], between(-t - gap, 0.0))?;
        }
        LibraryForm::Dimple => {
            let (d, h) = (v("Diameter").max(4.0 * t), v("Height").max(t * 1.05));
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![circle(d / 2.0, true)])?;
            sketch(s, el, sk_add, PlaneRef::Top, vec![circle(d / 2.0, false)])?;
            extrude(s, el, sk_add, add, &[Vec2::new(0.0, 0.0)], between(-t, h))?;
            sketch(s, el, sk_rem, PlaneRef::Top, vec![circle(d / 2.0 - t, false)])?;
            extrude(s, el, sk_rem, rem, &[Vec2::new(0.0, 0.0)], between(-t - gap, h - t))?;
        }
        LibraryForm::Emboss => {
            let (l, w, h) = (v("Length").max(4.0 * t), v("Width").max(4.0 * t), v("Height").max(t * 1.05));
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![rect(-l / 2.0, -w / 2.0, l / 2.0, w / 2.0, true)])?;
            sketch(s, el, sk_add, PlaneRef::Top, vec![rect(-l / 2.0, -w / 2.0, l / 2.0, w / 2.0, false)])?;
            extrude(s, el, sk_add, add, &[Vec2::new(0.0, 0.0)], between(-t, h))?;
            sketch(s, el, sk_rem, PlaneRef::Top, vec![rect(-l / 2.0 + t, -w / 2.0 + t, l / 2.0 - t, w / 2.0 - t, false)])?;
            extrude(s, el, sk_rem, rem, &[Vec2::new(0.0, 0.0)], between(-t - gap, h - t))?;
        }
        LibraryForm::ExtrudedHole => {
            let (d, h) = (v("Diameter"), v("Height"));
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![circle(d / 2.0, true)])?;
            sketch(s, el, sk_add, PlaneRef::Top, vec![circle(d / 2.0 + t, false), circle(d / 2.0, false)])?;
            extrude(s, el, sk_add, add, &[Vec2::new(d / 2.0 + t / 2.0, 0.0)], between(-t - h, 0.0))?;
            sketch(s, el, sk_rem, PlaneRef::Top, vec![circle(d / 2.0, false)])?;
            extrude(s, el, sk_rem, rem, &[Vec2::new(0.0, 0.0)], between(-t - h - gap, gap))?;
        }
    }
    let mc = MateConnectorFeature { origin: Some(ConnectorOrigin::Origin), ..Default::default() };
    s.run(&AddFeature { element: el, feature: origin_connector(form), base_name: "Mate connector".into(), kind: FeatureKind::MateConnector(mc) })?;
    let tag = TagFormFeature {
        add: vec![add_part(form)],
        remove: vec![remove_part(form)],
        sketch: Some(profile_sketch(form)),
        origin: Some(ConnectorRef::Feature(origin_connector(form))),
        ..Default::default()
    };
    s.run(&AddFeature { element: el, feature: fid(form, 0x50), base_name: "Tag".into(), kind: FeatureKind::TagForm(tag) })?;
    // The names a form author would give them.
    rename(s, el, profile_sketch(form), "Form profile")?;
    rename(s, el, origin_connector(form), "Form origin")?;
    Ok(())
}

fn rename(s: &mut dyn Studio, el: ElementId, f: FeatureId, name: &str) -> Result<(), CommandError> {
    s.run(&crate::commands::RenameFeature { element: el, feature: f, name: name.into() })
}

/// One form's Part Studio features for these variables and this sheet thickness.
pub fn studio(form: LibraryForm, vars: &[FormVariable], thickness: f64) -> Result<Vec<Feature>, CommandError> {
    let mut doc = Document::empty(LIBRARY_NAME);
    let mut el = Element::part_studio(form.label());
    el.id = studio_id(form);
    doc.elements.push(el);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h), studio_id(form), form, vars, thickness)?;
    Ok(doc.element(studio_id(form)).map(|e| e.features().to_vec()).unwrap_or_default())
}

/// The library as a document: one Part Studio per form, at its default sizes and 1 mm thick.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(LIBRARY_NAME);
    doc.id = DOCUMENT;
    let mut h = History::default();
    for form in LibraryForm::ALL {
        let mut el = Element::part_studio(form.label());
        el.id = studio_id(form);
        doc.elements.push(el);
        build_in(&mut DocHistory(&mut doc, &mut h), studio_id(form), form, &form.variables(), 1.0)?;
    }
    Ok(doc)
}

/// The library as a document file.
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}
