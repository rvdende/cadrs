//! The concrete document commands. Later milestones add sketch-editing commands here.

mod applied;
mod drawing;
pub use drawing::{EditDrawing, InsertElement};
pub use applied::{CreateFolder, MoveFeatures, SetFeature, SetFolder, UnpackFolder, normalize_folders};
mod list;
pub use list::{DeleteFolder, SetRollback, SetSuppressByVariable, SetSuppressed};

use cadrs_sketch::{CurveId, FaceName, PlaneRef, SketchOp, Vec2};

use crate::appearance::Appearance;
use crate::material::Material;

use crate::command::{Command, CommandError, Scope};
use crate::document::{
    BooleanFeature, DeletePartFeature, Document, Element, ElementKind, ExtrudeFeature, Feature,
    FeatureKind, PartProps, RevolveFeature, SketchFeature,
};
use crate::ids::{ElementId, FeatureId, PartId};

fn non_empty(name: &str) -> Result<String, CommandError> {
    let name = name.trim();
    if name.is_empty() {
        Err(CommandError::Invalid("name must not be empty".into()))
    } else {
        Ok(name.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct RenameDocument {
    pub name: String,
}

impl Command for RenameDocument {
    fn label(&self) -> String {
        "Rename document".into()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        doc.name = non_empty(&self.name)?;
        Ok(())
    }
}

/// Changes the document's workspace units (X1). Only how values are shown and typed changes;
/// the geometry stays as it is.
#[derive(Debug, Clone)]
pub struct SetUnits {
    pub units: cadrs_sketch::units::Units,
}

impl Command for SetUnits {
    fn label(&self) -> String {
        "Change workspace units".into()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        doc.units = self.units;
        Ok(())
    }
}

/// Which kind of element [`AddElement`] creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewElementKind {
    PartStudio,
    Assembly,
    /// P3H.3: "PCB Studio 1", with no boards.
    PcbStudio,
    /// P3F.6: a Render Studio of this Part Studio or Assembly.
    RenderStudio(Option<ElementId>),
}

/// Adds a new tab. `id` is chosen by the caller so it can select the new tab afterwards.
#[derive(Debug, Clone)]
pub struct AddElement {
    pub id: ElementId,
    pub kind: NewElementKind,
    /// `None` picks the next default name ("Part Studio 2").
    pub name: Option<String>,
    /// Insert directly right of this tab (Onshape inserts right of the active tab); `None` or
    /// an unknown id appends at the end.
    pub after: Option<ElementId>,
}

impl Command for AddElement {
    fn label(&self) -> String {
        match self.kind {
            NewElementKind::PartStudio => "Create Part Studio".into(),
            NewElementKind::Assembly => "Create Assembly".into(),
            NewElementKind::PcbStudio => "Create PCB Studio".into(),
            NewElementKind::RenderStudio(_) => "Create Render Studio".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if doc.element(self.id).is_some() {
            return Err(CommandError::Invalid("element id already in use".into()));
        }
        let base = match self.kind {
            NewElementKind::PartStudio => "Part Studio",
            NewElementKind::Assembly => "Assembly",
            NewElementKind::PcbStudio => "PCB Studio",
            NewElementKind::RenderStudio(_) => "Render Studio",
        };
        let name = match &self.name {
            Some(n) => non_empty(n)?,
            None => doc.next_element_name(base),
        };
        let mut element = match self.kind {
            NewElementKind::PartStudio => Element::part_studio(name),
            NewElementKind::Assembly => Element::assembly(name),
            NewElementKind::PcbStudio => Element::pcb_studio(name),
            NewElementKind::RenderStudio(source) => {
                if let Some(s) = source
                    && !doc.element(s).is_some_and(|e| matches!(e.kind, ElementKind::PartStudio { .. } | ElementKind::Assembly))
                {
                    return Err(CommandError::Invalid("a render needs a Part Studio or an Assembly".into()));
                }
                Element::render_studio(name, source)
            }
        };
        element.id = self.id;
        insert_after(doc, self.after, element);
        Ok(())
    }
}

fn insert_after(doc: &mut Document, after: Option<ElementId>, element: Element) {
    match after.and_then(|a| doc.element_index(a)) {
        Some(i) => doc.elements.insert(i + 1, element),
        None => doc.elements.push(element),
    }
}

/// Copies a tab and inserts the copy directly right of it, named "<name> (1)" (the next free
/// number). The copy is independent of the original.
#[derive(Debug, Clone)]
pub struct DuplicateElement {
    pub source: ElementId,
    /// The copy's id, chosen by the caller so it can select it.
    pub id: ElementId,
}

impl Command for DuplicateElement {
    fn label(&self) -> String {
        "Duplicate tab".into()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if doc.element(self.id).is_some() {
            return Err(CommandError::Invalid("element id already in use".into()));
        }
        let source = doc
            .element(self.source)
            .ok_or(CommandError::ElementNotFound(self.source))?;
        let mut copy = source.clone();
        copy.id = self.id;
        let mut n = 1;
        copy.name = loop {
            let name = format!("{} ({n})", source.name);
            if !doc.elements.iter().any(|e| e.name == name) {
                break name;
            }
            n += 1;
        };
        // Features keep their ids: they are unique within an element, not across elements.
        insert_after(doc, Some(self.source), copy);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RenameElement {
    pub id: ElementId,
    pub name: String,
}

impl Command for RenameElement {
    fn label(&self) -> String {
        format!("Rename tab to {}", self.name.trim())
    }
    fn scope(&self) -> Scope {
        Scope::ElementName(self.id)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = non_empty(&self.name)?;
        let el = doc
            .element_mut(self.id)
            .ok_or(CommandError::ElementNotFound(self.id))?;
        el.name = name;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DeleteElement {
    pub id: ElementId,
}

impl Command for DeleteElement {
    fn label(&self) -> String {
        "Delete tab".into()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let i = doc
            .element_index(self.id)
            .ok_or(CommandError::ElementNotFound(self.id))?;
        if doc.elements.len() == 1 {
            return Err(CommandError::Invalid("cannot delete the last tab".into()));
        }
        doc.elements.remove(i);
        Ok(())
    }
}

fn part_studio_features(
    doc: &mut Document,
    element: ElementId,
) -> Result<&mut Vec<Feature>, CommandError> {
    doc.element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?
        .features_mut()
        .ok_or_else(|| CommandError::Invalid("features need a Part Studio".into()))
}

/// Inserts a new feature at the rollback bar (P3.9: at the end while the bar is at the end), and
/// keeps the bar below it.
fn insert_feature(doc: &mut Document, element: ElementId, feature: Feature) -> Result<(), CommandError> {
    let el = doc.element_mut(element).ok_or(CommandError::ElementNotFound(element))?;
    let at = el.rollback_index();
    let ElementKind::PartStudio { features, rollback, .. } = &mut el.kind else {
        return Err(CommandError::Invalid("features need a Part Studio".into()));
    };
    features.insert(at, feature);
    if let Some(r) = rollback {
        *r = at + 1;
    }
    Ok(())
}

fn sketch_mut(
    doc: &mut Document,
    element: ElementId,
    feature: FeatureId,
) -> Result<&mut SketchFeature, CommandError> {
    part_studio_features(doc, element)?
        .iter_mut()
        .find(|f| f.id == feature)
        .and_then(|f| f.sketch_mut())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

/// Inserts a new sketch feature at the end of a Part Studio's feature list, named "Sketch N".
/// `plane` is `None` when the dialog opens without a preselected plane.
#[derive(Debug, Clone)]
pub struct AddSketch {
    pub element: ElementId,
    pub feature: FeatureId,
    pub plane: Option<PlaneRef>,
}

impl Command for AddSketch {
    fn label(&self) -> String {
        "Insert sketch".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if el.feature(self.feature).is_some() {
            return Err(CommandError::Invalid("feature id already in use".into()));
        }
        let name = el.next_feature_name("Sketch");
        insert_feature(doc, self.element, Feature {
            id: self.feature,
            name,
            kind: FeatureKind::Sketch(SketchFeature::new(self.plane)),
            suppress_by: None,
        })?;
        refresh(doc, self.element);
        Ok(())
    }
}

/// Sets (or clears, with `None`) a sketch's plane.
#[derive(Debug, Clone)]
pub struct SetSketchPlane {
    pub element: ElementId,
    pub feature: FeatureId,
    pub plane: Option<PlaneRef>,
}

impl Command for SetSketchPlane {
    fn label(&self) -> String {
        match self.plane {
            Some(p) => format!("Select {}", p.display_name()),
            None => "Clear sketch plane".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        sketch_mut(doc, self.element, self.feature)?.plane = self.plane;
        refresh(doc, self.element);
        Ok(())
    }
}

/// The sketch dialog's "Disable imprinting" option.
#[derive(Debug, Clone)]
pub struct SetSketchImprinting {
    pub element: ElementId,
    pub feature: FeatureId,
    pub disable_imprinting: bool,
}

impl Command for SetSketchImprinting {
    fn label(&self) -> String {
        "Disable imprinting".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        sketch_mut(doc, self.element, self.feature)?.disable_imprinting = self.disable_imprinting;
        refresh(doc, self.element);
        Ok(())
    }
}

/// Removes a feature from a Part Studio.
#[derive(Debug, Clone)]
pub struct DeleteFeature {
    pub element: ElementId,
    pub feature: FeatureId,
    /// Shown in the undo menu, e.g. "Cancel Sketch 2" or "Delete Sketch 1".
    pub label: String,
}

impl Command for DeleteFeature {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let features = part_studio_features(doc, self.element)?;
        let i = features
            .iter()
            .position(|f| f.id == self.feature)
            .ok_or_else(|| CommandError::Invalid("feature not found".into()))?;
        features.remove(i);
        if let Some(folders) = doc.element_mut(self.element).and_then(|e| e.folders_mut()) {
            for f in folders.iter_mut() {
                f.features.retain(|x| *x != self.feature);
            }
            // A folder left empty goes too.
            folders.retain(|f| !f.features.is_empty());
        }
        list::forget_removed(doc, self.element, &[i]);
        refresh(doc, self.element);
        Ok(())
    }
}

/// Moves a feature to another place in the feature list (`to`: its index afterwards). Onshape
/// lets features be dragged to any place their references allow; the check that a feature
/// stays after the features it uses comes with the drag (P3.6).
#[derive(Debug, Clone)]
pub struct MoveFeature {
    pub element: ElementId,
    pub feature: FeatureId,
    pub to: usize,
    pub label: String,
}

impl Command for MoveFeature {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let features = part_studio_features(doc, self.element)?;
        let i = features
            .iter()
            .position(|f| f.id == self.feature)
            .ok_or_else(|| CommandError::Invalid("feature not found".into()))?;
        if self.to >= features.len() {
            return Err(CommandError::Invalid("no such place in the feature list".into()));
        }
        let f = features.remove(i);
        features.insert(self.to, f);
        refresh(doc, self.element);
        Ok(())
    }
}

/// Puts a feature back the way it was (same id and position), as cancelling an edit does.
#[derive(Debug, Clone)]
pub struct ReplaceFeature {
    pub element: ElementId,
    pub feature: Feature,
    pub label: String,
}

impl Command for ReplaceFeature {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let f = part_studio_features(doc, self.element)?
            .iter_mut()
            .find(|f| f.id == self.feature.id)
            .ok_or_else(|| CommandError::Invalid("feature not found".into()))?;
        *f = self.feature.clone();
        refresh(doc, self.element);
        Ok(())
    }
}

/// Renames a feature.
#[derive(Debug, Clone)]
pub struct RenameFeature {
    pub element: ElementId,
    pub feature: FeatureId,
    pub name: String,
}

impl Command for RenameFeature {
    fn label(&self) -> String {
        format!("Rename feature to {}", self.name.trim())
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = non_empty(&self.name)?;
        let f = part_studio_features(doc, self.element)?
            .iter_mut()
            .find(|f| f.id == self.feature)
            .ok_or_else(|| CommandError::Invalid("feature not found".into()))?;
        // A hole keeps a name the user gives it instead of its callout.
        if let FeatureKind::Hole(h) = &mut f.kind {
            h.renamed = true;
        }
        f.name = name;
        Ok(())
    }
}

/// Adds a line to an existing sketch.
#[derive(Debug, Clone)]
pub struct AddSketchLine {
    pub element: ElementId,
    pub feature: FeatureId,
    pub a: Vec2,
    pub b: Vec2,
}

impl Command for AddSketchLine {
    fn label(&self) -> String {
        "Add line".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        sketch_mut(doc, self.element, self.feature)?
            .geometry
            .add_line(self.a, self.b);
        Ok(())
    }
}

/// Edits a sketch's geometry (see [`SketchOp`]): drawing, deleting, construction, dimensions.
#[derive(Debug, Clone)]
pub struct EditSketch {
    pub element: ElementId,
    pub feature: FeatureId,
    pub op: SketchOp,
}

impl Command for EditSketch {
    fn label(&self) -> String {
        self.op.label()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let sketch = sketch_mut(doc, self.element, self.feature)?;
        self.op
            .apply(&mut sketch.geometry)
            .map_err(CommandError::Invalid)?;
        refresh(doc, self.element);
        Ok(())
    }
}

/// Sketches on faces follow their faces after an edit that may have moved them.
fn refresh(doc: &mut Document, element: ElementId) {
    refresh_studio(doc, element);
}

/// Regenerates a Part Studio's sketches (faces, imprints, links), against its assembly context
/// too when it has one (P3B.9, [`crate::assembly::context`]).
pub fn refresh_studio(doc: &mut Document, element: ElementId) {
    // Derived features first: the sketches may sit on their parts.
    crate::derived::resolve_document(doc);
    let context = crate::assembly::context::solids(doc, element);
    let units = doc.units;
    // P3F.4: variables first, so the sketches regenerate with the dimensions they drive.
    let suppressed = doc.element(element).map(|e| e.suppressed().to_vec()).unwrap_or_default();
    if let Ok(features) = part_studio_features(doc, element) {
        crate::variables::refresh(features, &suppressed, &units);
        crate::parts::regenerate_with(features, &context);
    }
}

/// Inserts a new extrude at the end of a Part Studio's feature list, named "Extrude N".
#[derive(Debug, Clone)]
pub struct AddExtrude {
    pub element: ElementId,
    pub feature: FeatureId,
    pub extrude: ExtrudeFeature,
}

impl Command for AddExtrude {
    fn label(&self) -> String {
        "Insert extrude".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if el.feature(self.feature).is_some() {
            return Err(CommandError::Invalid("feature id already in use".into()));
        }
        let name = el.next_feature_name("Extrude");
        insert_feature(doc, self.element, Feature {
            id: self.feature,
            name,
            kind: FeatureKind::Extrude(self.extrude.clone()),
            suppress_by: None,
        })?;
        Ok(())
    }
}

/// Sets an extrude's parameters (the dialog's regions, depth, direction, …).
#[derive(Debug, Clone)]
pub struct SetExtrude {
    pub element: ElementId,
    pub feature: FeatureId,
    pub extrude: ExtrudeFeature,
    /// Shown in the undo menu ("Select Face of Sketch 1", "Depth", "Flip direction").
    pub label: String,
}

impl Command for SetExtrude {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.extrude.depth <= 0.0 || !self.extrude.depth.is_finite() {
            return Err(CommandError::Invalid("the depth must be positive".into()));
        }
        set_extrude_unchecked(doc, self.element, self.feature, &self.extrude)
    }
}

/// Sets an extrude's parameters without the depth check (the end type may make the depth
/// irrelevant): any change from the Extrude dialog.
fn set_extrude_unchecked(doc: &mut Document, element: ElementId, feature: FeatureId, e: &ExtrudeFeature) -> Result<(), CommandError> {
    *part_studio_features(doc, element)?
        .iter_mut()
        .find(|f| f.id == feature)
        .and_then(|f| f.extrude_mut())
        .ok_or_else(|| CommandError::Invalid("extrude not found".into()))? = e.clone();
    refresh(doc, element);
    Ok(())
}

/// Inserts a new revolve at the end of a Part Studio's feature list, named "Revolve N" (P3.4).
#[derive(Debug, Clone)]
pub struct AddRevolve {
    pub element: ElementId,
    pub feature: FeatureId,
    pub revolve: RevolveFeature,
}

impl Command for AddRevolve {
    fn label(&self) -> String {
        "Insert revolve".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if el.feature(self.feature).is_some() {
            return Err(CommandError::Invalid("feature id already in use".into()));
        }
        let name = el.next_feature_name("Revolve");
        insert_feature(doc, self.element, Feature {
            id: self.feature,
            name,
            kind: FeatureKind::Revolve(self.revolve.clone()),
            suppress_by: None,
        })?;
        Ok(())
    }
}

/// Sets a revolve's parameters (any change from the Revolve dialog).
#[derive(Debug, Clone)]
pub struct SetRevolve {
    pub element: ElementId,
    pub feature: FeatureId,
    pub revolve: RevolveFeature,
    /// Shown in the undo menu ("Select Face of Sketch 1", "Revolve angle", "Revolve axis").
    pub label: String,
}

impl Command for SetRevolve {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if !(self.revolve.angle > 0.0 && self.revolve.angle.is_finite()) {
            return Err(CommandError::Invalid("the angle must be positive".into()));
        }
        *part_studio_features(doc, self.element)?
            .iter_mut()
            .find(|f| f.id == self.feature)
            .and_then(|f| f.revolve_mut())
            .ok_or_else(|| CommandError::Invalid("revolve not found".into()))? = self.revolve.clone();
        refresh(doc, self.element);
        Ok(())
    }
}

/// The part settings of a Part Studio, for editing.
fn part_props(doc: &mut Document, element: ElementId) -> Result<&mut Vec<PartProps>, CommandError> {
    doc.element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?
        .part_props_mut()
        .ok_or_else(|| CommandError::Invalid("not a Part Studio".into()))
}

fn part_prop(props: &mut Vec<PartProps>, part: PartId) -> &mut PartProps {
    if let Some(i) = props.iter().position(|p| p.part == part) {
        return &mut props[i];
    }
    props.push(PartProps::new(part));
    props.last_mut().expect("just pushed")
}

/// Renames a part (the Parts list's Rename, PS6.5).
#[derive(Debug, Clone)]
pub struct RenamePart {
    pub element: ElementId,
    pub part: PartId,
    pub name: String,
}

impl Command for RenamePart {
    fn label(&self) -> String {
        format!("Rename part to {}", self.name.trim())
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = non_empty(&self.name)?;
        part_prop(part_props(doc, self.element)?, self.part).name = Some(name);
        Ok(())
    }
}

/// Sets a part's description (P3C.3: the Ex1 stand-in's "Made by cadrs", which a drawing's
/// title block shows). Since P3C.5 it is the part's Description property
/// ([`crate::properties`]), the one the Properties dialog edits.
#[derive(Debug, Clone)]
pub struct SetPartDescription {
    pub element: ElementId,
    pub part: PartId,
    pub description: Option<String>,
}

impl Command for SetPartDescription {
    fn label(&self) -> String {
        "Edit part description".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let d = self.description.as_ref().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
        let props = part_props(doc, self.element)?;
        part_prop(props, self.part).properties.description = d;
        props.retain(|p| !p.is_default());
        Ok(())
    }
}

/// Hides or shows parts (the Parts list's Hide and Show).
#[derive(Debug, Clone)]
pub struct SetPartsHidden {
    pub element: ElementId,
    pub parts: Vec<PartId>,
    pub hidden: bool,
}

impl Command for SetPartsHidden {
    fn label(&self) -> String {
        if self.hidden { "Hide parts".into() } else { "Show parts".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let props = part_props(doc, self.element)?;
        for p in &self.parts {
            part_prop(props, *p).hidden = self.hidden;
        }
        props.retain(|p| !p.is_default());
        Ok(())
    }
}

/// Sets or clears the appearance of parts (Edit appearance, the palette on the part's context
/// menu; PS9.1–9.3). `None` goes back to the palette colour.
#[derive(Debug, Clone)]
pub struct SetPartAppearance {
    pub element: ElementId,
    pub parts: Vec<PartId>,
    pub appearance: Option<Appearance>,
}

impl Command for SetPartAppearance {
    fn label(&self) -> String {
        "Edit appearance".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let props = part_props(doc, self.element)?;
        for p in &self.parts {
            part_prop(props, *p).appearance = self.appearance;
        }
        props.retain(|p| !p.is_default());
        Ok(())
    }
}

/// Sets or clears the appearance of faces of a part (Add appearance to face, PS9.4).
#[derive(Debug, Clone)]
pub struct SetFaceAppearance {
    pub element: ElementId,
    pub part: PartId,
    pub faces: Vec<FaceName>,
    pub appearance: Option<Appearance>,
}

impl Command for SetFaceAppearance {
    fn label(&self) -> String {
        if self.appearance.is_some() {
            "Add appearance to face".into()
        } else {
            "Remove face appearance".into()
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let props = part_props(doc, self.element)?;
        let p = part_prop(props, self.part);
        p.faces.retain(|(f, _)| !self.faces.contains(f));
        if let Some(a) = self.appearance {
            p.faces.extend(self.faces.iter().map(|f| (*f, a)));
        }
        props.retain(|p| !p.is_default());
        Ok(())
    }
}

/// Sets or clears the appearance of a feature (its faces, PS9.4) or a sketch (its curves,
/// PS9.5).
#[derive(Debug, Clone)]
pub struct SetFeatureAppearance {
    pub element: ElementId,
    pub feature: FeatureId,
    pub appearance: Option<Appearance>,
    pub label: String,
}

impl Command for SetFeatureAppearance {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        let ElementKind::PartStudio { appearances, features, .. } = &mut el.kind else {
            return Err(CommandError::Invalid("not a Part Studio".into()));
        };
        if !features.iter().any(|f| f.id == self.feature) {
            return Err(CommandError::Invalid("feature not found".into()));
        }
        appearances.retain(|(f, _)| *f != self.feature);
        if let Some(a) = self.appearance {
            appearances.push((self.feature, a));
        }
        Ok(())
    }
}

/// Sets or clears the appearance of one sketch curve (Edit curve appearance, PS9.5).
#[derive(Debug, Clone)]
pub struct SetCurveAppearance {
    pub element: ElementId,
    pub sketch: FeatureId,
    pub curve: CurveId,
    pub appearance: Option<Appearance>,
}

impl Command for SetCurveAppearance {
    fn label(&self) -> String {
        "Curve appearance".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        let ElementKind::PartStudio { curve_appearances, features, .. } = &mut el.kind else {
            return Err(CommandError::Invalid("not a Part Studio".into()));
        };
        let exists = features
            .iter()
            .find(|f| f.id == self.sketch)
            .and_then(|f| f.sketch())
            .is_some_and(|s| s.geometry.curves.contains_key(self.curve));
        if !exists {
            return Err(CommandError::Invalid("curve not found".into()));
        }
        curve_appearances.retain(|(f, c, _)| !(*f == self.sketch && *c == self.curve));
        if let Some(a) = self.appearance {
            curve_appearances.push((self.sketch, self.curve, a));
        }
        Ok(())
    }
}

/// Assigns a material to parts, or removes it (Assign material, PS10.1–10.3).
#[derive(Debug, Clone)]
pub struct SetPartMaterial {
    pub element: ElementId,
    pub parts: Vec<PartId>,
    pub material: Option<Material>,
}

impl Command for SetPartMaterial {
    fn label(&self) -> String {
        match &self.material {
            Some(m) => format!("Assign material {}", m.name),
            None => "Remove material".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if let Some(m) = &self.material {
            if !(m.density.is_finite() && m.density > 0.0) {
                return Err(CommandError::Invalid("the density must be positive".into()));
            }
            if m.name.trim().is_empty() {
                return Err(CommandError::Invalid("a material needs a name".into()));
            }
        }
        let props = part_props(doc, self.element)?;
        for p in &self.parts {
            part_prop(props, *p).material = self.material.clone();
        }
        props.retain(|p| !p.is_default());
        Ok(())
    }
}

/// Replaces the document's saved custom colours (the Edit appearance dialog's **+**, and a
/// custom colour's Delete and Update color, PS9.2).
#[derive(Debug, Clone)]
pub struct SetCustomColors {
    pub colors: Vec<Appearance>,
}

impl Command for SetCustomColors {
    fn label(&self) -> String {
        "Custom colors".into()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        doc.custom_colors = self.colors.clone();
        Ok(())
    }
}

/// Replaces the document's custom material libraries (PS10.3, P3.6): a new library, or a
/// material added to one.
#[derive(Debug, Clone)]
pub struct SetMaterialLibraries {
    pub libraries: Vec<crate::material::MaterialLibrary>,
    pub label: String,
}

impl Command for SetMaterialLibraries {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        for (i, l) in self.libraries.iter().enumerate() {
            if l.name.trim().is_empty() || self.libraries[..i].iter().any(|o| o.name == l.name) {
                return Err(CommandError::Invalid("library names must be unique and not empty".into()));
            }
        }
        doc.material_libraries = self.libraries.clone();
        Ok(())
    }
}

/// Shows or hides a sketch with its eye (PS1.5); `None` goes back to the automatic behaviour
/// (hidden once a feature uses it).
#[derive(Debug, Clone)]
pub struct SetSketchVisibility {
    pub element: ElementId,
    pub sketch: FeatureId,
    pub visible: Option<bool>,
}

impl Command for SetSketchVisibility {
    fn label(&self) -> String {
        match self.visible {
            Some(true) => "Show sketch".into(),
            Some(false) => "Hide sketch".into(),
            None => "Sketch visibility".into(),
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        let ElementKind::PartStudio { sketch_visibility, .. } = &mut el.kind else {
            return Err(CommandError::Invalid("not a Part Studio".into()));
        };
        sketch_visibility.retain(|(s, _)| *s != self.sketch);
        if let Some(v) = self.visible {
            sketch_visibility.push((self.sketch, v));
        }
        Ok(())
    }
}

/// Inserts a feature of another kind (a Boolean, a Delete part) at the end of the list, named
/// `base N`.
#[derive(Debug, Clone)]
pub struct AddFeature {
    pub element: ElementId,
    pub feature: FeatureId,
    pub base_name: String,
    pub kind: FeatureKind,
}

impl AddFeature {
    /// A Boolean feature ("Boolean N").
    pub fn boolean(element: ElementId, feature: FeatureId, b: BooleanFeature) -> Self {
        Self {
            element,
            feature,
            base_name: "Boolean".into(),
            kind: FeatureKind::Boolean(b),
        }
    }

    /// An Import feature ("Import N"); see also [`crate::import::AddImport`], which takes the
    /// file's bytes.
    pub fn import(element: ElementId, feature: FeatureId, x: crate::import::ImportFeature) -> Self {
        Self { element, feature, base_name: "Import".into(), kind: FeatureKind::Import(x) }
    }

    /// A Derived feature ("Derived N").
    pub fn derived(element: ElementId, feature: FeatureId, x: crate::derived::DerivedFeature) -> Self {
        Self { element, feature, base_name: "Derived".into(), kind: FeatureKind::Derived(Box::new(x)) }
    }

    /// A Delete part feature ("Delete part N").
    pub fn delete_parts(element: ElementId, feature: FeatureId, parts: Vec<PartId>) -> Self {
        Self {
            element,
            feature,
            base_name: "Delete part".into(),
            kind: FeatureKind::DeletePart(DeletePartFeature { parts }),
        }
    }
}

impl Command for AddFeature {
    fn label(&self) -> String {
        format!("Insert {}", self.base_name.to_lowercase())
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if el.feature(self.feature).is_some() {
            return Err(CommandError::Invalid("feature id already in use".into()));
        }
        let name = match &self.kind {
            FeatureKind::Hole(h) if !h.renamed => h.spec.callout(),
            // P3F.4: a Variable is listed by its name ("#piston_d").
            FeatureKind::Variable(v) if !v.name.is_empty() => format!("#{}", v.name),
            _ => el.next_feature_name(&self.base_name),
        };
        insert_feature(doc, self.element, Feature {
            id: self.feature,
            name,
            kind: self.kind.clone(),
            suppress_by: None,
        })?;
        refresh(doc, self.element);
        Ok(())
    }
}

/// Sets a Boolean feature's parameters.
#[derive(Debug, Clone)]
pub struct SetBoolean {
    pub element: ElementId,
    pub feature: FeatureId,
    pub boolean: BooleanFeature,
    pub label: String,
}

impl Command for SetBoolean {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        *part_studio_features(doc, self.element)?
            .iter_mut()
            .find(|f| f.id == self.feature)
            .and_then(|f| f.boolean_mut())
            .ok_or_else(|| CommandError::Invalid("boolean not found".into()))? = self.boolean.clone();
        refresh(doc, self.element);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::History;

    fn doc() -> Document {
        Document::new("Doc 1")
    }

    #[test]
    fn new_document_has_default_tabs() {
        let d = doc();
        let names: Vec<_> = d.elements.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Part Studio 1", "Assembly 1"]);
    }

    #[test]
    fn rename_undo_redo() {
        let mut d = doc();
        let mut h = History::default();
        h.execute(
            &mut d,
            &RenameDocument {
                name: "Bracket".into(),
            },
        )
        .unwrap();
        assert_eq!(d.name, "Bracket");
        assert_eq!(h.undo(&mut d).as_deref(), Some("Rename document"));
        assert_eq!(d.name, "Doc 1");
        h.redo(&mut d);
        assert_eq!(d.name, "Bracket");
        assert!(!h.can_redo());
    }

    #[test]
    fn workspace_units_are_undoable_and_saved() {
        use cadrs_sketch::units::{LengthUnit, Units};
        let mut d = doc();
        assert_eq!(d.units, Units::default());
        let mut h = History::default();
        let inch = Units::new(LengthUnit::Inch, 2);
        h.execute(&mut d, &SetUnits { units: inch }).unwrap();
        assert_eq!(d.units, inch);
        assert_eq!(h.undo_label(), Some("Change workspace units"));
        // Saved with the document; older files without units load as millimetres.
        let text = ron::to_string(&d).unwrap();
        let back: Document = ron::from_str(&text).unwrap();
        assert_eq!(back.units, inch);
        let old = text.replace(",units:(length:Inch,mass:Kilogram,decimals:2,angle_decimals:3)", "");
        assert_ne!(old, text);
        let back: Document = ron::from_str(&old).unwrap();
        assert_eq!(back.units, Units::default());
        // T2 files have no angle decimals: they get the default 3.
        let t2 = text.replace(",angle_decimals:3", "");
        assert_ne!(t2, text);
        let back: Document = ron::from_str(&t2).unwrap();
        assert_eq!(back.units, inch);
        // P3.4 files have no mass unit: they get kilograms.
        let p34 = text.replace("mass:Kilogram,", "");
        assert_ne!(p34, text);
        let back: Document = ron::from_str(&p34).unwrap();
        assert_eq!(back.units, inch);
        h.undo(&mut d);
        assert_eq!(d.units, Units::default());
        h.redo(&mut d);
        assert_eq!(d.units, inch);
    }

    #[test]
    fn renaming_a_sketch_is_one_undo_step() {
        let mut d = doc();
        let mut h = History::default();
        let ps = d.elements[0].id;
        let f = FeatureId::new();
        h.execute(&mut d, &AddSketch { element: ps, feature: f, plane: None })
            .unwrap();
        h.execute(
            &mut d,
            &RenameFeature {
                element: ps,
                feature: f,
                name: "  Base plate ".into(),
            },
        )
        .unwrap();
        assert_eq!(d.elements[0].feature(f).unwrap().name, "Base plate");
        assert!(
            h.execute(
                &mut d,
                &RenameFeature {
                    element: ps,
                    feature: f,
                    name: " ".into(),
                },
            )
            .is_err()
        );
        h.undo(&mut d);
        assert_eq!(d.elements[0].feature(f).unwrap().name, "Sketch 1");
        h.redo(&mut d);
        assert_eq!(d.elements[0].feature(f).unwrap().name, "Base plate");
    }

    #[test]
    fn failed_command_changes_nothing() {
        let mut d = doc();
        let before = d.clone();
        let mut h = History::default();
        let err = h.execute(&mut d, &RenameDocument { name: "  ".into() });
        assert!(err.is_err());
        assert_eq!(d, before);
        assert!(!h.can_undo());
    }

    #[test]
    fn noop_is_not_recorded() {
        let mut d = doc();
        let mut h = History::default();
        h.execute(
            &mut d,
            &RenameDocument {
                name: "Doc 1".into(),
            },
        )
        .unwrap();
        assert!(!h.can_undo());
    }

    #[test]
    fn add_and_delete_elements() {
        let mut d = doc();
        let mut h = History::default();
        let id = ElementId::new();
        h.execute(
            &mut d,
            &AddElement {
                id,
                kind: NewElementKind::PartStudio,
                name: None,
                after: None,
            },
        )
        .unwrap();
        assert_eq!(d.element(id).unwrap().name, "Part Studio 2");
        h.execute(&mut d, &DeleteElement { id }).unwrap();
        assert!(d.element(id).is_none());
        h.undo(&mut d);
        assert_eq!(d.elements[2].id, id);
        h.undo(&mut d);
        assert_eq!(d.elements.len(), 2);
    }

    #[test]
    fn new_tab_goes_right_of_the_given_tab() {
        let mut d = doc();
        let mut h = History::default();
        let ps = d.elements[0].id;
        let id = ElementId::new();
        h.execute(
            &mut d,
            &AddElement {
                id,
                kind: NewElementKind::Assembly,
                name: None,
                after: Some(ps),
            },
        )
        .unwrap();
        let names: Vec<_> = d.elements.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Part Studio 1", "Assembly 2", "Assembly 1"]);
        assert_eq!(h.undo_label(), Some("Create Assembly"));
        h.undo(&mut d);
        assert!(d.element(id).is_none());
        h.redo(&mut d);
        assert_eq!(d.elements[1].id, id);
    }

    #[test]
    fn rename_tab_undo_redo() {
        let mut d = doc();
        let mut h = History::default();
        let asm = d.elements[1].id;
        h.execute(
            &mut d,
            &RenameElement {
                id: asm,
                name: "  Main assembly ".into(),
            },
        )
        .unwrap();
        assert_eq!(d.element(asm).unwrap().name, "Main assembly");
        assert!(
            h.execute(
                &mut d,
                &RenameElement {
                    id: asm,
                    name: " ".into()
                }
            )
            .is_err()
        );
        h.undo(&mut d);
        assert_eq!(d.element(asm).unwrap().name, "Assembly 1");
        h.redo(&mut d);
        assert_eq!(d.element(asm).unwrap().name, "Main assembly");
    }

    #[test]
    fn delete_active_tab_and_undo_restores_position() {
        let mut d = doc();
        let mut h = History::default();
        let ps = d.elements[0].id;
        h.execute(&mut d, &DeleteElement { id: ps }).unwrap();
        assert_eq!(d.elements.len(), 1);
        // The last tab can never be deleted.
        let last = d.elements[0].id;
        assert!(h.execute(&mut d, &DeleteElement { id: last }).is_err());
        h.undo(&mut d);
        assert_eq!(d.elements[0].id, ps);
        assert_eq!(d.elements.len(), 2);
    }

    #[test]
    fn duplicate_tab() {
        let mut d = doc();
        let mut h = History::default();
        let ps = d.elements[0].id;
        let a = ElementId::new();
        let b = ElementId::new();
        h.execute(&mut d, &DuplicateElement { source: ps, id: a })
            .unwrap();
        h.execute(&mut d, &DuplicateElement { source: ps, id: b })
            .unwrap();
        let names: Vec<_> = d.elements.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Part Studio 1",
                "Part Studio 1 (2)",
                "Part Studio 1 (1)",
                "Assembly 1"
            ]
        );
        h.undo(&mut d);
        h.undo(&mut d);
        assert_eq!(d.elements.len(), 2);
    }

    #[test]
    fn cannot_delete_last_tab() {
        let mut d = Document::empty("x");
        d.elements.push(Element::part_studio("Part Studio 1"));
        let id = d.elements[0].id;
        let mut h = History::default();
        assert!(h.execute(&mut d, &DeleteElement { id }).is_err());
    }

    #[test]
    fn element_scope_undo_leaves_other_elements_alone() {
        let mut d = doc();
        let ps = d.elements[0].id;
        let asm = d.elements[1].id;
        let mut h = History::default();
        let sketch = FeatureId::new();
        h.execute(
            &mut d,
            &AddSketch {
                element: ps,
                feature: sketch,
                plane: Some(PlaneRef::Top),
            },
        )
        .unwrap();
        h.execute(
            &mut d,
            &AddSketchLine {
                element: ps,
                feature: sketch,
                a: Vec2::ZERO,
                b: Vec2::new(10.0, 0.0),
            },
        )
        .unwrap();
        // Rename the assembly without going through the history: undoing the part studio edit
        // must not revert it, because the snapshot only covers the part studio.
        d.element_mut(asm).unwrap().name = "Main assembly".into();
        h.undo(&mut d);
        let s = d.element(ps).unwrap().features()[0].sketch().unwrap();
        assert!(s.geometry.curves.is_empty());
        assert_eq!(d.element(asm).unwrap().name, "Main assembly");
        h.redo(&mut d);
        let s = d.element(ps).unwrap().features()[0].sketch().unwrap();
        assert_eq!(s.geometry.curves.len(), 1);
    }

    fn sketch_plane(d: &Document, el: ElementId, f: FeatureId) -> Option<PlaneRef> {
        d.element(el)?.feature(f)?.sketch()?.plane
    }

    fn feature_names(d: &Document, el: ElementId) -> Vec<String> {
        d.element(el)
            .unwrap()
            .features()
            .iter()
            .map(|f| f.name.clone())
            .collect()
    }

    /// Opening the dialog inserts an invalid sketch; picking a plane makes it valid; accepting
    /// squashes the session into one undo step.
    #[test]
    fn sketch_create_and_accept() {
        let mut d = doc();
        let ps = d.elements[0].id;
        let mut h = History::default();
        let f = FeatureId::new();
        let mark = h.undo_len();
        h.execute(&mut d, &AddSketch { element: ps, feature: f, plane: None })
            .unwrap();
        let feat = d.element(ps).unwrap().feature(f).unwrap();
        assert_eq!(feat.name, "Sketch 1");
        assert!(!feat.is_valid());
        h.execute(
            &mut d,
            &SetSketchPlane { element: ps, feature: f, plane: Some(PlaneRef::Top) },
        )
        .unwrap();
        assert!(d.element(ps).unwrap().feature(f).unwrap().is_valid());
        assert_eq!(h.undo_len(), 2);
        assert!(h.squash_since(mark, "Insert Sketch 1"));
        assert_eq!(h.undo_len(), 1);
        assert_eq!(h.undo_label(), Some("Insert Sketch 1"));
        // One undo removes the whole sketch; redo brings it back with its plane.
        h.undo(&mut d);
        assert!(d.element(ps).unwrap().features().is_empty());
        h.redo(&mut d);
        assert_eq!(sketch_plane(&d, ps, f), Some(PlaneRef::Top));
    }

    /// Cancelling a new sketch removes it, and the removal can be undone.
    #[test]
    fn sketch_cancel_is_undoable() {
        let mut d = doc();
        let ps = d.elements[0].id;
        let mut h = History::default();
        let f = FeatureId::new();
        let mark = h.undo_len();
        h.execute(&mut d, &AddSketch { element: ps, feature: f, plane: None })
            .unwrap();
        h.execute(
            &mut d,
            &SetSketchPlane { element: ps, feature: f, plane: Some(PlaneRef::Right) },
        )
        .unwrap();
        assert!(h.squash_since(mark, "Insert Sketch 1"));
        h.execute(
            &mut d,
            &DeleteFeature { element: ps, feature: f, label: "Cancel Sketch 1".into() },
        )
        .unwrap();
        assert!(d.element(ps).unwrap().features().is_empty());
        assert_eq!(h.undo(&mut d).as_deref(), Some("Cancel Sketch 1"));
        assert_eq!(sketch_plane(&d, ps, f), Some(PlaneRef::Right));
        // A cancelled session with nothing worth keeping can simply be discarded.
        let mark = h.undo_len();
        let g = FeatureId::new();
        h.execute(&mut d, &AddSketch { element: ps, feature: g, plane: None })
            .unwrap();
        h.undo(&mut d);
        h.discard_since(mark);
        assert_eq!(h.undo_len(), mark);
        assert!(!h.can_redo());
        assert!(d.element(ps).unwrap().feature(g).is_none());
    }

    /// Editing an existing sketch and accepting is one undo step that restores the old plane.
    #[test]
    fn sketch_edit_and_undo() {
        let mut d = doc();
        let ps = d.elements[0].id;
        let mut h = History::default();
        let f = FeatureId::new();
        h.execute(
            &mut d,
            &AddSketch { element: ps, feature: f, plane: Some(PlaneRef::Top) },
        )
        .unwrap();
        let before = d.element(ps).unwrap().feature(f).unwrap().clone();
        let mark = h.undo_len();
        h.execute(&mut d, &SetSketchPlane { element: ps, feature: f, plane: None })
            .unwrap();
        h.execute(
            &mut d,
            &SetSketchPlane { element: ps, feature: f, plane: Some(PlaneRef::Front) },
        )
        .unwrap();
        assert!(h.squash_since(mark, "Edit Sketch 1"));
        assert_eq!(h.undo_len(), 2);
        assert_eq!(sketch_plane(&d, ps, f), Some(PlaneRef::Front));
        h.undo(&mut d);
        assert_eq!(sketch_plane(&d, ps, f), Some(PlaneRef::Top));
        h.redo(&mut d);
        // Cancelling an edit puts the old feature back.
        h.execute(
            &mut d,
            &ReplaceFeature { element: ps, feature: before, label: "Cancel Sketch 1".into() },
        )
        .unwrap();
        assert_eq!(sketch_plane(&d, ps, f), Some(PlaneRef::Top));
        // An edit that changed nothing leaves no undo step.
        let n = h.undo_len();
        let mark = h.undo_len();
        h.execute(&mut d, &SetSketchPlane { element: ps, feature: f, plane: None })
            .unwrap();
        h.execute(
            &mut d,
            &SetSketchPlane { element: ps, feature: f, plane: Some(PlaneRef::Top) },
        )
        .unwrap();
        assert!(h.squash_since(mark, "Edit Sketch 1"));
        assert_eq!(h.undo_len(), n);
    }

    #[test]
    fn sketch_names_increment() {
        let mut d = doc();
        let ps = d.elements[0].id;
        let mut h = History::default();
        let ids: Vec<FeatureId> = (0..3).map(|_| FeatureId::new()).collect();
        for &f in &ids {
            h.execute(&mut d, &AddSketch { element: ps, feature: f, plane: None })
                .unwrap();
        }
        assert_eq!(feature_names(&d, ps), ["Sketch 1", "Sketch 2", "Sketch 3"]);
        // Deleting Sketch 1 does not free its number while higher ones exist.
        h.execute(
            &mut d,
            &DeleteFeature { element: ps, feature: ids[0], label: "Delete".into() },
        )
        .unwrap();
        let f = FeatureId::new();
        h.execute(&mut d, &AddSketch { element: ps, feature: f, plane: None })
            .unwrap();
        assert_eq!(feature_names(&d, ps), ["Sketch 2", "Sketch 3", "Sketch 4"]);
        // Renamed features do not count.
        h.execute(
            &mut d,
            &RenameFeature { element: ps, feature: f, name: " Base ".into() },
        )
        .unwrap();
        assert_eq!(feature_names(&d, ps)[2], "Base");
        // Sketches only live in Part Studios.
        let asm = d.elements[1].id;
        assert!(
            h.execute(&mut d, &AddSketch { element: asm, feature: FeatureId::new(), plane: None })
                .is_err()
        );
        // Ids must be unique.
        assert!(
            h.execute(&mut d, &AddSketch { element: ps, feature: f, plane: None })
                .is_err()
        );
    }

    #[test]
    fn imprinting_option_is_undoable() {
        let mut d = doc();
        let ps = d.elements[0].id;
        let mut h = History::default();
        let f = FeatureId::new();
        h.execute(&mut d, &AddSketch { element: ps, feature: f, plane: None })
            .unwrap();
        h.execute(
            &mut d,
            &SetSketchImprinting { element: ps, feature: f, disable_imprinting: true },
        )
        .unwrap();
        let s = |d: &Document| d.element(ps).unwrap().feature(f).unwrap().sketch().unwrap().clone();
        assert!(s(&d).disable_imprinting);
        h.undo(&mut d);
        assert!(!s(&d).disable_imprinting);
    }

    #[test]
    fn squash_refuses_mixed_scopes() {
        let mut d = doc();
        let ps = d.elements[0].id;
        let mut h = History::default();
        h.execute(&mut d, &AddSketch { element: ps, feature: FeatureId::new(), plane: None })
            .unwrap();
        h.execute(&mut d, &RenameDocument { name: "Other".into() })
            .unwrap();
        assert!(!h.squash_since(0, "x"));
        assert_eq!(h.undo_len(), 2);
    }

    #[test]
    fn new_command_clears_redo() {
        let mut d = doc();
        let mut h = History::default();
        h.execute(&mut d, &RenameDocument { name: "A".into() })
            .unwrap();
        h.undo(&mut d);
        assert!(h.can_redo());
        h.execute(&mut d, &RenameDocument { name: "B".into() })
            .unwrap();
        assert!(!h.can_redo());
    }

    #[test]
    fn history_limit() {
        let mut d = doc();
        let mut h = History::new(3);
        for i in 0..5 {
            h.execute(
                &mut d,
                &RenameDocument {
                    name: format!("n{i}"),
                },
            )
            .unwrap();
        }
        let mut n = 0;
        while h.undo(&mut d).is_some() {
            n += 1;
        }
        assert_eq!(n, 3);
        assert_eq!(d.name, "n1");
    }

    #[test]
    fn document_round_trips_through_ron() {
        let d = doc();
        let text = ron::to_string(&d).unwrap();
        let back: Document = ron::from_str(&text).unwrap();
        assert_eq!(d, back);
    }

    fn sketch_doc() -> (Document, History, ElementId, FeatureId) {
        let mut d = doc();
        let mut h = History::default();
        let element = d.elements[0].id;
        let feature = FeatureId::new();
        h.execute(
            &mut d,
            &AddSketch {
                element,
                feature,
                plane: Some(PlaneRef::Top),
            },
        )
        .unwrap();
        (d, h, element, feature)
    }

    fn geometry(d: &Document, e: ElementId, f: FeatureId) -> &cadrs_sketch::Sketch {
        &d.element(e).unwrap().feature(f).unwrap().sketch().unwrap().geometry
    }

    #[test]
    fn sketch_edits_undo_and_redo() {
        let (mut d, mut h, element, feature) = sketch_doc();
        let add = EditSketch {
            element,
            feature,
            op: SketchOp::AddPolyline {
                points: vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 5.0)],
                closed: false,
                construction: false,
                label: "Add line",
            },
        };
        h.execute(&mut d, &add).unwrap();
        assert_eq!(geometry(&d, element, feature).curves.len(), 2);
        let circle = EditSketch {
            element,
            feature,
            op: SketchOp::AddCircle {
                center: Vec2::new(30.0, 0.0),
                radius: 4.0,
                construction: false,
            },
        };
        h.execute(&mut d, &circle).unwrap();
        assert_eq!(h.undo_label(), Some("Add circle"));
        let line = geometry(&d, element, feature)
            .curves
            .iter()
            .find(|(_, c)| matches!(c.kind, cadrs_sketch::CurveKind::Line { .. }))
            .unwrap()
            .0;
        h.execute(
            &mut d,
            &EditSketch {
                element,
                feature,
                op: SketchOp::Delete {
                    curves: vec![line],
                    points: vec![],
                    dimensions: vec![],
                    constraints: vec![],
                },
            },
        )
        .unwrap();
        assert_eq!(geometry(&d, element, feature).curves.len(), 2);
        assert_eq!(geometry(&d, element, feature).points.len(), 3);
        assert_eq!(h.undo(&mut d).as_deref(), Some("Delete"));
        assert_eq!(geometry(&d, element, feature).curves.len(), 3);
        assert_eq!(h.undo(&mut d).as_deref(), Some("Add circle"));
        assert_eq!(h.undo(&mut d).as_deref(), Some("Add line"));
        assert!(geometry(&d, element, feature).is_empty());
        h.redo(&mut d);
        assert_eq!(geometry(&d, element, feature).curves.len(), 2);
    }

    #[test]
    fn invalid_sketch_edit_changes_nothing() {
        let (mut d, mut h, element, feature) = sketch_doc();
        let before = d.clone();
        let bad = EditSketch {
            element,
            feature,
            op: SketchOp::AddCircle {
                center: Vec2::ZERO,
                radius: 0.0,
                construction: false,
            },
        };
        assert!(h.execute(&mut d, &bad).is_err());
        assert_eq!(d, before);
        assert_eq!(h.undo_len(), 1);
    }

    #[test]
    fn dimension_edit_delete_and_label_moves_undo() {
        use cadrs_sketch::{Dimension, DimensionKind};
        let (mut d, mut h, element, feature) = sketch_doc();
        let edit = |op| EditSketch {
            element,
            feature,
            op,
        };
        h.execute(
            &mut d,
            &edit(SketchOp::AddPolyline {
                points: vec![Vec2::ZERO, Vec2::new(10.0, 0.0)],
                closed: false,
                construction: false,
                label: "Add line",
            }),
        )
        .unwrap();
        let g = geometry(&d, element, feature);
        let (a, b) = g.curve_ends(g.curves.keys().next().unwrap()).unwrap();
        let kind = DimensionKind::Aligned { a, b };
        h.execute(
            &mut d,
            &edit(SketchOp::SetDimension {
                dimension: Dimension::new(kind, 10.0, -5.0),
                moves: vec![],
                radii: vec![],
            }),
        )
        .unwrap();
        let id = geometry(&d, element, feature).dimensions.keys().next().unwrap();
        let length = |d: &Document| {
            let g = geometry(d, element, feature);
            g.pos(a).distance(g.pos(b))
        };
        // Edit the value.
        h.execute(&mut d, &edit(SketchOp::SetDimensionValue { id, value: 25.4 }))
            .unwrap();
        assert!((length(&d) - 25.4).abs() < 1e-6);
        assert_eq!(h.undo_label(), Some("Edit dimension"));
        // Move the label.
        h.execute(
            &mut d,
            &edit(SketchOp::MoveDimensionLabel {
                id,
                offset: -12.0,
                along: 3.0,
            }),
        )
        .unwrap();
        assert_eq!(h.undo_label(), Some("Move dimension"));
        // Delete it.
        h.execute(
            &mut d,
            &edit(SketchOp::Delete {
                curves: vec![],
                points: vec![],
                dimensions: vec![id],
                constraints: vec![],
            }),
        )
        .unwrap();
        assert!(geometry(&d, element, feature).dimensions.is_empty());
        // Undo each in turn.
        assert_eq!(h.undo(&mut d).as_deref(), Some("Delete"));
        assert_eq!(geometry(&d, element, feature).dimensions[id].offset, -12.0);
        assert_eq!(h.undo(&mut d).as_deref(), Some("Move dimension"));
        let dim = geometry(&d, element, feature).dimensions[id];
        assert_eq!((dim.offset, dim.along), (-5.0, 0.0));
        assert_eq!(h.undo(&mut d).as_deref(), Some("Edit dimension"));
        assert_eq!(geometry(&d, element, feature).dimensions[id].value, 10.0);
        assert!((length(&d) - 10.0).abs() < 1e-6);
        // An invalid value is refused and changes nothing.
        let before = d.clone();
        assert!(h
            .execute(&mut d, &edit(SketchOp::SetDimensionValue { id, value: 0.0 }))
            .is_err());
        assert_eq!(d, before);
    }

    /// A Part Studio with Sketch 1 on Top holding a 50 × 30 rectangle at the origin.
    fn rectangle_doc() -> (Document, History, ElementId, FeatureId) {
        let (mut d, mut h, element, feature) = sketch_doc();
        h.execute(
            &mut d,
            &EditSketch {
                element,
                feature,
                op: SketchOp::AddPolyline {
                    points: vec![
                        Vec2::ZERO,
                        Vec2::new(50.0, 0.0),
                        Vec2::new(50.0, 30.0),
                        Vec2::new(0.0, 30.0),
                    ],
                    closed: true,
                    construction: false,
                    label: "Add rectangle",
                },
            },
        )
        .unwrap();
        (d, h, element, feature)
    }

    fn region_of(d: &Document, e: ElementId, sketch: FeatureId) -> crate::document::RegionRef {
        let g = geometry(d, e, sketch);
        let r = cadrs_sketch::region::regions(g).remove(0);
        crate::document::RegionRef::new(sketch, &r)
    }

    #[test]
    fn extrude_insert_edit_undo_redo() {
        use crate::parts::{parts, sketch_consumed};
        let (mut d, mut h, element, sketch) = rectangle_doc();
        let ex = FeatureId::new();
        // The dialog opens with nothing selected: invalid, no part.
        h.execute(
            &mut d,
            &AddExtrude { element, feature: ex, extrude: ExtrudeFeature::default() },
        )
        .unwrap();
        let f = d.element(element).unwrap().feature(ex).unwrap().clone();
        assert_eq!(f.name, "Extrude 1");
        assert!(!f.is_valid());
        assert!(parts(d.element(element).unwrap().features()).is_empty());
        // Picking the region: valid, Part 1, and the sketch is consumed.
        let mut params = ExtrudeFeature {
            regions: vec![region_of(&d, element, sketch)],
            ..ExtrudeFeature::default()
        };
        h.execute(
            &mut d,
            &SetExtrude {
                element,
                feature: ex,
                extrude: params.clone(),
                label: "Select Face of Sketch 1".into(),
            },
        )
        .unwrap();
        let features = d.element(element).unwrap().features().to_vec();
        assert!(features[1].is_valid());
        assert!(sketch_consumed(&features, sketch));
        let p = parts(&features);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "Part 1");
        assert!((p[0].solid.volume() - 50.0 * 30.0 * 25.0).abs() < 1e-6);
        // A new depth and flipped.
        params.depth = 12.5;
        params.depth_expr = "25/2".into();
        params.flip = true;
        h.execute(
            &mut d,
            &SetExtrude { element, feature: ex, extrude: params.clone(), label: "Depth".into() },
        )
        .unwrap();
        let (lo, _) = parts(d.element(element).unwrap().features())[0].solid.bounds().unwrap();
        assert!((lo[2] + 12.5).abs() < 1e-9);
        // A zero depth is refused.
        let bad = ExtrudeFeature { depth: 0.0, ..params.clone() };
        assert!(
            h.execute(&mut d, &SetExtrude { element, feature: ex, extrude: bad, label: "x".into() })
                .is_err()
        );
        // Undo steps back through each edit, then removes the extrude.
        assert_eq!(h.undo(&mut d).as_deref(), Some("Depth"));
        let e = d.element(element).unwrap().feature(ex).unwrap().extrude().unwrap().clone();
        assert_eq!((e.depth, e.flip), (25.0, false));
        h.undo(&mut d);
        h.undo(&mut d);
        assert!(d.element(element).unwrap().feature(ex).is_none());
        assert!(!sketch_consumed(d.element(element).unwrap().features(), sketch));
        h.redo(&mut d);
        h.redo(&mut d);
        h.redo(&mut d);
        let e = d.element(element).unwrap().feature(ex).unwrap().extrude().unwrap().clone();
        assert_eq!(e, params);
        // A second extrude is "Extrude 2" and makes Part 2.
        let ex2 = FeatureId::new();
        let extrude = ExtrudeFeature {
            regions: vec![region_of(&d, element, sketch)],
            ..ExtrudeFeature::default()
        };
        h.execute(&mut d, &AddExtrude { element, feature: ex2, extrude })
            .unwrap();
        let features = d.element(element).unwrap().features();
        assert_eq!(features[2].name, "Extrude 2");
        assert_eq!(parts(features)[1].name, "Part 2");
    }

    /// A sketch on the top face of a part sits on the face and follows it when the depth
    /// changes.
    #[test]
    fn sketches_on_faces_follow_their_face() {
        use crate::parts::face_plane;
        let (mut d, mut h, element, sketch) = rectangle_doc();
        let ex = FeatureId::new();
        let mut params = ExtrudeFeature {
            regions: vec![region_of(&d, element, sketch)],
            ..ExtrudeFeature::default()
        };
        h.execute(&mut d, &AddExtrude { element, feature: ex, extrude: params.clone() })
            .unwrap();
        let features = d.element(element).unwrap().features().to_vec();
        let top = face_plane(&features, ex, crate::parts::cap_name(&features, ex, 0, true).unwrap()).unwrap();
        let frame = top.frame();
        assert!((frame.normal()[2] - 1.0).abs() < 1e-9);
        assert!((frame.origin[2] - 25.0).abs() < 1e-9);
        // Sketch 2 on the top face, with a circle.
        let s2 = FeatureId::new();
        h.execute(&mut d, &AddSketch { element, feature: s2, plane: Some(top) })
            .unwrap();
        assert_eq!(
            d.element(element).unwrap().features()[2].name,
            "Sketch 2"
        );
        // Deepening the extrude moves the face, and the sketch with it.
        params.depth = 40.0;
        h.execute(
            &mut d,
            &SetExtrude { element, feature: ex, extrude: params, label: "Depth".into() },
        )
        .unwrap();
        let plane = d.element(element).unwrap().feature(s2).unwrap().sketch().unwrap().plane;
        let f = plane.unwrap().frame();
        assert!((f.origin[2] - 40.0).abs() < 1e-9, "{f:?}");
        assert!(f.to_world(Vec2::new(10.0, 5.0)) == [10.0, 5.0, 40.0]);
        // Undo puts it back.
        h.undo(&mut d);
        let plane = d.element(element).unwrap().feature(s2).unwrap().sketch().unwrap().plane;
        assert!((plane.unwrap().frame().origin[2] - 25.0).abs() < 1e-9);
        // A side face is a sketch plane too: the front face (y = 0) faces -Y.
        let features = d.element(element).unwrap().features().to_vec();
        let solid = crate::parts::part_of(&features, features[1].id).unwrap().solid;
        let front = solid
            .faces
            .iter()
            .find(|f| f.plane.is_some_and(|p| p.normal()[1] < -0.99))
            .unwrap();
        let fp = face_plane(&features, ex, front.name).unwrap().frame();
        assert!(fp.distance([3.0, 0.0, 7.0]).abs() < 1e-9);
        assert!((fp.normal()[1] + 1.0).abs() < 1e-9);
    }
}
