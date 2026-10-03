//! Drawings of assemblies (P3C.5, D1.2, D11, D12, D14.7, D14.8, X10): what a drawing keeps of an
//! assembly, the projection of its views, and its BOM tables' rows.
//!
//! - **The state a drawing keeps** ([`AssemblyState`], written as RON in the drawing's
//!   [`cadrs_drawing::ModelSource`] like a Part Studio's, P3C.6): every part occurrence (at any
//!   depth, suppressed instances left out) with its source part and placement, and the state of
//!   every Part Studio (standard content included) they come from. Views of the assembly are
//!   projected from it, so editing the assembly or one of its studios changes nothing on the
//!   sheet until the drawing is updated. Beside it the source carries a
//!   [`cadrs_drawing::assembly::AssemblyInfo`]: each occurrence's name, origin and part
//!   properties, which callouts read.
//! - **The dependency hash** ([`assembly_hash`]) covers the occurrences (sources, placements,
//!   hidden), the dependency hash of each part they show (its edges, faces, holes and
//!   appearance, [`crate::drawing_source::part_hash`]), their properties and the assembly's BOM
//!   settings: a change to any of them marks the assembly's views and BOM tables out of date;
//!   a change in a studio the assembly doesn't use marks nothing.
//! - **Projection** ([`request`]): on the rebuild worker each studio is rebuilt (cached), a
//!   moved copy of each occurrence's body made with the kernel, and all of them projected
//!   together by the hidden-line removal, so parts hide each other; the shaded view uses each
//!   studio's appearances. Each projected edge's body index is the occurrence's place in
//!   [`crate::views::ViewGeometry::parts`] (whose feature id is the occurrence's id).
//! - **BOM rows** ([`bom_data`]): the assembly's own BOM (`assembly::bom::compute_with`) with its
//!   columns, for the BOM type the drawing asks for; the Item column reads "Item No." as the
//!   course's tables do.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_drawing::ModelSource;
use cadrs_drawing::assembly::{AssemblyInfo, BomData, BomOrder, BomRowData, BomType, OccurrenceInfo};
use serde::{Deserialize, Serialize};

use crate::assembly::bom::{BomColumn, BomOptions, BomView};
use crate::assembly::{InstanceId, InstanceSource, Pose};
use crate::command::CommandError;
use crate::document::Document;
use crate::drawing_source::StudioState;
use crate::ids::{ElementId, FeatureId, PartId};
use crate::properties::{PropertyKey, PropertyOwner};
use crate::rebuild::Build;

/// One part occurrence of an assembly as a drawing keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OccState {
    /// The occurrence's id (see [`crate::assembly::structure::Occurrence::id`]).
    pub id: InstanceId,
    pub element: ElementId,
    pub part: PartId,
    pub pose: Pose,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Its `<n>`.
    #[serde(default)]
    pub index: u32,
}

impl OccState {
    /// Its part in an assembly view's geometry: the occurrence's id as the feature.
    pub fn view_part(&self) -> PartId {
        PartId::new(FeatureId(self.id.0), 0)
    }
}

/// An assembly's state as a drawing keeps it (see the module docs).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssemblyState {
    pub occurrences: Vec<OccState>,
    /// The Part Studios the occurrences come from, each once.
    pub studios: Vec<(ElementId, StudioState)>,
}

impl AssemblyState {
    /// The workspace state of assembly `element` of `doc`.
    pub fn of(doc: &Document, element: ElementId) -> Option<Self> {
        let asm = doc.element(element)?.assembly_model()?;
        let mut out = AssemblyState::default();
        for o in crate::assembly::structure::occurrences(doc, asm) {
            if !out.studios.iter().any(|(e, _)| *e == o.element)
                && let Some(st) = doc.element(o.element).and_then(StudioState::of)
            {
                out.studios.push((o.element, st));
            }
            out.occurrences.push(OccState { id: o.id, element: o.element, part: o.part, pose: o.pose, hidden: o.hidden, index: o.index });
        }
        Some(out)
    }

    pub fn to_snapshot(&self) -> String {
        ron::to_string(self).unwrap_or_default()
    }

    pub fn parse(snapshot: &str) -> Option<Self> {
        ron::from_str(snapshot).ok()
    }

    pub fn studio(&self, element: ElementId) -> Option<&StudioState> {
        self.studios.iter().find(|(e, _)| *e == element).map(|(_, s)| s)
    }
}

/// Whether `element` of `doc` is an Assembly tab.
pub fn is_assembly(doc: &Document, element: ElementId) -> bool {
    doc.element(element).is_some_and(|e| e.assembly_model().is_some())
}

/// The properties callouts read, by label.
const PROPS: [(PropertyKey, &str); 6] = [
    (PropertyKey::Name, "Name"),
    (PropertyKey::PartNumber, "Part number"),
    (PropertyKey::Description, "Description"),
    (PropertyKey::Material, "Material"),
    (PropertyKey::Revision, "Revision"),
    (PropertyKey::Vendor, "Vendor"),
];

/// The occurrences' names, origins and part properties (callouts' `Part:` fields).
pub fn assembly_info(doc: &Document, state: &AssemblyState, builds: &HashMap<ElementId, Arc<Build>>) -> AssemblyInfo {
    let occurrences = state
        .occurrences
        .iter()
        .map(|o| {
            let owner = PropertyOwner::Part { element: o.element, part: o.part };
            let build = builds.get(&o.element).map(|b| &**b);
            let props = PROPS
                .iter()
                .map(|(k, label)| (label.to_string(), crate::properties::text(doc, owner, *k, build)))
                .filter(|(_, v)| !v.is_empty())
                .collect();
            let part_name = crate::assembly::source_part_name(doc, &InstanceSource::Part { element: o.element, part: o.part }, build);
            OccurrenceInfo { id: o.id.0, name: format!("{part_name} <{}>", o.index), origin: o.pose.translation, props }
        })
        .collect();
    AssemblyInfo { occurrences }
}

fn push_f(bytes: &mut Vec<u8>, v: f64) {
    bytes.extend_from_slice(&((v * 1e4).round() as i64).to_le_bytes());
}

/// The dependency hash of an assembly view (see the module docs).
pub fn assembly_hash(doc: &Document, element: ElementId, state: &AssemblyState, info: &AssemblyInfo, builds: &HashMap<ElementId, Arc<Build>>) -> u64 {
    let mut bytes = Vec::new();
    let mut seen: Vec<(ElementId, PartId)> = Vec::new();
    for o in &state.occurrences {
        bytes.extend_from_slice(format!("{:?}{:?}{:?}{}", o.id, o.element, o.part, o.hidden).as_bytes());
        for row in &o.pose.rotation {
            for v in row {
                push_f(&mut bytes, *v);
            }
        }
        for v in o.pose.translation {
            push_f(&mut bytes, v);
        }
        if !seen.contains(&(o.element, o.part)) {
            seen.push((o.element, o.part));
            let h = match (builds.get(&o.element), state.studio(o.element)) {
                (Some(b), Some(st)) => crate::drawing_source::part_hash(&b.parts, st, Some(o.part)),
                _ => None,
            };
            bytes.extend_from_slice(&h.unwrap_or(crate::drawing_source::GONE).to_le_bytes());
        }
    }
    bytes.extend_from_slice(format!("{info:?}").as_bytes());
    if let Some(a) = doc.element(element).and_then(|e| e.assembly_model()) {
        bytes.extend_from_slice(format!("{:?}{:?}", a.bom, a.properties).as_bytes());
    }
    cadrs_kernel::naming::stable_hash(&bytes)
}

/// The drawing source of assembly `element` in state `state` (the studios' `builds` given).
pub fn source_of(doc: &Document, element: ElementId, state: &AssemblyState, builds: &HashMap<ElementId, Arc<Build>>) -> ModelSource {
    let info = assembly_info(doc, state, builds);
    let hash = assembly_hash(doc, element, state, &info, builds);
    ModelSource {
        element: element.0,
        snapshot: state.to_snapshot(),
        parts: vec![cadrs_drawing::PartHash { part: None, hash }],
        assembly: Some(info),
        pinned: false,
    }
}

/// Each studio of `state` rebuilt (waits; cached features are free).
pub fn builds(state: &AssemblyState) -> HashMap<ElementId, Arc<Build>> {
    state.studios.iter().map(|(e, st)| (*e, crate::rebuild::build(&st.features))).collect()
}

/// The workspace state of assembly `element` as a drawing source (rebuilds its studios on the
/// worker thread and waits).
pub fn live_source(doc: &Document, element: ElementId) -> Option<ModelSource> {
    let state = AssemblyState::of(doc, element)?;
    let b = builds(&state);
    Some(source_of(doc, element, &state, &b))
}

/// The request that projects assembly view `v` (hidden lines computed; shaded triangles if the
/// view is shaded). Sections and broken-out sections of assemblies are not supported: no cut.
pub fn view_request(v: &cadrs_drawing::View) -> crate::views::ViewRequest {
    crate::views::ViewRequest {
        part: None,
        frame: v.frame.view_frame(),
        options: cadrs_kernel::ProjectOptions { tolerance: 0.01, hidden: true },
        shaded: v.shaded,
        props: Vec::new(),
        appearances: Vec::new(),
        cut: None,
        intersections: v.part_intersections,
        flat: false,
    }
}

/// Starts projecting assembly view `v` from `state` on the worker thread.
pub fn request(state: AssemblyState, v: &cadrs_drawing::View) -> crate::views::PendingView {
    crate::views::request_assembly(state, view_request(v))
}

/// Projects and waits (tests, export).
pub fn project(state: &AssemblyState, v: &cadrs_drawing::View) -> Result<Arc<crate::views::ViewGeometry>, String> {
    request(state.clone(), v).wait()
}

/// The occurrence a projected edge of an assembly view belongs to.
pub fn edge_occurrence(g: &crate::views::ViewGeometry, edge: usize) -> Option<InstanceId> {
    let src = g.projection.edges.get(edge)?.source.as_ref()?;
    g.parts.get(src.body).map(|p| InstanceId(p.feature.0))
}

/// The rows of a drawing's BOM table of assembly `element` (D11.1): the assembly's BOM with its
/// own columns, as `kind` asks (Flattened; Structured, top level only or every level
/// expanded). `build_of` gives each studio's rebuild; `hash` is the assembly's dependency hash
/// the rows are of.
pub fn bom_data(
    doc: &Document,
    element: ElementId,
    kind: BomType,
    order: BomOrder,
    hash: u64,
    build_of: &mut dyn FnMut(ElementId) -> Option<Arc<Build>>,
) -> Result<BomData, CommandError> {
    let asm = doc.element(element).and_then(|e| e.assembly_model()).ok_or(CommandError::ElementNotFound(element))?;
    let mut settings = asm.bom.clone();
    settings.view = if kind == BomType::Flattened { BomView::Flattened } else { BomView::Structured };
    let options = BomOptions { expand_all: kind == BomType::MultiLevel, ..BomOptions::default() };
    let bom = crate::assembly::bom::compute_with(doc, element, &settings, &options, &doc.units, build_of)?;
    let columns = bom
        .columns
        .iter()
        .zip(&bom.labels)
        .map(|(c, l)| if *c == BomColumn::Item { "Item No.".to_string() } else { l.clone() })
        .collect();
    let rows = bom
        .rows
        .iter()
        .map(|r| BomRowData {
            item: r.item.clone().unwrap_or_default(),
            depth: r.depth,
            quantity: r.quantity,
            occurrences: r.occurrences.iter().map(|o| o.0).collect(),
            owner: owner_key(&r.key.owner),
            cells: r.cells.clone(),
        })
        .collect();
    Ok(BomData { assembly: element.0, kind, order, columns, rows, source_hash: hash })
}

/// How a BOM row names its part or assembly: `part:<studio uuid>:<feature uuid>:<index>` or
/// `assembly:<uuid>`.
pub fn owner_key(o: &PropertyOwner) -> String {
    match o {
        PropertyOwner::Part { element, part } => format!("part:{}:{}:{}", element.0, part.feature.0, part.index),
        PropertyOwner::Assembly { element } => format!("assembly:{}", element.0),
        PropertyOwner::Item { element, item } => format!("item:{}:{}", element.0, item.0),
    }
}

/// The owner of a BOM row's [`owner_key`].
pub fn owner_of(key: &str) -> Option<PropertyOwner> {
    let mut it = key.split(':');
    match it.next()? {
        "part" => {
            let element = ElementId(it.next()?.parse().ok()?);
            let feature = FeatureId(it.next()?.parse().ok()?);
            let index = it.next()?.parse().ok()?;
            Some(PropertyOwner::Part { element, part: PartId::new(feature, index) })
        }
        "assembly" => Some(PropertyOwner::Assembly { element: ElementId(it.next()?.parse().ok()?) }),
        "item" => {
            let element = ElementId(it.next()?.parse().ok()?);
            let item = crate::assembly::items::ItemId(it.next()?.parse().ok()?);
            Some(PropertyOwner::Item { element, item })
        }
        _ => None,
    }
}

/// The rows of a BOM table of `element` as the workspace has them now (rebuilds and waits).
pub fn live_bom(doc: &Document, element: ElementId, kind: BomType, order: BomOrder) -> Result<BomData, CommandError> {
    let src = live_source(doc, element).ok_or(CommandError::ElementNotFound(element))?;
    let hash = src.hash_of(None).unwrap_or(0);
    let mut cache: HashMap<ElementId, Arc<Build>> = HashMap::new();
    bom_data(doc, element, kind, order, hash, &mut |e| {
        if let Some(b) = cache.get(&e) {
            return Some(b.clone());
        }
        let b = crate::rebuild::build(doc.element(e)?.features());
        cache.insert(e, b.clone());
        Some(b)
    })
}
