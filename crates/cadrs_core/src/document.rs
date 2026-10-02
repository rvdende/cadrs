//! The document model. Persistence lives in [`crate::store`].

use cadrs_sketch::units::Units;
use cadrs_sketch::{CurveId, EdgeName, FaceName, PlaneRef, Region, Sketch, Vec2, Vec3, VertexName};
use serde::{Deserialize, Serialize};

use crate::ids::{DocumentId, ElementId, FeatureId, PartId};

/// A document: a name and an ordered list of elements (the tabs at the bottom of the window).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub name: String,
    pub elements: Vec<Element>,
    /// The workspace units (X1): how lengths are shown and bare numbers read. Values are
    /// stored in millimetres whatever the units.
    #[serde(default)]
    pub units: Units,
    /// Colours saved with **+** in the Edit appearance dialog (PS9.2, P3.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_colors: Vec<crate::appearance::Appearance>,
    /// Custom material libraries made with the Material dialog's **+** (PS10.3, P3.6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub material_libraries: Vec<crate::material::MaterialLibrary>,
    /// The standard content configurations its assemblies use (P3B.5): generated Part Studios
    /// that are not tabs, found by [`Document::element`] (see [`crate::assembly::standard`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub standard_content: Vec<crate::assembly::standard::StandardPart>,
    /// Custom property definitions, part numbering and BOM templates (P3B.6, see
    /// [`crate::properties`]).
    #[serde(default, skip_serializing_if = "crate::properties::PropertySettings::is_default")]
    pub properties: crate::properties::PropertySettings,
    /// P3G.1 (schema 5): frozen copies of the elements this document references from another
    /// document or at a version, under namespaced ids (see [`crate::external`]). Not tabs:
    /// found by [`Document::element`], never edited.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked: Vec<crate::external::LinkedElement>,
    /// P3G.3 (ER7.6): tabs moved to another document ([`crate::move_doc`]), so a reference to
    /// one of them at an older version can be pointed at the new document.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moved: Vec<crate::move_doc::MovedElement>,
    /// P3E.2 (TD5.3): the tab folders and their order ([`crate::tab_tree`]). Empty without
    /// folders: the tabs are then in element order.
    #[serde(default, skip_serializing_if = "crate::tab_tree::TabTree::is_empty")]
    pub tab_tree: crate::tab_tree::TabTree,
}

impl Document {
    /// An empty document without any elements.
    pub fn empty(name: impl Into<String>) -> Self {
        Self {
            id: DocumentId::new(),
            name: name.into(),
            elements: Vec::new(),
            units: Units::default(),
            custom_colors: Vec::new(),
            material_libraries: Vec::new(),
            standard_content: Vec::new(),
            properties: Default::default(),
            linked: Vec::new(),
            moved: Vec::new(),
            tab_tree: Default::default(),
        }
    }

    /// A new document as Onshape creates it: "Part Studio 1" and "Assembly 1".
    pub fn new(name: impl Into<String>) -> Self {
        let mut doc = Self::empty(name);
        doc.elements.push(Element::part_studio("Part Studio 1"));
        doc.elements.push(Element::assembly("Assembly 1"));
        doc
    }

    /// A tab, a standard content configuration's generated Part Studio (P3B.5), or a linked
    /// element's frozen copy (P3G.1).
    pub fn element(&self, id: ElementId) -> Option<&Element> {
        self.elements
            .iter()
            .find(|e| e.id == id)
            .or_else(|| self.standard_part(id).map(|p| &p.element))
            .or_else(|| self.linked_element(id).map(|l| &l.element))
    }

    /// The linked element (P3G.1) stored under `id`.
    pub fn linked_element(&self, id: ElementId) -> Option<&crate::external::LinkedElement> {
        self.linked.iter().find(|l| l.element.id == id)
    }

    pub fn element_mut(&mut self, id: ElementId) -> Option<&mut Element> {
        self.elements.iter_mut().find(|e| e.id == id)
    }

    /// The standard content configuration whose generated studio is `id` (P3B.5).
    pub fn standard_part(&self, id: ElementId) -> Option<&crate::assembly::standard::StandardPart> {
        self.standard_content.iter().find(|p| p.element.id == id)
    }

    pub fn element_index(&self, id: ElementId) -> Option<usize> {
        self.elements.iter().position(|e| e.id == id)
    }

    /// The next free default name such as "Part Studio 2".
    pub fn next_element_name(&self, base: &str) -> String {
        let mut n = 1;
        loop {
            let name = format!("{base} {n}");
            if !self.elements.iter().any(|e| e.name == name) {
                return name;
            }
            n += 1;
        }
    }
}

/// One tab of a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: ElementId,
    pub name: String,
    pub kind: ElementKind,
    /// An Assembly tab's instances (P3B.1, [`crate::assembly`]); empty for a Part Studio.
    #[serde(default, skip_serializing_if = "crate::assembly::Assembly::is_empty")]
    pub assembly: crate::assembly::Assembly,
    /// A Part Studio edited **in the context** of an assembly (P3B.9, X15,
    /// [`crate::assembly::context`]): the other instances around its part, as reference
    /// geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<crate::assembly::context::StudioContext>,
    /// A Part Studio's or Assembly's simulation setup: its Loads list and mesh (P3F.5,
    /// [`crate::simulation`]).
    #[serde(default, skip_serializing_if = "crate::simulation::Simulation::is_empty")]
    pub simulation: crate::simulation::Simulation,
    /// Cameras saved under a name (P3E.3a, TD6.5: the view cube menu's Named views…), see
    /// [`crate::named_views`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_views: Vec<crate::named_views::NamedView>,
}

impl Element {
    pub fn part_studio(name: impl Into<String>) -> Self {
        Self {
            id: ElementId::new(),
            name: name.into(),
            kind: ElementKind::PartStudio {
                features: Vec::new(),
                parts: Vec::new(),
                sketch_visibility: Vec::new(),
                appearances: Vec::new(),
                curve_appearances: Vec::new(),
                folders: Vec::new(),
                suppressed: Vec::new(),
                rollback: None,
            },
            assembly: Default::default(),
            context: None,
            simulation: Default::default(),
            named_views: Vec::new(),
        }
    }

    /// A Render Studio tab of `source` (P3F.6).
    pub fn render_studio(name: impl Into<String>, source: Option<ElementId>) -> Self {
        Self {
            id: ElementId::new(),
            name: name.into(),
            kind: ElementKind::Render(Box::new(crate::render::RenderStudio::new(source))),
            assembly: Default::default(),
            context: None,
            simulation: Default::default(),
            named_views: Vec::new(),
        }
    }

    pub fn assembly(name: impl Into<String>) -> Self {
        Self {
            id: ElementId::new(),
            name: name.into(),
            kind: ElementKind::Assembly,
            assembly: Default::default(),
            context: None,
            simulation: Default::default(),
            named_views: Vec::new(),
        }
    }

    /// A Drawing tab (P3C.1).
    pub fn drawing(name: impl Into<String>, drawing: cadrs_drawing::Drawing) -> Self {
        Self {
            id: ElementId::new(),
            name: name.into(),
            kind: ElementKind::Drawing(Box::new(drawing)),
            assembly: Default::default(),
            context: None,
            simulation: Default::default(),
            named_views: Vec::new(),
        }
    }

    /// The drawing of a Drawing tab.
    pub fn drawing_data(&self) -> Option<&cadrs_drawing::Drawing> {
        match &self.kind {
            ElementKind::Drawing(d) => Some(d),
            _ => None,
        }
    }

    /// An Assembly tab's instances (P3B.1); `None` for a Part Studio.
    pub fn assembly_model(&self) -> Option<&crate::assembly::Assembly> {
        matches!(self.kind, ElementKind::Assembly).then_some(&self.assembly)
    }

    pub fn assembly_model_mut(&mut self) -> Option<&mut crate::assembly::Assembly> {
        matches!(self.kind, ElementKind::Assembly).then_some(&mut self.assembly)
    }

    pub fn features(&self) -> &[Feature] {
        match &self.kind {
            ElementKind::PartStudio { features, .. } => features,
            _ => &[],
        }
    }

    /// The feature list of a Part Studio, for editing.
    pub fn features_mut(&mut self) -> Option<&mut Vec<Feature>> {
        match &mut self.kind {
            ElementKind::PartStudio { features, .. } => Some(features),
            _ => None,
        }
    }

    /// The feature list's folders (P3.6).
    pub fn folders(&self) -> &[FeatureFolder] {
        match &self.kind {
            ElementKind::PartStudio { folders, .. } => folders,
            _ => &[],
        }
    }

    pub fn folders_mut(&mut self) -> Option<&mut Vec<FeatureFolder>> {
        match &mut self.kind {
            ElementKind::PartStudio { folders, .. } => Some(folders),
            _ => None,
        }
    }

    /// The suppressed features (P3.9).
    pub fn suppressed(&self) -> &[FeatureId] {
        match &self.kind {
            ElementKind::PartStudio { suppressed, .. } => suppressed,
            ElementKind::Assembly | ElementKind::Drawing(_) | ElementKind::PcbStudio(_) | ElementKind::Render(_) => &[],
        }
    }

    /// True if the feature is suppressed: by Suppress, or by its suppression variable (IR5.5).
    /// Only a feature with a suppression variable evaluates the variables (once, the list's);
    /// for many features, take [`Self::all_suppressed`] once instead.
    pub fn is_suppressed(&self, feature: FeatureId) -> bool {
        self.suppressed().contains(&feature)
            || (self.feature(feature).is_some_and(|f| f.suppress_by.is_some()) && self.suppressed_by_variable().contains(&feature))
    }

    /// The features their suppression variable suppresses (IR5.5), with the variables' values
    /// as last evaluated ([`crate::variables::suppressed_by_variables`]).
    pub fn suppressed_by_variable(&self) -> Vec<FeatureId> {
        crate::variables::suppressed_by_variables(self.features(), self.suppressed())
    }

    /// Every suppressed feature, in list order: by Suppress or by a variable (IR5.5).
    pub fn all_suppressed(&self) -> Vec<FeatureId> {
        let by_var = self.suppressed_by_variable();
        let manual = self.suppressed();
        self.features().iter().map(|f| f.id).filter(|f| manual.contains(f) || by_var.contains(f)).collect()
    }

    /// The number of features above the rollback bar (P3.9): all of them when it is at the end.
    pub fn rollback_index(&self) -> usize {
        let n = self.features().len();
        match &self.kind {
            ElementKind::PartStudio { rollback: Some(i), .. } => (*i).min(n),
            _ => n,
        }
    }

    /// True if the feature is below the rollback bar.
    pub fn is_rolled_back(&self, feature: FeatureId) -> bool {
        self.features().iter().position(|f| f.id == feature).is_some_and(|i| i >= self.rollback_index())
    }

    /// The features that are built (P3.9): the ones above the rollback bar, without the
    /// suppressed ones (by Suppress or by a variable, IR5.5).
    pub fn active_features(&self) -> Vec<Feature> {
        let bar = self.rollback_index();
        let suppressed = self.suppressed();
        let by_var = self.suppressed_by_variable();
        self.features()[..bar]
            .iter()
            .filter(|f| !suppressed.contains(&f.id) && !by_var.contains(&f.id))
            .cloned()
            .collect()
    }

    /// The folder a feature is in.
    pub fn folder_of(&self, feature: FeatureId) -> Option<&FeatureFolder> {
        self.folders().iter().find(|f| f.features.contains(&feature))
    }

    /// The part settings of a Part Studio (renames, hidden parts).
    pub fn part_props(&self) -> &[PartProps] {
        match &self.kind {
            ElementKind::PartStudio { parts, .. } => parts,
            _ => &[],
        }
    }

    pub fn part_props_mut(&mut self) -> Option<&mut Vec<PartProps>> {
        match &mut self.kind {
            ElementKind::PartStudio { parts, .. } => Some(parts),
            _ => None,
        }
    }

    /// A sketch's shown (`Some(true)`) or hidden (`Some(false)`) setting from its eye, if set.
    pub fn sketch_visibility(&self, sketch: FeatureId) -> Option<bool> {
        match &self.kind {
            ElementKind::PartStudio { sketch_visibility, .. } => {
                sketch_visibility.iter().find(|(s, _)| *s == sketch).map(|(_, v)| *v)
            }
            _ => None,
        }
    }

    /// The appearances of features and sketches (PS9.4, PS9.5), by feature.
    pub fn feature_appearances(&self) -> &[(FeatureId, crate::appearance::Appearance)] {
        match &self.kind {
            ElementKind::PartStudio { appearances, .. } => appearances,
            _ => &[],
        }
    }

    /// The appearances of single sketch curves (PS9.5).
    pub fn curve_appearances(&self) -> &[(FeatureId, CurveId, crate::appearance::Appearance)] {
        match &self.kind {
            ElementKind::PartStudio { curve_appearances, .. } => curve_appearances,
            _ => &[],
        }
    }

    /// A feature's or sketch's appearance, if it has one.
    pub fn feature_appearance(&self, feature: FeatureId) -> Option<crate::appearance::Appearance> {
        self.feature_appearances().iter().find(|(f, _)| *f == feature).map(|(_, a)| *a)
    }

    /// A part's settings, if it has any.
    pub fn part_prop(&self, part: PartId) -> Option<&PartProps> {
        self.part_props().iter().find(|p| p.part == part)
    }

    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.features().iter().find(|f| f.id == id)
    }

    /// The next free default feature name, such as "Sketch 2": one more than the highest number
    /// in use, as Onshape does (deleting "Sketch 1" does not make the name free again while
    /// "Sketch 2" exists).
    pub fn next_feature_name(&self, base: &str) -> String {
        let prefix = format!("{base} ");
        let max = self
            .features()
            .iter()
            .filter_map(|f| f.name.strip_prefix(&prefix)?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("{base} {}", max + 1)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElementKind {
    PartStudio {
        features: Vec<Feature>,
        /// The parts' own settings (P3.3).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parts: Vec<PartProps>,
        /// Sketches shown or hidden with their eye (PS1.5); the others are shown until a
        /// feature uses them.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sketch_visibility: Vec<(FeatureId, bool)>,
        /// Appearances of features (their faces, PS9.4) and sketches (their curves, PS9.5)
        /// (P3.5).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        appearances: Vec<(FeatureId, crate::appearance::Appearance)>,
        /// Appearances of single sketch curves (Edit curve appearance, PS9.5), over their
        /// sketch's.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        curve_appearances: Vec<(FeatureId, CurveId, crate::appearance::Appearance)>,
        /// Folders in the feature list (P3.6, PS3/PS17.2): each holds a run of features.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        folders: Vec<FeatureFolder>,
        /// Suppressed features (P3.9, PS13): left out of the rebuild, greyed and struck through
        /// in the feature list.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        suppressed: Vec<FeatureId>,
        /// Where the rollback bar sits (P3.9, PS13.1): the number of features above it; `None`
        /// at the end. The features below it are rolled back (not built).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rollback: Option<usize>,
    },
    Assembly,
    /// A drawing (P3C): sheets, templates and drawing properties, see `cadrs_drawing`.
    Drawing(Box<cadrs_drawing::Drawing>),
    /// A PCB Studio (P3H.3): boards imported from IDF and the studio's settings, see
    /// [`crate::pcb`].
    PcbStudio(Box<crate::pcb::PcbStudio>),
    /// A Render Studio (P3F.6): renders a Part Studio or Assembly, see [`crate::render`].
    Render(Box<crate::render::RenderStudio>),
}

/// A folder in the feature list (P3.6): a name and the features in it, which follow each other
/// in the list. It is shown at its first feature; closed, its features are hidden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureFolder {
    pub id: FeatureId,
    pub name: String,
    pub features: Vec<FeatureId>,
    #[serde(default)]
    pub open: bool,
}

/// A feature in a Part Studio's feature list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    pub kind: FeatureKind,
    /// IR5.5 "Suppress by variable…": a variable that suppresses the feature (see
    /// [`crate::variables::SuppressByVariable`]). Files from before it load without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppress_by: Option<crate::variables::SuppressByVariable>,
}

impl Feature {
    /// A feature with no suppression variable.
    pub fn new(id: FeatureId, name: impl Into<String>, kind: FeatureKind) -> Self {
        Self { id, name: name.into(), kind, suppress_by: None }
    }

    /// The sketch parameters, if this is a sketch feature.
    pub fn sketch(&self) -> Option<&SketchFeature> {
        match &self.kind {
            FeatureKind::Sketch(s) => Some(s),
            _ => None,
        }
    }

    pub fn sketch_mut(&mut self) -> Option<&mut SketchFeature> {
        match &mut self.kind {
            FeatureKind::Sketch(s) => Some(s),
            _ => None,
        }
    }

    /// The extrude parameters, if this is an extrude.
    pub fn extrude(&self) -> Option<&ExtrudeFeature> {
        match &self.kind {
            FeatureKind::Extrude(e) => Some(e),
            _ => None,
        }
    }

    pub fn extrude_mut(&mut self) -> Option<&mut ExtrudeFeature> {
        match &mut self.kind {
            FeatureKind::Extrude(e) => Some(e),
            _ => None,
        }
    }

    /// The revolve parameters, if this is a revolve (P3.4).
    pub fn revolve(&self) -> Option<&RevolveFeature> {
        match &self.kind {
            FeatureKind::Revolve(r) => Some(r),
            _ => None,
        }
    }

    pub fn revolve_mut(&mut self) -> Option<&mut RevolveFeature> {
        match &mut self.kind {
            FeatureKind::Revolve(r) => Some(r),
            _ => None,
        }
    }

    /// The sketches a feature takes regions or curves of (an extrude's or a revolve's): Onshape
    /// hides them once the feature is accepted (PS1.4).
    pub fn input_sketches(&self) -> Vec<FeatureId> {
        match &self.kind {
            FeatureKind::Extrude(e) => e.sketches(),
            FeatureKind::Revolve(r) => r.sketches(),
            FeatureKind::Hole(h) => h.sketch_ids(),
            FeatureKind::Sweep(s) => s.sketches(),
            FeatureKind::Loft(l) => l.sketches(),
            _ => Vec::new(),
        }
    }

    /// The sketch regions it takes, and the sketches it takes whole.
    pub fn input_regions(&self) -> (&[RegionRef], &[FeatureId]) {
        match &self.kind {
            FeatureKind::Extrude(e) => (&e.regions, &e.sketches),
            FeatureKind::Revolve(r) => (&r.regions, &r.sketches),
            _ => (&[], &[]),
        }
    }

    /// The Fillet feature's parameters (P3.6).
    pub fn fillet(&self) -> Option<&crate::applied::FilletFeature> {
        match &self.kind {
            FeatureKind::Fillet(x) => Some(x),
            _ => None,
        }
    }

    /// The Chamfer feature's parameters (P3.6).
    pub fn chamfer(&self) -> Option<&crate::applied::ChamferFeature> {
        match &self.kind {
            FeatureKind::Chamfer(x) => Some(x),
            _ => None,
        }
    }

    /// The Shell feature's parameters (P3.6).
    pub fn shell(&self) -> Option<&crate::applied::ShellFeature> {
        match &self.kind {
            FeatureKind::Shell(x) => Some(x),
            _ => None,
        }
    }

    /// The Hole feature's parameters (P3.6).
    pub fn hole(&self) -> Option<&crate::applied::HoleFeature> {
        match &self.kind {
            FeatureKind::Hole(x) => Some(x),
            _ => None,
        }
    }

    /// The Boolean feature's parameters.
    pub fn boolean(&self) -> Option<&BooleanFeature> {
        match &self.kind {
            FeatureKind::Boolean(b) => Some(b),
            _ => None,
        }
    }

    pub fn boolean_mut(&mut self) -> Option<&mut BooleanFeature> {
        match &mut self.kind {
            FeatureKind::Boolean(b) => Some(b),
            _ => None,
        }
    }

    /// True for features that make or change parts (everything but sketches and variables).
    pub fn is_part_feature(&self) -> bool {
        !matches!(self.kind, FeatureKind::Sketch(_) | FeatureKind::Variable(_))
    }

    /// A feature is valid when it can regenerate; an invalid one shows red in the feature
    /// list (a sketch without a plane, an extrude with nothing to extrude).
    pub fn is_valid(&self) -> bool {
        match &self.kind {
            FeatureKind::Sketch(s) => s.plane.is_some(),
            FeatureKind::Extrude(e) => e.problem().is_none(),
            FeatureKind::Revolve(r) => r.problem().is_none(),
            FeatureKind::Boolean(b) => b.problem().is_none(),
            FeatureKind::DeletePart(d) => !d.parts.is_empty(),
            FeatureKind::Fillet(x) => x.problem().is_none(),
            FeatureKind::Chamfer(x) => x.problem().is_none(),
            FeatureKind::Shell(x) => x.problem().is_none(),
            FeatureKind::Hole(x) => x.problem().is_none(),
            FeatureKind::Plane(x) => x.problem().is_none(),
            FeatureKind::Sweep(x) => x.problem().is_none(),
            FeatureKind::Loft(x) => x.problem().is_none(),
            FeatureKind::Split(x) => x.problem().is_none(),
            FeatureKind::MateConnector(x) => x.problem().is_none(),
            FeatureKind::Pattern(x) => x.problem().is_none(),
            FeatureKind::Mirror(x) => x.problem().is_none(),
            FeatureKind::Draft(x) => x.problem().is_none(),
            FeatureKind::Transform(x) => x.problem().is_none(),
            FeatureKind::Composite(x) => x.problem().is_none(),
            FeatureKind::Import(x) => x.problem().is_none(),
            FeatureKind::Derived(x) => x.problem().is_none(),
            FeatureKind::Thicken(x) => x.problem().is_none(),
            FeatureKind::Helix(x) => x.problem().is_none(),
            FeatureKind::Fill(x) => x.problem().is_none(),
            FeatureKind::Variable(x) => x.problem().is_none(),
        }
    }

    /// Why the parameters can't be built, if they can't (P3.6: the applied features).
    pub fn problem(&self) -> Option<&'static str> {
        match &self.kind {
            FeatureKind::Sketch(s) => s.plane.is_none().then_some("Select a sketch plane"),
            FeatureKind::Extrude(e) => e.problem(),
            FeatureKind::Revolve(r) => r.problem(),
            FeatureKind::Boolean(b) => b.problem(),
            FeatureKind::DeletePart(d) => d.parts.is_empty().then_some("Select parts to delete"),
            FeatureKind::Fillet(x) => x.problem(),
            FeatureKind::Chamfer(x) => x.problem(),
            FeatureKind::Shell(x) => x.problem(),
            FeatureKind::Hole(x) => x.problem(),
            FeatureKind::Plane(x) => x.problem(),
            FeatureKind::Sweep(x) => x.problem(),
            FeatureKind::Loft(x) => x.problem(),
            FeatureKind::Split(x) => x.problem(),
            FeatureKind::MateConnector(x) => x.problem(),
            FeatureKind::Pattern(x) => x.problem(),
            FeatureKind::Mirror(x) => x.problem(),
            FeatureKind::Draft(x) => x.problem(),
            FeatureKind::Transform(x) => x.problem(),
            FeatureKind::Composite(x) => x.problem(),
            FeatureKind::Import(x) => x.problem(),
            FeatureKind::Derived(x) => x.problem(),
            FeatureKind::Thicken(x) => x.problem(),
            FeatureKind::Helix(x) => x.problem(),
            FeatureKind::Fill(x) => x.problem(),
            FeatureKind::Variable(x) => x.problem(),
        }
    }

    /// The features this one refers to (PS11: its parents): the sketches it takes regions,
    /// curves or points of, the sketch or feature its faces, edges and vertices come from, and
    /// the features that made the parts it names. A parent below it in the list makes it fail
    /// (PS11.3).
    pub fn parents(&self) -> Vec<FeatureId> {
        let mut out: Vec<FeatureId> = Vec::new();
        let mut add = |f: FeatureId| {
            if f != self.id && !out.contains(&f) {
                out.push(f);
            }
        };
        let face = |r: &FaceRef| FeatureId(r.face.op);
        // P3.11 (PS11.2): an edge depends on the features of both its faces (a fillet along
        // the edge between the plate and another fillet's face has both as parents).
        let edge = |r: &EdgeRef| [FeatureId(r.edge.faces[0].op), FeatureId(r.edge.faces[1].op)];
        match &self.kind {
            FeatureKind::Sketch(_) => {}
            FeatureKind::Extrude(e) => {
                e.sketches().into_iter().for_each(&mut add);
                e.faces.iter().map(face).for_each(&mut add);
                e.merge_scope.iter().for_each(|p| add(p.feature));
                match &e.up_to {
                    Some(UpTo::Face(f)) => add(face(f)),
                    Some(UpTo::Part(p)) => add(p.feature),
                    Some(UpTo::Vertex(v)) => add(v.part.feature),
                    None => {}
                }
            }
            FeatureKind::Revolve(r) => {
                r.sketches().into_iter().for_each(&mut add);
                r.faces.iter().map(face).for_each(&mut add);
                r.merge_scope.iter().for_each(|p| add(p.feature));
                match &r.axis {
                    Some(AxisRef::SketchCurve { sketch, .. }) => add(*sketch),
                    Some(AxisRef::Edge(e)) => edge(e).into_iter().for_each(&mut add),
                    Some(AxisRef::Face(f)) => add(face(f)),
                    Some(AxisRef::Connector(c)) => c.parent().into_iter().for_each(&mut add),
                    None => {}
                }
            }
            FeatureKind::Boolean(b) => {
                b.tools.iter().chain(&b.targets).for_each(|p| add(p.feature));
                if let Some(o) = &b.offset {
                    o.faces.iter().map(face).for_each(&mut add);
                }
            }
            FeatureKind::DeletePart(d) => d.parts.iter().for_each(|p| add(p.feature)),
            FeatureKind::Fillet(x) => {
                x.entities.iter().flat_map(|e| e.ops()).for_each(|o| add(FeatureId(o)));
                x.side1.iter().chain(&x.center).chain(&x.side2).map(face).for_each(&mut add);
            }
            FeatureKind::Chamfer(x) => x.entities.iter().flat_map(|e| e.ops()).for_each(|o| add(FeatureId(o))),
            FeatureKind::Shell(x) => {
                x.faces.iter().map(face).for_each(&mut add);
                x.parts.iter().for_each(|p| add(p.feature));
            }
            FeatureKind::Hole(x) => {
                x.sketch_ids().into_iter().for_each(&mut add);
                x.merge_scope.iter().for_each(|p| add(p.feature));
                x.connectors.iter().filter_map(|c| c.parent()).for_each(&mut add);
                for p in [&x.start_plane, &x.up_to].into_iter().flatten() {
                    match p {
                        crate::pattern::MirrorPlane::Plane(PlaneRef::Feature(f)) => add(FeatureId(f.feature)),
                        crate::pattern::MirrorPlane::Face(f) => add(face(f)),
                        crate::pattern::MirrorPlane::Connector(c) => c.parent().into_iter().for_each(&mut add),
                        _ => {}
                    }
                }
            }
            FeatureKind::Plane(x) => x.entities.iter().filter_map(|e| e.feature()).for_each(&mut add),
            FeatureKind::Sweep(x) => {
                x.sketches().into_iter().for_each(&mut add);
                x.faces.iter().map(face).for_each(&mut add);
                x.merge_scope.iter().for_each(|p| add(p.feature));
                for p in &x.path {
                    match p {
                        crate::advanced::PathRef::Edge(e) => edge(e).into_iter().for_each(&mut add),
                        crate::advanced::PathRef::Curve(f) => add(*f),
                        _ => {}
                    }
                }
                if let Some(d) = &x.lock_direction {
                    direction_parent(d).into_iter().for_each(&mut add);
                }
            }
            FeatureKind::Loft(x) => {
                x.sketches().into_iter().for_each(&mut add);
                x.merge_scope.iter().for_each(|p| add(p.feature));
                for d in [&x.start_direction, &x.end_direction].into_iter().flatten() {
                    direction_parent(d).into_iter().for_each(&mut add);
                }
                for p in &x.profiles {
                    match p {
                        crate::advanced::LoftProfile::Face(f) => add(face(f)),
                        crate::advanced::LoftProfile::Vertex(v) => add(v.part.feature),
                        _ => {}
                    }
                }
            }
            FeatureKind::Split(x) => {
                x.parts.iter().for_each(|p| add(p.feature));
                match &x.tool {
                    Some(crate::advanced::SplitToolRef::Plane(PlaneRef::Feature(f))) => add(FeatureId(f.feature)),
                    Some(crate::advanced::SplitToolRef::Face(f)) => add(face(f)),
                    Some(crate::advanced::SplitToolRef::Sketch(s)) => add(*s),
                    _ => {}
                }
            }
            FeatureKind::MateConnector(_)
            | FeatureKind::Pattern(_)
            | FeatureKind::Mirror(_)
            | FeatureKind::Draft(_)
            | FeatureKind::Transform(_)
            | FeatureKind::Import(_)
            | FeatureKind::Variable(_) => {}
            FeatureKind::Derived(x) => x.parents().into_iter().for_each(&mut add),
            // P3H.6: the features that made the members (context parts aren't features).
            FeatureKind::Composite(x) => x.parts.iter().filter(|p| !crate::assembly::context::is_context(p.feature)).for_each(|p| add(p.feature)),
            FeatureKind::Thicken(x) => x.parents().into_iter().for_each(&mut add),
            FeatureKind::Helix(x) => x.parents().into_iter().for_each(&mut add),
            FeatureKind::Fill(x) => x.parents().into_iter().for_each(&mut add),
        }
        match &self.kind {
            FeatureKind::MateConnector(x) => {
                x.parents().into_iter().for_each(&mut add);
                x.alignment.iter().filter_map(direction_parent).for_each(&mut add);
                if x.owner_on {
                    x.owner.iter().for_each(|p| add(p.feature));
                }
            }
            FeatureKind::Pattern(x) => {
                x.parts.iter().for_each(|p| add(p.feature));
                x.features.iter().for_each(|f| add(*f));
                x.faces.iter().map(face).for_each(&mut add);
                x.merge_scope.iter().for_each(|p| add(p.feature));
                for d in [&x.first.direction, &x.second.direction].into_iter().flatten() {
                    direction_parent(d).into_iter().for_each(&mut add);
                }
                match &x.axis {
                    Some(AxisRef::SketchCurve { sketch, .. }) => add(*sketch),
                    Some(AxisRef::Edge(e)) => edge(e).into_iter().for_each(&mut add),
                    Some(AxisRef::Face(f)) => add(face(f)),
                    Some(AxisRef::Connector(c)) => c.parent().into_iter().for_each(&mut add),
                    None => {}
                }
                for p in &x.path {
                    match p {
                        crate::advanced::PathRef::Edge(e) => edge(e).into_iter().for_each(&mut add),
                        crate::advanced::PathRef::SketchCurve { sketch, .. } | crate::advanced::PathRef::Sketch(sketch) => add(*sketch),
                        crate::advanced::PathRef::Curve(f) => add(*f),
                    }
                }
            }
            FeatureKind::Mirror(x) => {
                x.parts.iter().for_each(|p| add(p.feature));
                x.features.iter().for_each(|f| add(*f));
                x.faces.iter().map(face).for_each(&mut add);
                x.merge_scope.iter().for_each(|p| add(p.feature));
                match &x.plane {
                    Some(crate::pattern::MirrorPlane::Plane(PlaneRef::Feature(f))) => add(FeatureId(f.feature)),
                    Some(crate::pattern::MirrorPlane::Face(f)) => add(face(f)),
                    Some(crate::pattern::MirrorPlane::Connector(c)) => c.parent().into_iter().for_each(&mut add),
                    _ => {}
                }
            }
            FeatureKind::Draft(x) => {
                x.faces.iter().map(face).for_each(&mut add);
                match &x.neutral {
                    Some(crate::pattern::MirrorPlane::Plane(PlaneRef::Feature(f))) => add(FeatureId(f.feature)),
                    Some(crate::pattern::MirrorPlane::Face(f)) => add(face(f)),
                    Some(crate::pattern::MirrorPlane::Connector(c)) => c.parent().into_iter().for_each(&mut add),
                    _ => {}
                }
            }
            FeatureKind::Transform(x) => {
                x.parts.iter().for_each(|p| add(p.feature));
                for d in [&x.line, &x.direction].into_iter().flatten() {
                    direction_parent(d).into_iter().for_each(&mut add);
                }
                match &x.axis {
                    Some(AxisRef::SketchCurve { sketch, .. }) => add(*sketch),
                    Some(AxisRef::Edge(e)) => edge(e).into_iter().for_each(&mut add),
                    Some(AxisRef::Face(f)) => add(face(f)),
                    Some(AxisRef::Connector(c)) => c.parent().into_iter().for_each(&mut add),
                    None => {}
                }
                for c in [&x.from, &x.to, &x.scale_point].into_iter().flatten() {
                    c.parent().into_iter().for_each(&mut add);
                }
            }
            _ => {}
        }
        if let FeatureKind::Extrude(e) = &self.kind
            && let Some(d) = &e.direction
        {
            direction_parent(d).into_iter().for_each(&mut add);
        }
        out
    }
}

// A sketch is the common feature, so it is kept inline rather than boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    Sketch(SketchFeature),
    /// An extrusion of sketch regions or faces (M9; complete in P3.3).
    Extrude(ExtrudeFeature),
    /// Union, Subtract or Intersect of parts (P3.3, PS5.5).
    Boolean(BooleanFeature),
    /// A revolution of sketch regions or curves about an axis (P3.4, PS7).
    Revolve(RevolveFeature),
    /// Parts deleted from the Parts list (P3.3).
    DeletePart(DeletePartFeature),
    /// Rounds edges (P3.6, PS14).
    Fillet(crate::applied::FilletFeature),
    /// Bevels edges (P3.6, PS14.7).
    Chamfer(crate::applied::ChamferFeature),
    /// Hollows parts (P3.6, PS16).
    Shell(crate::applied::ShellFeature),
    /// Holes at sketch points (P3.6, PS15).
    Hole(crate::applied::HoleFeature),
    /// A reference plane (P3.7, PS12).
    Plane(crate::plane::PlaneFeature),
    /// A profile swept along a path (P3.7, PS19).
    Sweep(crate::advanced::SweepFeature),
    /// A body through ordered profiles (P3.7, PS20).
    Loft(crate::advanced::LoftFeature),
    /// Parts split in pieces (P3.7, PS18.5).
    Split(crate::advanced::SplitFeature),
    /// A mate connector (P3.8, X11).
    MateConnector(crate::mate::MateConnectorFeature),
    /// A Linear, Circular or Curve pattern (P3.8, PS22–PS25).
    Pattern(crate::pattern::PatternFeature),
    /// A mirror (P3.8, PS26).
    Mirror(crate::pattern::MirrorFeature),
    /// Faces drafted about a neutral plane (P3.10, PS4.9).
    Draft(crate::draft::DraftFeature),
    /// Parts, sketches, planes and mate connectors of another Part Studio (P3G.4, DV3; the
    /// Onshape importer's `importDerived` too).
    Derived(Box<crate::derived::DerivedFeature>),
    /// Parts moved, turned, scaled or copied (Onshape's Transform).
    Transform(crate::transform::TransformFeature),
    /// Parts made from a CAD file stored with the document: STEP, IGES or STL (Onshape import;
    /// P3F.2, T8, X5; [`crate::import`]).
    Import(crate::import::ImportFeature),
    /// Parts grouped into a composite part (P3H.6, PCB7.9, [`crate::transform`]).
    Composite(crate::transform::CompositeFeature),
    /// Surfaces and faces made solid with a thickness (Onshape's Thicken).
    Thicken(crate::surfacing::ThickenFeature),
    /// A helical curve (Onshape's Helix), usable as a sweep path.
    Helix(crate::surfacing::HelixFeature),
    /// A surface filling a closed boundary (Onshape's Fill).
    Fill(crate::surfacing::FillFeature),
    /// A variable, `#name = expression` (P3F.4, P5.2; [`crate::variables`]).
    Variable(crate::variables::VariableFeature),
}

/// A closed region of a sketch, as an extrude refers to it: the sketch, the curves on its outer
/// boundary, and a point inside it (used when the curves no longer match exactly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionRef {
    pub sketch: FeatureId,
    pub curves: Vec<CurveId>,
    pub seed: Vec2,
}

impl RegionRef {
    /// A reference to `region` of `sketch`.
    pub fn new(sketch: FeatureId, region: &Region) -> Self {
        let mut curves = region.curves.clone();
        curves.sort();
        curves.dedup();
        Self {
            sketch,
            curves,
            seed: interior_point(region),
        }
    }

    /// A stable key for the region (P3.2): the faces an extrude makes from it are named after
    /// it ([`cadrs_sketch::FaceOrigin`]). It depends on the sketch and the boundary curves the
    /// extrude selected, not on the region's place in the extrude's list, so adding, removing
    /// or reordering other regions leaves it alone.
    pub fn key(&self) -> u64 {
        use slotmap::Key;
        let mut bytes = self.sketch.0.as_bytes().to_vec();
        for c in &self.curves {
            bytes.extend_from_slice(&c.data().as_ffi().to_le_bytes());
        }
        cadrs_kernel::naming::stable_hash(&bytes)
    }

    /// The region of `geometry` this refers to: the one with the same boundary curves (the one
    /// around the seed point if several share them, like the two halves of a circle a line
    /// cuts), else the smallest one containing the seed point.
    pub fn resolve(&self, geometry: &Sketch) -> Option<Region> {
        let all = cadrs_sketch::region::regions_shared(geometry);
        let same = |r: &&Region| {
            let mut c = r.curves.clone();
            c.sort();
            c.dedup();
            c == self.curves
        };
        let same_curves: Vec<&Region> = all.iter().filter(same).collect();
        if let Some(r) = same_curves
            .iter()
            .find(|r| r.contains(self.seed))
            .or(same_curves.first())
        {
            return Some((*r).clone());
        }
        // P3D.4 (IR6.10): never guess. A sketch redrawn from scratch (none of the boundary
        // curves left) has lost the region, even if a new one covers the seed point.
        let kept = |c: &CurveId| geometry.curves.contains_key(*c) || cadrs_sketch::is_synthetic(*c);
        if !self.curves.is_empty() && !self.curves.iter().any(kept) {
            return None;
        }
        all.iter()
            .filter(|r| r.contains(self.seed))
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .cloned()
    }
}

/// A point inside a region (the middle of its largest triangle, which is inside even for
/// concave shapes and holes).
pub fn interior_point(r: &Region) -> Vec2 {
    let (verts, idx) = r.triangulate();
    let mut best = (0.0, r.outer.first().copied().unwrap_or(Vec2::ZERO));
    for t in idx.chunks(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| verts[i as usize]);
        let area = ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.0;
        if area > best.0 {
            best = (area, Vec2::new((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0));
        }
    }
    best.1
}

/// How an extrude ends (PS4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EndType {
    /// A given depth.
    #[default]
    Blind,
    /// Up to the next part faces in the way.
    UpToNext,
    /// Up to a picked face.
    UpToFace,
    /// Up to a picked part.
    UpToPart,
    /// Up to the height of a picked vertex.
    UpToVertex,
    /// Through every part.
    ThroughAll,
}

impl EndType {
    /// Every end type, in the dialog's order.
    pub const ALL: [EndType; 6] = [
        EndType::Blind,
        EndType::UpToNext,
        EndType::UpToFace,
        EndType::UpToPart,
        EndType::UpToVertex,
        EndType::ThroughAll,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EndType::Blind => "Blind",
            EndType::UpToNext => "Up to next",
            EndType::UpToFace => "Up to face",
            EndType::UpToPart => "Up to part",
            EndType::UpToVertex => "Up to vertex",
            EndType::ThroughAll => "Through all",
        }
    }

    /// The "Up to" family, which has an offset option.
    pub fn is_up_to(self) -> bool {
        matches!(
            self,
            EndType::UpToNext | EndType::UpToFace | EndType::UpToPart | EndType::UpToVertex
        )
    }

    /// End types that need a picked face, part or vertex.
    pub fn needs_target(self) -> bool {
        matches!(self, EndType::UpToFace | EndType::UpToPart | EndType::UpToVertex)
    }
}

/// What an extrude creates (PS4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BodyType {
    #[default]
    Solid,
    /// Sheets swept by the sketch curves (PS4.10).
    Surface,
    /// A wall along the sketch curves (PS4.11).
    Thin,
}

/// How the result combines with existing parts (PS5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BooleanOp {
    /// A new part.
    #[default]
    New,
    /// Joined with the parts in its merge scope.
    Add,
    /// Cut from them.
    Remove,
    /// Only the overlap with them is kept.
    Intersect,
}

impl BooleanOp {
    pub const ALL: [BooleanOp; 4] = [BooleanOp::New, BooleanOp::Add, BooleanOp::Remove, BooleanOp::Intersect];

    pub fn label(self) -> &'static str {
        match self {
            BooleanOp::New => "New",
            BooleanOp::Add => "Add",
            BooleanOp::Remove => "Remove",
            BooleanOp::Intersect => "Intersect",
        }
    }
}

/// A face of a part, as a feature refers to it (an extrude's input or its Up to face): the part,
/// the face's persistent name, and a point on the face for the geometric fallback.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FaceRef {
    pub part: PartId,
    pub face: FaceName,
    pub seed: Vec3,
}

/// A vertex of a part (Up to vertex) and where it was.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VertexRef {
    pub part: PartId,
    pub vertex: VertexName,
    pub point: Vec3,
}

/// An edge of a part (a Direction) and a point on it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub part: PartId,
    pub edge: EdgeName,
    pub seed: Vec3,
}

/// What an "Up to" end goes to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum UpTo {
    Face(FaceRef),
    Part(PartId),
    Vertex(VertexRef),
}

/// A typed distance with its own flip arrow: the Offset distance of an "Up to" end, or the
/// Starting offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offset {
    /// mm.
    pub value: f64,
    /// As typed ("5 mm").
    pub expr: String,
    /// The opposite direction: an end offset goes past its target instead of stopping short; a
    /// starting offset goes against the extrude direction.
    #[serde(default)]
    pub flip: bool,
}

impl Default for Offset {
    fn default() -> Self {
        Self {
            value: 5.0,
            expr: "5 mm".into(),
            flip: false,
        }
    }
}

impl Offset {
    /// The offset along its positive direction (negative when flipped).
    pub fn signed(&self) -> f64 {
        if self.flip { -self.value } else { self.value }
    }
}

/// The direction to extrude along instead of the sketch normal (PS4.6).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DirectionRef {
    /// A straight edge of a part.
    Edge(EdgeRef),
    /// A sketch line.
    SketchLine { sketch: FeatureId, curve: CurveId },
    /// A planar face's normal.
    FaceNormal(FaceRef),
    /// The normal of a default plane or a Plane feature (P3.7, PS12.3: planes as directions).
    PlaneNormal(PlaneRef),
    /// A mate connector's Z axis (P3.8, X11).
    Connector(crate::mate::ConnectorRef),
}

/// The feature a direction refers to (the part's, the sketch's or the Plane feature).
pub fn direction_parent(d: &DirectionRef) -> Option<FeatureId> {
    match d {
        DirectionRef::Edge(e) => Some(FeatureId(e.edge.op())),
        DirectionRef::SketchLine { sketch, .. } => Some(*sketch),
        DirectionRef::FaceNormal(f) => Some(FeatureId(f.face.op)),
        DirectionRef::PlaneNormal(PlaneRef::Feature(f)) => Some(FeatureId(f.feature)),
        DirectionRef::PlaneNormal(_) => None,
        DirectionRef::Connector(c) => c.parent(),
    }
}

/// One end of an extrude: its type, depth, target and offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndCondition {
    pub end: EndType,
    /// mm (Blind).
    pub depth: f64,
    pub depth_expr: String,
    /// The face, part or vertex of an "Up to" end.
    #[serde(default)]
    pub up_to: Option<UpTo>,
    /// The "Offset distance" option of an "Up to" end.
    #[serde(default)]
    pub offset: Option<Offset>,
}

impl Default for EndCondition {
    fn default() -> Self {
        Self {
            end: EndType::Blind,
            depth: DEFAULT_DEPTH,
            depth_expr: "25 mm".into(),
            up_to: None,
            offset: None,
        }
    }
}

/// The Thin tab's wall (PS4.11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinWall {
    /// mm, on the inside of closed profiles (the left of open ones).
    pub thickness1: f64,
    pub thickness1_expr: String,
    /// mm, on the other side.
    pub thickness2: f64,
    pub thickness2_expr: String,
    /// Swaps the two sides.
    pub flip_wall: bool,
    /// Thickness 1 split evenly across the curves.
    pub mid_plane: bool,
}

impl Default for ThinWall {
    fn default() -> Self {
        Self {
            thickness1: 5.0,
            thickness1_expr: "5 mm".into(),
            thickness2: 0.0,
            thickness2_expr: "0 mm".into(),
            flip_wall: false,
            mid_plane: false,
        }
    }
}

impl ThinWall {
    /// The wall to the left and to the right of the curves (mm).
    pub fn sides(&self) -> (f64, f64) {
        let (l, r) = if self.mid_plane {
            (self.thickness1 / 2.0, self.thickness1 / 2.0)
        } else {
            (self.thickness1, self.thickness2)
        };
        if self.flip_wall { (r, l) } else { (l, r) }
    }
}

/// An extrude feature's parameters (the Extrude dialog, `reference/onshape/screens/22`,
/// `training/intro-to-part-studios/ex1-step3.png`, `ex1-step4.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeFeature {
    /// "Faces and sketch regions to extrude": sketch regions...
    pub regions: Vec<RegionRef>,
    /// ...whole sketches, picked in the feature list (PS1.1: the outer boundary less the loops
    /// inside it)...
    #[serde(default)]
    pub sketches: Vec<FeatureId>,
    /// ...and planar part faces (PS4.2).
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    #[serde(default)]
    pub body: BodyType,
    #[serde(default)]
    pub thin: ThinWall,
    #[serde(default)]
    pub op: BooleanOp,
    /// "Merge with all" (PS5.4): the boolean acts on every part.
    #[serde(default)]
    pub merge_all: bool,
    /// The parts the boolean acts on; empty: the parts the new body touches (Add) or overlaps
    /// (Remove, Intersect).
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
    /// The first end: its type...
    #[serde(default)]
    pub end: EndType,
    /// ...its depth in mm (Blind; the total depth when symmetric)...
    pub depth: f64,
    /// ...as typed ("25 mm", "20 + 5")...
    pub depth_expr: String,
    /// ...its target (Up to face, part, vertex)...
    #[serde(default)]
    pub up_to: Option<UpTo>,
    /// ...and its offset distance.
    #[serde(default)]
    pub offset: Option<Offset>,
    /// Extrude against the sketch plane's normal (or the picked direction).
    pub flip: bool,
    /// Symmetric (PS4.7).
    #[serde(default)]
    pub symmetric: bool,
    /// Starting offset (PS4.5).
    #[serde(default)]
    pub start_offset: Option<Offset>,
    /// Direction (PS4.6).
    #[serde(default)]
    pub direction: Option<DirectionRef>,
    /// Second end position (PS4.8), against the extrude direction.
    #[serde(default)]
    pub second: Option<EndCondition>,
    /// Draft (P3.10, PS4.9): the side faces lean by an angle, each end away from the sketch
    /// plane. Solids only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<crate::draft::ExtrudeDraft>,
}

/// Onshape's default extrude depth.
pub const DEFAULT_DEPTH: f64 = 25.0;

impl Default for ExtrudeFeature {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            sketches: Vec::new(),
            faces: Vec::new(),
            body: BodyType::Solid,
            thin: ThinWall::default(),
            op: BooleanOp::New,
            merge_all: false,
            merge_scope: Vec::new(),
            end: EndType::Blind,
            depth: DEFAULT_DEPTH,
            depth_expr: "25 mm".into(),
            up_to: None,
            offset: None,
            flip: false,
            symmetric: false,
            start_offset: None,
            direction: None,
            second: None,
            draft: None,
        }
    }
}

impl ExtrudeFeature {
    /// The sketches it extrudes regions of (or whole).
    pub fn sketches(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = self.regions.iter().map(|r| r.sketch).collect();
        for s in &self.sketches {
            if !v.contains(s) {
                v.push(*s);
            }
        }
        v.dedup();
        v
    }

    /// True when nothing is selected to extrude.
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty() && self.sketches.is_empty() && self.faces.is_empty()
    }

    /// The first end as an [`EndCondition`].
    pub fn first_end(&self) -> EndCondition {
        EndCondition {
            end: self.end,
            depth: self.depth,
            depth_expr: self.depth_expr.clone(),
            up_to: self.up_to,
            offset: self.offset.clone(),
        }
    }

    /// Why the parameters can't be built, if they can't (a missing target, a zero depth).
    pub fn problem(&self) -> Option<&'static str> {
        if self.is_empty() {
            return Some("Select sketch regions to extrude");
        }
        let check = |e: EndType, depth: f64, up_to: &Option<UpTo>| -> Option<&'static str> {
            match e {
                EndType::Blind if !(depth > 0.0 && depth.is_finite()) => {
                    Some("The depth must be greater than zero")
                }
                EndType::UpToFace if !matches!(up_to, Some(UpTo::Face(_))) => Some("Select a face to extrude up to"),
                EndType::UpToPart if !matches!(up_to, Some(UpTo::Part(_))) => Some("Select a part to extrude up to"),
                EndType::UpToVertex if !matches!(up_to, Some(UpTo::Vertex(_))) => {
                    Some("Select a vertex to extrude up to")
                }
                _ => None,
            }
        };
        check(self.end, self.depth, &self.up_to).or_else(|| {
            self.second
                .as_ref()
                .filter(|_| !self.symmetric)
                .and_then(|s| check(s.end, s.depth, &s.up_to))
        })
    }
}

/// What a revolve turns about (PS7.2): a sketch line (a construction centreline) or circle,
/// a straight part edge, the axis of a circular part edge, or the axis of a cylindrical or
/// conical part face.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AxisRef {
    /// A line of a sketch (its direction from its first point to its second), or a circle or
    /// arc (its axis, through its center along the sketch normal).
    SketchCurve { sketch: FeatureId, curve: CurveId },
    /// A straight part edge, or a circular one (its axis).
    Edge(EdgeRef),
    /// A cylindrical, conical or other face of revolution (its axis).
    Face(FaceRef),
    /// A mate connector's Z axis (P3.8, PS7.2, PS24.1).
    Connector(crate::mate::ConnectorRef),
}

/// How a revolve ends (PS7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RevolveType {
    /// A whole turn.
    #[default]
    Full,
    /// A given angle, with a flip arrow.
    Blind,
    /// The angle split evenly across the profile.
    Symmetric,
    UpToNext,
    UpToFace,
    UpToPart,
    UpToVertex,
}

impl RevolveType {
    /// Every type, in the dialog's order.
    pub const ALL: [RevolveType; 7] = [
        RevolveType::Full,
        RevolveType::Blind,
        RevolveType::Symmetric,
        RevolveType::UpToNext,
        RevolveType::UpToFace,
        RevolveType::UpToPart,
        RevolveType::UpToVertex,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RevolveType::Full => "Full",
            RevolveType::Blind => "Blind",
            RevolveType::Symmetric => "Symmetric",
            RevolveType::UpToNext => "Up to next",
            RevolveType::UpToFace => "Up to face",
            RevolveType::UpToPart => "Up to part",
            RevolveType::UpToVertex => "Up to vertex",
        }
    }

    /// The extrude end type of the same name (Blind for Full and Symmetric).
    pub fn end_type(self) -> EndType {
        match self {
            RevolveType::Full | RevolveType::Blind | RevolveType::Symmetric => EndType::Blind,
            RevolveType::UpToNext => EndType::UpToNext,
            RevolveType::UpToFace => EndType::UpToFace,
            RevolveType::UpToPart => EndType::UpToPart,
            RevolveType::UpToVertex => EndType::UpToVertex,
        }
    }

    /// The type for an end type (Blind for Blind, the "Up to" types; Through all has none).
    pub fn of_end(end: EndType) -> Self {
        match end {
            EndType::UpToNext => RevolveType::UpToNext,
            EndType::UpToFace => RevolveType::UpToFace,
            EndType::UpToPart => RevolveType::UpToPart,
            EndType::UpToVertex => RevolveType::UpToVertex,
            EndType::Blind | EndType::ThroughAll => RevolveType::Blind,
        }
    }

    /// Types with an angle field.
    pub fn has_angle(self) -> bool {
        matches!(self, RevolveType::Blind | RevolveType::Symmetric)
    }

    /// Types with a flip arrow and a second end: all but Full and Symmetric.
    pub fn one_sided(self) -> bool {
        !matches!(self, RevolveType::Full | RevolveType::Symmetric)
    }
}

/// The end types a revolve's second end offers.
pub const REVOLVE_SECOND_ENDS: [EndType; 5] = [
    EndType::Blind,
    EndType::UpToNext,
    EndType::UpToFace,
    EndType::UpToPart,
    EndType::UpToVertex,
];

/// Onshape's default revolve angle for Blind and Symmetric.
pub const DEFAULT_REVOLVE_ANGLE: f64 = 90.0;

/// A revolve feature's parameters (the Revolve dialog, `training/intro-to-part-studios/
/// ex2-step5.png`). Angles are in degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevolveFeature {
    /// "Faces and sketch regions to revolve": sketch regions...
    pub regions: Vec<RegionRef>,
    /// ...whole sketches picked in the feature list (PS1.6)...
    #[serde(default)]
    pub sketches: Vec<FeatureId>,
    /// ...and planar part faces (PS7.1).
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    #[serde(default)]
    pub body: BodyType,
    #[serde(default)]
    pub thin: ThinWall,
    #[serde(default)]
    pub op: BooleanOp,
    #[serde(default)]
    pub merge_all: bool,
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
    /// The "Revolve axis" field.
    #[serde(default)]
    pub axis: Option<AxisRef>,
    /// The revolve type...
    #[serde(default)]
    pub kind: RevolveType,
    /// ...its angle in degrees (Blind; the total when Symmetric)...
    pub angle: f64,
    /// ...as typed ("90 deg").
    pub angle_expr: String,
    /// The face, part or vertex of an "Up to" type.
    #[serde(default)]
    pub up_to: Option<UpTo>,
    /// Its "Offset angle" option (degrees in `value`).
    #[serde(default)]
    pub offset: Option<Offset>,
    /// Turn the other way about the axis.
    #[serde(default)]
    pub flip: bool,
    /// Second end position: an end turning the other way (`depth` is its angle in degrees).
    #[serde(default)]
    pub second: Option<EndCondition>,
}

impl Default for RevolveFeature {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            sketches: Vec::new(),
            faces: Vec::new(),
            body: BodyType::Solid,
            thin: ThinWall::default(),
            op: BooleanOp::New,
            merge_all: false,
            merge_scope: Vec::new(),
            axis: None,
            kind: RevolveType::Full,
            angle: DEFAULT_REVOLVE_ANGLE,
            angle_expr: "90 deg".into(),
            up_to: None,
            offset: None,
            flip: false,
            second: None,
        }
    }
}

/// An angle offset or angle end as a revolve's default: "5 deg" (the value in degrees).
pub fn default_angle_offset() -> Offset {
    Offset {
        value: 5.0,
        expr: "5 deg".into(),
        flip: false,
    }
}

/// A revolve's second end as it starts: Blind 90°.
pub fn default_revolve_second() -> EndCondition {
    EndCondition {
        end: EndType::Blind,
        depth: DEFAULT_REVOLVE_ANGLE,
        depth_expr: "90 deg".into(),
        up_to: None,
        offset: None,
    }
}

impl RevolveFeature {
    /// The sketches it revolves regions of (or whole).
    pub fn sketches(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = self.regions.iter().map(|r| r.sketch).collect();
        for s in &self.sketches {
            if !v.contains(s) {
                v.push(*s);
            }
        }
        v.dedup();
        v
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty() && self.sketches.is_empty() && self.faces.is_empty()
    }

    /// Why the parameters can't be built, if they can't.
    pub fn problem(&self) -> Option<&'static str> {
        if self.is_empty() {
            return Some("Select sketch regions to revolve");
        }
        if self.axis.is_none() {
            return Some("Select a revolve axis");
        }
        let angle_ok = |a: f64| a > 0.0 && a <= 360.0 + 1e-9 && a.is_finite();
        let check = |e: EndType, angle: f64, up_to: &Option<UpTo>| -> Option<&'static str> {
            match e {
                EndType::Blind if !angle_ok(angle) => Some("The angle must be between 0 and 360 degrees"),
                EndType::UpToFace if !matches!(up_to, Some(UpTo::Face(_))) => Some("Select a face to revolve up to"),
                EndType::UpToPart if !matches!(up_to, Some(UpTo::Part(_))) => Some("Select a part to revolve up to"),
                EndType::UpToVertex if !matches!(up_to, Some(UpTo::Vertex(_))) => {
                    Some("Select a vertex to revolve up to")
                }
                _ => None,
            }
        };
        if self.kind == RevolveType::Full {
            return None;
        }
        check(self.kind.end_type(), self.angle, &self.up_to).or_else(|| {
            self.second
                .as_ref()
                .filter(|_| self.kind.one_sided())
                .and_then(|s| check(s.end, s.depth, &s.up_to))
        })
    }
}

/// The standalone Boolean feature's operation (PS5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BooleanKind {
    #[default]
    Union,
    Subtract,
    Intersect,
}

impl BooleanKind {
    pub const ALL: [BooleanKind; 3] = [BooleanKind::Union, BooleanKind::Subtract, BooleanKind::Intersect];

    pub fn label(self) -> &'static str {
        match self {
            BooleanKind::Union => "Union",
            BooleanKind::Subtract => "Subtract",
            BooleanKind::Intersect => "Intersect",
        }
    }
}

/// The Boolean feature (PS5.5): Union, Subtract or Intersect of parts. The result keeps the
/// identity (name, and later the properties) of the first tool (Union, Intersect) or of each
/// target (Subtract). The tools are used up unless "Keep tools" is on.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BooleanFeature {
    pub op: BooleanKind,
    /// The tools, in the order picked.
    pub tools: Vec<PartId>,
    /// Subtract: the parts to cut.
    #[serde(default)]
    pub targets: Vec<PartId>,
    #[serde(default)]
    pub keep_tools: bool,
    /// (P3.10, PS5.5) Subtract's Offset: the tools grown (or, flipped, shrunk) before they cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<BooleanOffset>,
}

/// The Boolean feature's Subtract offset (P3.10, PS5.5): every face of the tools (Offset all)
/// or only the picked faces moved by the distance, edges kept sharp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BooleanOffset {
    /// mm, greater than zero.
    pub distance: f64,
    pub expr: String,
    /// Opposite direction: the faces move inward.
    #[serde(default)]
    pub flip: bool,
    /// Offset all the tools' faces (on by default); off: only `faces`.
    #[serde(default = "yes_default")]
    pub all: bool,
    #[serde(default)]
    pub faces: Vec<FaceRef>,
}

fn yes_default() -> bool {
    true
}

impl Default for BooleanOffset {
    fn default() -> Self {
        Self { distance: 1.0, expr: "1 mm".into(), flip: false, all: true, faces: Vec::new() }
    }
}

impl BooleanOffset {
    /// The signed distance (outward positive).
    pub fn signed(&self) -> f64 {
        if self.flip { -self.distance } else { self.distance }
    }
}

impl BooleanFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if let Some(o) = &self.offset
            && self.op == BooleanKind::Subtract
        {
            if !(o.distance > 0.0 && o.distance.is_finite()) {
                return Some("The offset distance must be greater than zero");
            }
            if !o.all && o.faces.is_empty() {
                return Some("Select the faces to offset");
            }
        }
        match self.op {
            BooleanKind::Subtract if self.targets.is_empty() => Some("Select the parts to subtract from"),
            BooleanKind::Subtract if self.tools.is_empty() => Some("Select the parts to subtract"),
            BooleanKind::Union | BooleanKind::Intersect if self.tools.len() < 2 => {
                Some("Select at least two parts")
            }
            _ => None,
        }
    }
}

/// Deleting parts from the Parts list adds this feature (Onshape's "Delete part").
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DeletePartFeature {
    pub parts: Vec<PartId>,
}

/// A part's own settings in a Part Studio (the Parts list's context menu, PS2.8): its name,
/// whether it is hidden, its appearance and its faces' (PS9, P3.5) and its material (PS10).
///
/// P3C.5 retired the P3C.3 `description` field: a file that still has one reads it into
/// [`crate::properties::Properties::description`] (unless that is set), see [`PartPropsFile`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "PartPropsFile")]
pub struct PartProps {
    pub part: PartId,
    /// A name given with Rename (else "Part N").
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    /// Its own appearance (else its palette colour).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<crate::appearance::Appearance>,
    /// Appearances of single faces (Add appearance to face).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<(FaceName, crate::appearance::Appearance)>,
    /// Its material (Assign material).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<crate::material::Material>,
    /// Part number, Description and the other properties (P3B.6, [`crate::properties`]).
    #[serde(default, skip_serializing_if = "crate::properties::Properties::is_empty")]
    pub properties: crate::properties::Properties,
}

/// [`PartProps`] as files store it, with the P3C.3 `description` field older files have.
#[derive(Deserialize)]
struct PartPropsFile {
    part: PartId,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    appearance: Option<crate::appearance::Appearance>,
    #[serde(default)]
    faces: Vec<(FaceName, crate::appearance::Appearance)>,
    #[serde(default)]
    material: Option<crate::material::Material>,
    /// P3C.3's description, now the Description property.
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    properties: crate::properties::Properties,
}

impl From<PartPropsFile> for PartProps {
    fn from(f: PartPropsFile) -> Self {
        let mut properties = f.properties;
        if properties.description.is_none() {
            properties.description = f.description.filter(|d| !d.trim().is_empty());
        }
        Self { part: f.part, name: f.name, hidden: f.hidden, appearance: f.appearance, faces: f.faces, material: f.material, properties }
    }
}

impl PartProps {
    pub fn new(part: PartId) -> Self {
        Self {
            part,
            name: None,
            hidden: false,
            appearance: None,
            faces: Vec::new(),
            material: None,
            properties: Default::default(),
        }
    }

    /// True when nothing is set (the entry can go).
    pub fn is_default(&self) -> bool {
        self.name.is_none()
            && !self.hidden
            && self.appearance.is_none()
            && self.faces.is_empty()
            && self.material.is_none()
            && self.properties.is_empty()
    }
}

/// A sketch feature: its parameters (the plane and the dialog's options) and its geometry.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SketchFeature {
    /// The sketch plane; `None` while the dialog waits for one (the feature is invalid).
    pub plane: Option<PlaneRef>,
    /// The dialog's "Disable imprinting" option.
    #[serde(default)]
    pub disable_imprinting: bool,
    /// The geometry, in the plane's coordinates.
    pub geometry: Sketch,
}

impl SketchFeature {
    pub fn new(plane: Option<PlaneRef>) -> Self {
        Self {
            plane,
            ..Self::default()
        }
    }
}
