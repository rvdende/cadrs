//! Exporting drawings (P3C.7, D2.10 Export…, X13): a drawing's sheets as pages
//! ([`cadrs_drawing::export::Page`]) with every view projected, for the PDF, DXF, DWG and raster
//! writers of `cadrs_drawing`.
//!
//! A view shows the studio state its drawing keeps (P3C.6), or the workspace when the drawing
//! has none; views the caller has no geometry for are projected here (synchronously).

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_drawing::export::{Page, PageContext, ShadedTri, ViewInput, sheet_page};
use cadrs_drawing::rich::DrawingContext;
use cadrs_drawing::{Drawing, ObjectRef, ReferenceProps, View};

use crate::document::{Document, ElementKind};
use crate::drawing_source::{StudioState, view_request};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::views::ViewGeometry;

/// The part or assembly properties a sheet's title block and parametric notes read (D2.8, D9.4,
/// X3), from the property model (P3B.6, [`crate::properties`]; P3C.5): a part's Name (its
/// rename, else its studio's name), Part number, Description, Revision, Material and Vendor; an
/// assembly's (its tab name and its own properties); a whole Part Studio's name only. Undefined
/// properties are `None` (the title block shows dashes).
pub fn reference_props(doc: &Document, r: Option<ObjectRef>) -> ReferenceProps {
    use crate::properties::{PropertyKey, PropertyOwner, text};
    let Some(r) = r else {
        return ReferenceProps::default();
    };
    let Some(el) = doc.element(ElementId(r.element)) else {
        return ReferenceProps::default();
    };
    let mut props = ReferenceProps { name: Some(el.name.clone()), ..Default::default() };
    let owner = match r.part {
        Some((f, index)) => {
            let part = PartId { feature: FeatureId(f), index };
            if let Some(n) = el.part_prop(part).and_then(|p| p.name.clone()) {
                props.name = Some(n);
            }
            PropertyOwner::Part { element: el.id, part }
        }
        None if el.assembly_model().is_some() => PropertyOwner::Assembly { element: el.id },
        None => return props,
    };
    let get = |k: PropertyKey| Some(text(doc, owner, k, None)).filter(|v| !v.trim().is_empty());
    props.part_number = get(PropertyKey::PartNumber);
    props.description = get(PropertyKey::Description);
    props.revision = get(PropertyKey::Revision);
    props.material = get(PropertyKey::Material);
    props.vendor = get(PropertyKey::Vendor);
    props
}

/// What fields on sheet `index` read (D9.4).
pub fn drawing_context(drawing_name: &str, d: &Drawing, index: usize, date: Option<(i32, u32, u32)>) -> DrawingContext {
    let s = &d.sheets[index.min(d.sheets.len().saturating_sub(1))];
    DrawingContext {
        drawing_name: drawing_name.to_string(),
        sheet_name: s.name.clone(),
        sheet_index: index,
        sheet_count: d.sheets.len(),
        scale: s.scale,
        size: s.format.size,
        projection: d.projection,
        units: d.units,
        date,
        date_format: d.style.date_format,
        title: d.title.clone(),
    }
}

/// The studio state view `v` of `d` shows: the drawing's snapshot, else the workspace.
pub fn shown_state(doc: &Document, d: &Drawing, v: &View) -> Option<StudioState> {
    if let Some(src) = d.source(v.reference.element) {
        return StudioState::parse(&src.snapshot);
    }
    let el = doc.element(ElementId(v.reference.element))?;
    matches!(el.kind, ElementKind::PartStudio { .. }).then(|| StudioState::of(el)).flatten()
}

/// Projects view `v` of `d` (waits).
pub fn view_geometry(doc: &Document, d: &Drawing, v: &View) -> Result<Arc<ViewGeometry>, String> {
    // An assembly view (P3C.5): the drawing's state of the assembly, else the workspace's.
    if crate::drawing_assembly::is_assembly(doc, ElementId(v.reference.element)) {
        use crate::drawing_assembly::{AssemblyState, project};
        let state = match d.source(v.reference.element) {
            Some(s) => AssemblyState::parse(&s.snapshot),
            None => AssemblyState::of(doc, ElementId(v.reference.element)),
        }
        .ok_or("the view's assembly is gone")?;
        return project(&state, v);
    }
    let state = shown_state(doc, d, v).ok_or("the view's Part Studio is gone")?;
    crate::views::project(&state.features, view_request(&state, v))
}

/// The sketches view `v` shows (D7.4), as sheet polylines.
pub fn sketch_polylines(state: &StudioState, v: &View) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    if v.sketches.is_empty() {
        return out;
    }
    let vf = v.frame.view_frame();
    for f in &state.features {
        if !v.sketches.contains(&f.id.0) {
            continue;
        }
        let Some(sk) = f.sketch() else { continue };
        let Some(plane) = sk.plane else { continue };
        let frame = plane.frame();
        for (id, c) in sk.geometry.curves.iter() {
            if c.construction {
                continue;
            }
            let pts = cadrs_sketch::hit::curve_polyline(&sk.geometry, id);
            out.push(
                pts.into_iter()
                    .map(|p| {
                        let w = frame.to_world(p);
                        let q = vf.to_2d(&nalgebra::Point3::new(w[0], w[1], w[2]));
                        v.to_sheet([q.x, q.y])
                    })
                    .collect(),
            );
        }
    }
    out
}

/// A view with its geometry and the sketches it shows.
type Shown = (View, Arc<ViewGeometry>, Vec<Vec<[f64; 2]>>);

/// The pages of sheets `sheets` of drawing `d` (named `name` in `doc`): each view's geometry
/// from `cached` when it has it, else projected now. Views that can't be projected are left
/// out.
pub fn pages(
    doc: &Document,
    name: &str,
    d: &Drawing,
    sheets: &[usize],
    date: Option<(i32, u32, u32)>,
    cached: &dyn Fn(&View) -> Option<Arc<ViewGeometry>>,
) -> Vec<Page> {
    let mut out = Vec::new();
    for &i in sheets {
        let Some(sheet) = d.sheets.get(i) else { continue };
        let mut geo: Vec<Shown> = Vec::new();
        for v in &sheet.views {
            let g = match cached(v) {
                Some(g) if !v.shaded || !g.shaded.is_empty() => Some(g),
                _ => view_geometry(doc, d, v).ok(),
            };
            let Some(g) = g else { continue };
            let sketches = shown_state(doc, d, v).map(|s| sketch_polylines(&s, v)).unwrap_or_default();
            geo.push((v.clone(), g, sketches));
        }
        let mut views = HashMap::new();
        for (v, g, sketches) in &geo {
            let shaded = if v.shaded {
                g.shaded
                    .iter()
                    .map(|t| ShadedTri { points: t.points, colors: t.colors.map(|c| [c[0], c[1], c[2]]) })
                    .collect()
            } else {
                Vec::new()
            };
            views.insert(v.id, ViewInput { model: &**g, shaded, sketches: sketches.clone() });
        }
        let reference = reference_props(doc, sheet.reference);
        let fields = drawing_context(name, d, i, date);
        let ctx = PageContext { reference: &reference, fields: &fields, views };
        out.push(sheet_page(d, i, &ctx));
    }
    out
}
