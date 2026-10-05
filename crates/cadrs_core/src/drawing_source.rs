//! What a drawing's views show (P3C.6, D13): the referenced Part Studio's state as of the
//! drawing's last update, and the dependency hash of each of its parts.
//!
//! A drawing keeps a snapshot of each studio its views reference ([`cadrs_drawing::ModelSource`]):
//! the studio's features, part settings and feature appearances ([`StudioState`], written as
//! RON). Views are projected from the snapshot, so editing the studio changes nothing on the
//! sheet until the drawing is updated.
//!
//! A part's **dependency hash** ([`part_hash`]) is a stable hash of what its views show: its
//! edges (persistent names and exact geometry, rounded to 0.1 µm), its faces' names, the specs
//! of the Hole features that made it (hole callouts read them) and its appearance (shaded
//! views). Editing a feature that doesn't change a part (a sketch of another part, another
//! studio) leaves its hash alone, so only the views of the parts that changed go out of date.

use cadrs_drawing::{ModelSource, PartHash};
use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use crate::document::{Document, Element, ElementKind, Feature, PartProps};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::parts::Part;
use crate::rebuild::Build;

/// A Part Studio's state as a drawing keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StudioState {
    /// The features that build (suppressed ones left out).
    pub features: Vec<Feature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<PartProps>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub appearances: Vec<(FeatureId, Appearance)>,
}

impl StudioState {
    /// The workspace state of a Part Studio.
    pub fn of(el: &Element) -> Option<Self> {
        if !matches!(el.kind, ElementKind::PartStudio { .. }) {
            return None;
        }
        let off = el.all_suppressed();
        Some(Self {
            features: el.features().iter().filter(|f| !off.contains(&f.id)).cloned().collect(),
            props: el.part_props().to_vec(),
            appearances: el.feature_appearances().to_vec(),
        })
    }

    /// As a drawing stores it.
    pub fn to_snapshot(&self) -> String {
        ron::to_string(self).unwrap_or_default()
    }

    /// A stored snapshot.
    pub fn parse(snapshot: &str) -> Option<Self> {
        ron::from_str(snapshot).ok()
    }
}

/// A part key as drawings store it.
pub fn part_key(p: Option<PartId>) -> Option<(uuid::Uuid, u32)> {
    p.map(|p| (p.feature.0, p.index))
}

fn push_f(bytes: &mut Vec<u8>, v: f64) {
    // 0.1 µm: far below anything a drawing shows, far above rebuild noise.
    bytes.extend_from_slice(&((v * 1e4).round() as i64).to_le_bytes());
}

fn push_p(bytes: &mut Vec<u8>, p: [f64; 3]) {
    for v in p {
        push_f(bytes, v);
    }
}

/// The dependency hash of `part` (every solid part when `None`) in a build of `state` (see the
/// module docs); `None` when the part isn't there.
pub fn part_hash(parts: &[Part], state: &StudioState, part: Option<PartId>) -> Option<u64> {
    let shown = crate::views::view_parts(parts, part);
    if shown.is_empty() {
        return None;
    }
    let mut bytes = Vec::new();
    for p in shown {
        bytes.extend_from_slice(format!("{:?}", p.id).as_bytes());
        for e in &p.solid.edges {
            bytes.extend_from_slice(format!("{:?}", e.name).as_bytes());
            match (e.circle, crate::views::model_edge(e)) {
                (Some(c), _) => {
                    push_p(&mut bytes, c.center);
                    push_p(&mut bytes, c.normal);
                    push_f(&mut bytes, c.radius);
                    if let (Some(a), Some(b)) = (e.points.first(), e.points.last()) {
                        push_p(&mut bytes, *a);
                        push_p(&mut bytes, *b);
                    }
                }
                (None, cadrs_drawing::annotation::ModelEdge::Line { a, b }) => {
                    push_p(&mut bytes, a);
                    push_p(&mut bytes, b);
                }
                (None, _) => {
                    bytes.extend_from_slice(&(e.points.len() as u64).to_le_bytes());
                    for q in &e.points {
                        push_p(&mut bytes, *q);
                    }
                }
            }
        }
        for f in &p.solid.faces {
            bytes.extend_from_slice(format!("{:?}", f.name).as_bytes());
        }
        // The holes that made it (callouts), and its appearance (shaded views).
        for f in &state.features {
            if p.features.contains(&f.id)
                && let Some(h) = f.hole()
            {
                bytes.extend_from_slice(format!("{:?}", crate::views::hole_info(&h.spec)).as_bytes());
            }
            if p.features.contains(&f.id)
                && let Some((_, a)) = state.appearances.iter().find(|(id, _)| *id == f.id)
            {
                bytes.extend_from_slice(format!("{a:?}").as_bytes());
            }
        }
        if let Some(pp) = state.props.iter().find(|q| q.part == p.id) {
            bytes.extend_from_slice(format!("{:?}{:?}", pp.appearance, pp.faces).as_bytes());
        }
    }
    Some(cadrs_kernel::naming::stable_hash(&bytes))
}

/// The hashes of every part of a build, and of the whole studio.
pub fn part_hashes(build: &Build, state: &StudioState) -> Vec<PartHash> {
    let mut out: Vec<PartHash> = build
        .parts
        .iter()
        .filter(|p| p.kind == crate::parts::PartKind::Solid)
        .filter_map(|p| part_hash(&build.parts, state, Some(p.id)).map(|hash| PartHash { part: part_key(Some(p.id)), hash }))
        .collect();
    if let Some(hash) = part_hash(&build.parts, state, None) {
        out.push(PartHash { part: None, hash });
    }
    out
}

/// A drawing source from a studio's state and its build.
pub fn source_of(element: ElementId, state: &StudioState, build: &Build) -> ModelSource {
    ModelSource { element: element.0, snapshot: state.to_snapshot(), parts: part_hashes(build, state), assembly: None, pinned: false }
}

/// The workspace state of Part Studio `element` of `doc` as a drawing source (rebuilds it on
/// the worker thread and waits: cached features are free).
pub fn live_source(doc: &Document, element: ElementId) -> Option<ModelSource> {
    // An assembly's (P3C.5).
    if crate::drawing_assembly::is_assembly(doc, element) {
        return crate::drawing_assembly::live_source(doc, element);
    }
    let state = StudioState::of(doc.element(element)?)?;
    let build = crate::rebuild::build(&state.features);
    Some(source_of(element, &state, &build))
}

/// The hash a drawing stores for a part that is gone from the workspace (never a real hash's
/// value in practice): its views are out of date.
pub const GONE: u64 = 0;

/// The workspace's dependency hash of what a view referencing `r` shows ([`GONE`] when the part
/// no longer exists), rebuilding the studio on the worker thread and waiting.
pub fn live_hash(doc: &Document, r: &cadrs_drawing::ObjectRef) -> Option<u64> {
    let src = live_source(doc, ElementId(r.element))?;
    Some(src.hash_of(r.part).unwrap_or(GONE))
}

/// The views of drawing `d` that are out of date with the workspace of `doc` (waits for the
/// rebuilds; the app computes the same asynchronously).
pub fn out_of_date(doc: &Document, d: &cadrs_drawing::Drawing) -> Vec<cadrs_drawing::ViewId> {
    let mut cache: std::collections::HashMap<uuid::Uuid, Option<ModelSource>> = Default::default();
    let mut live = |r: &cadrs_drawing::ObjectRef| -> Option<u64> {
        let src = cache.entry(r.element).or_insert_with(|| live_source(doc, ElementId(r.element)));
        src.as_ref().map(|s| s.hash_of(r.part).unwrap_or(GONE))
    };
    let hashes: Vec<(cadrs_drawing::ObjectRef, Option<u64>)> = d
        .sheets
        .iter()
        .flat_map(|s| s.views.iter())
        .map(|v| (v.reference, live(&v.reference)))
        .collect();
    d.out_of_date(&|r| hashes.iter().find(|(q, _)| q == r).and_then(|(_, h)| *h))
}

/// The request that projects view `v` (hidden lines computed; shaded triangles if the view is
/// shaded) from `state`.
pub fn view_request(state: &StudioState, v: &cadrs_drawing::View) -> crate::views::ViewRequest {
    crate::views::ViewRequest {
        part: v.reference.part.map(|(f, index)| PartId { feature: FeatureId(f), index }),
        frame: v.frame.view_frame(),
        options: cadrs_kernel::ProjectOptions { tolerance: 0.01, hidden: true },
        shaded: v.shaded,
        props: state.props.clone(),
        appearances: state.appearances.clone(),
        cut: v.effective_cut(),
        intersections: false,
        flat: v.flat.is_some(),
    }
}

/// Brings every out-of-date view of drawing `d` up to date with the workspace of `doc`, as one
/// [`cadrs_drawing::DrawingOp::Update`] (waits for the projections; the app makes the same op
/// from its view cache). `None` when nothing is out of date.
pub fn update_now(doc: &Document, d: &cadrs_drawing::Drawing) -> Option<cadrs_drawing::DrawingOp> {
    use cadrs_drawing::annotation::ViewModel;
    let stale = out_of_date(doc, d);
    if stale.is_empty() {
        return None;
    }
    let mut sources = Vec::new();
    let mut models = Vec::new();
    for id in &stale {
        let (_, v) = d.view(*id)?;
        let element = ElementId(v.reference.element);
        if !sources.iter().any(|s: &ModelSource| s.element == element.0) {
            sources.push(live_source(doc, element)?);
        }
        // An assembly view (P3C.5): projected from the assembly's old and new states.
        if crate::drawing_assembly::is_assembly(doc, element) {
            use crate::drawing_assembly::{AssemblyState, project};
            let old = d.source(element.0).and_then(|s| AssemblyState::parse(&s.snapshot)).and_then(|st| project(&st, v).ok());
            let new = project(&AssemblyState::of(doc, element)?, v).ok()?;
            models.push((*id, old, new));
            continue;
        }
        let live = StudioState::of(doc.element(element)?)?;
        let old = d
            .source(element.0)
            .and_then(|s| StudioState::parse(&s.snapshot))
            .and_then(|st| crate::views::project(&st.features, view_request(&st, v)).ok());
        let new = crate::views::project(&live.features, view_request(&live, v)).ok()?;
        models.push((*id, old, new));
    }
    let refs: Vec<(cadrs_drawing::ViewId, Option<&dyn ViewModel>, &dyn ViewModel)> = models
        .iter()
        .map(|(id, old, new)| (*id, old.as_deref().map(|g| g as &dyn ViewModel), &**new as &dyn ViewModel))
        .collect();
    let op = cadrs_drawing::update::update_op(d, sources, &refs);
    Some(with_bom_tables(doc, d, op))
}

/// An update `op` together with the BOM tables of the assemblies it updates (P3C.5, D14.8):
/// each out-of-date BOM table gets the workspace's rows, in one undoable step.
pub fn with_bom_tables(doc: &Document, d: &cadrs_drawing::Drawing, op: cadrs_drawing::DrawingOp) -> cadrs_drawing::DrawingOp {
    use cadrs_drawing::DrawingOp;
    let tables = bom_updates(doc, d);
    if tables.is_empty() {
        return op;
    }
    let label = op.label();
    let mut ops = vec![op];
    ops.extend(tables.into_iter().map(|(sheet, table)| DrawingOp::SetTable { sheet, table, label: label.clone() }));
    DrawingOp::Batch { ops, label }
}

/// The out-of-date BOM tables of `d` with the workspace's rows (waits for the rebuilds).
pub fn bom_updates(doc: &Document, d: &cadrs_drawing::Drawing) -> Vec<(cadrs_drawing::SheetId, cadrs_drawing::Table)> {
    let mut hashes: std::collections::HashMap<uuid::Uuid, Option<u64>> = Default::default();
    let mut live = |r: &cadrs_drawing::ObjectRef| -> Option<u64> {
        *hashes.entry(r.element).or_insert_with(|| live_source(doc, ElementId(r.element)).and_then(|s| s.hash_of(None)))
    };
    let known: Vec<(uuid::Uuid, Option<u64>)> = d
        .sheets
        .iter()
        .flat_map(|s| s.tables.iter())
        .filter_map(|t| t.bom.as_ref().map(|b| b.assembly))
        .map(|a| (a, live(&cadrs_drawing::ObjectRef { element: a, part: None })))
        .collect();
    let stale = d.stale_boms(&|r| known.iter().find(|(a, _)| *a == r.element).and_then(|(_, h)| *h));
    let mut out = Vec::new();
    for (sheet, id) in stale {
        let Some(t) = d.sheet(sheet).and_then(|s| s.tables.iter().find(|t| t.id == id)) else { continue };
        let Some(b) = &t.bom else { continue };
        if let Ok(data) = crate::drawing_assembly::live_bom(doc, ElementId(b.assembly), b.kind, b.order) {
            out.push((sheet, cadrs_drawing::assembly::refreshed_bom_table(t, data, &d.style)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_round_trip() {
        let mut doc = Document::new("Doc");
        let el = doc.elements[0].id;
        let mut h = crate::History::default();
        crate::samples::drawing_bracket::build_in(&mut crate::samples::gear_cover::DocHistory(&mut doc, &mut h), el).unwrap();
        let s = StudioState::of(doc.element(el).unwrap()).unwrap();
        assert_eq!(StudioState::parse(&s.to_snapshot()), Some(s));
    }
}
