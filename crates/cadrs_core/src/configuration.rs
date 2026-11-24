//! Configurations of a Part Studio (the node view's **Configuration** node): named **inputs**
//! (a length, an angle, a number or a boolean) and **presets** that give each input a value, so
//! one studio makes several variations of its parts.
//!
//! - **Inputs are variables.** The active preset's values are defined above the whole feature
//!   list, like a Variable Studio's ([`crate::document::Element::variable_base`]), so a feature
//!   field reads one as `#length` and a feature is suppressed by one with
//!   [`crate::variables::SuppressByVariable`] (`#withHole`). A rebuild sees them as Variable
//!   features ahead of the list ([`Configuration::features`]), with ids derived from the
//!   input's so the rebuild cache keeps its entries across presets.
//! - **The node view's wires are those expressions**: a field whose text is `#length` is wired
//!   from the input `length`. Connecting a wire writes the expression ([`ConnectParameter`]),
//!   disconnecting writes the value back ([`DisconnectParameter`]).
//! - **Presets in assemblies.** An instance of one preset is an instance of a generated Part
//!   Studio ([`ConfiguredStudio`]), as standard content is: a copy of the studio with the
//!   preset's values, under an id derived from the studio's and the preset's
//!   ([`configured_id`]), found by [`crate::document::Document::element`]. Only the presets an
//!   assembly uses are generated; [`refresh`] keeps them in step with their studio after every
//!   edit. A copy doesn't regenerate its sketches on faces: they are as in the studio's active
//!   preset.

use std::collections::{BTreeMap, BTreeSet};

use cadrs_sketch::units::{Quantity, VarValue};
use cadrs_vars::{VariableFeature, VariableKind};
use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind, Feature, FeatureKind};
use crate::ids::{ElementId, FeatureId, InputId, PresetId};

// ---------------------------------------------------------------------------------------------
// The model

/// What an input's value measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum InputKind {
    /// Millimetres.
    #[default]
    Length,
    /// Degrees.
    Angle,
    /// A plain number (a count, a ratio).
    Number,
    /// 0 (false) or 1 (true): for suppression (a feature's Bypass).
    Boolean,
}

impl InputKind {
    pub const ALL: [InputKind; 4] = [
        InputKind::Length,
        InputKind::Angle,
        InputKind::Number,
        InputKind::Boolean,
    ];

    pub fn label(self) -> &'static str {
        match self {
            InputKind::Length => "Length",
            InputKind::Angle => "Angle",
            InputKind::Number => "Number",
            InputKind::Boolean => "Boolean",
        }
    }

    /// The kind of variable expressions see.
    pub fn variable_kind(self) -> VariableKind {
        match self {
            InputKind::Length => VariableKind::Length,
            InputKind::Angle => VariableKind::Angle,
            InputKind::Number | InputKind::Boolean => VariableKind::Number,
        }
    }

    pub fn quantity(self) -> Quantity {
        self.variable_kind().quantity()
    }

    /// Whether a field of quantity `q` can be wired from it.
    pub fn fits(self, q: Quantity) -> bool {
        self != InputKind::Boolean && self.quantity() == q
    }
}

/// A configuration input: `#name`, with what it measures and its value where a preset gives
/// none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigInput {
    pub id: InputId,
    /// The name, without the `#`.
    pub name: String,
    #[serde(default)]
    pub kind: InputKind,
    /// mm, degrees, a number, or 0/1.
    #[serde(default)]
    pub default: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl ConfigInput {
    pub fn new(name: impl Into<String>, kind: InputKind, default: f64) -> Self {
        Self {
            id: InputId::new(),
            name: name.into(),
            kind,
            default,
            description: String::new(),
        }
    }

    /// The id of the Variable feature a rebuild sees for it ([`Configuration::features`]):
    /// derived from the input's, so it is the same in every preset and every rebuild.
    pub fn feature_id(&self) -> FeatureId {
        FeatureId::from_u128(fnv(b"cadrs configuration input\0", self.id.0.as_bytes()))
    }
}

/// A named set of input values (Onshape's configuration row).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: PresetId,
    pub name: String,
    /// The inputs it sets; the others take their default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<InputId, f64>,
}

impl Preset {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: PresetId::new(),
            name: name.into(),
            values: BTreeMap::new(),
        }
    }

    /// The value it gives `input`.
    pub fn value(&self, input: &ConfigInput) -> f64 {
        self.values.get(&input.id).copied().unwrap_or(input.default)
    }
}

/// A Part Studio's configuration: its inputs, its presets and the preset the studio shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Configuration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<ConfigInput>,
    pub presets: Vec<Preset>,
    pub active: PresetId,
}

impl Default for Configuration {
    /// One preset, "Default", and no inputs.
    fn default() -> Self {
        let p = Preset::new("Default");
        Self {
            inputs: Vec::new(),
            active: p.id,
            presets: vec![p],
        }
    }
}

impl Configuration {
    pub fn preset(&self, id: PresetId) -> Option<&Preset> {
        self.presets.iter().find(|p| p.id == id)
    }

    pub fn preset_mut(&mut self, id: PresetId) -> Option<&mut Preset> {
        self.presets.iter_mut().find(|p| p.id == id)
    }

    pub fn preset_named(&self, name: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.name == name)
    }

    /// The preset the studio shows (the first one if `active` names none).
    pub fn active_preset(&self) -> Option<&Preset> {
        self.preset(self.active).or_else(|| self.presets.first())
    }

    pub fn input(&self, id: InputId) -> Option<&ConfigInput> {
        self.inputs.iter().find(|i| i.id == id)
    }

    pub fn input_named(&self, name: &str) -> Option<&ConfigInput> {
        self.inputs.iter().find(|i| i.name == name)
    }

    /// The inputs as expressions see them with `preset`'s values (the active one's for `None`).
    pub fn values(&self, preset: Option<PresetId>) -> Vec<(String, VarValue)> {
        let p = match preset {
            Some(id) => self.preset(id),
            None => self.active_preset(),
        };
        self.inputs
            .iter()
            .map(|i| {
                let value = p.map_or(i.default, |p| p.value(i));
                (
                    i.name.clone(),
                    VarValue {
                        value,
                        quantity: i.kind.quantity(),
                    },
                )
            })
            .collect()
    }

    /// The Variable features a rebuild sees ahead of the list for the active preset: `#name`
    /// set to its exact value.
    pub fn features(&self) -> Vec<Feature> {
        let p = self.active_preset();
        self.inputs
            .iter()
            .map(|i| {
                let kind = i.kind.variable_kind();
                let value = p.map_or(i.default, |p| p.value(i));
                let var = VariableFeature {
                    name: i.name.clone(),
                    var_type: kind.var_type(),
                    expr: kind.literal(value),
                    value,
                    kind,
                    description: i.description.clone(),
                };
                Feature::new(
                    i.feature_id(),
                    format!("#{}", i.name),
                    FeatureKind::Variable(var),
                )
            })
            .collect()
    }

    /// Why `name` can't name a new input (or `input`, renamed), if it can't: `taken` are the
    /// other names in scope (the studio's Variables and its Variable Studios' variables).
    pub fn name_problem(
        &self,
        name: &str,
        input: Option<InputId>,
        taken: &[String],
    ) -> Option<String> {
        if name.is_empty() {
            return Some("Enter a name".into());
        }
        if !cadrs_sketch::units::is_variable_name(name) {
            return Some("A name is letters, digits and _ (starting with a letter)".into());
        }
        if self
            .inputs
            .iter()
            .any(|i| i.name == name && Some(i.id) != input)
        {
            return Some(format!("There is already an input #{name}"));
        }
        if taken.iter().any(|t| t == name) {
            return Some(format!("#{name} is already a variable"));
        }
        None
    }
}

/// Where the node view puts a Part Studio's nodes (graph coordinates, px at zoom 1), and which
/// are collapsed. Features without a position are laid out by the app.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeLayout {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub positions: BTreeMap<FeatureId, [f32; 2]>,
    /// The Configuration node's position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs_node: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub collapsed: BTreeSet<FeatureId>,
}

impl NodeLayout {
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty() && self.inputs_node.is_none() && self.collapsed.is_empty()
    }
}

/// The names a Part Studio's features define as Variables, and its Variable Studios'
/// variables: what an input can't be called.
pub fn taken_names(el: &Element) -> Vec<String> {
    let mut out: Vec<String> = el
        .features()
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Variable(v) => Some(v.name.clone()),
            _ => None,
        })
        .collect();
    out.extend(el.studio_variables.iter().map(|v| v.name.clone()));
    out
}

fn fnv(tag: &[u8], data: &[u8]) -> u128 {
    // FNV-1a, 128 bit (as standard content's ids).
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    for b in tag.iter().chain(data) {
        h ^= *b as u128;
        h = h.wrapping_mul(0x0000000001000000000000000000013B);
    }
    h
}

// ---------------------------------------------------------------------------------------------
// Presets in assemblies

/// A preset of a Part Studio as an assembly uses it: a generated copy of the studio with the
/// preset's values ([`generate`]). Not a tab: found by [`Document::element`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredStudio {
    pub studio: ElementId,
    pub preset: PresetId,
    pub element: Element,
}

/// The id the generated studio of `studio`'s `preset` has.
pub fn configured_id(studio: ElementId, preset: PresetId) -> ElementId {
    let mut bytes = studio.0.as_bytes().to_vec();
    bytes.extend_from_slice(preset.0.as_bytes());
    ElementId::from_u128(fnv(b"cadrs configured studio\0", &bytes))
}

/// The studio and preset a generated studio is of: one of the document's, generated or not
/// yet.
pub fn configured_of(doc: &Document, id: ElementId) -> Option<(ElementId, PresetId)> {
    if let Some(c) = doc.configured.iter().find(|c| c.element.id == id) {
        return Some((c.studio, c.preset));
    }
    doc.elements.iter().find_map(|el| {
        el.configuration()?
            .presets
            .iter()
            .find(|p| configured_id(el.id, p.id) == id)
            .map(|p| (el.id, p.id))
    })
}

/// The copy of `studio` with `preset`'s values: its expressions evaluated with them, the
/// features they (or Suppress) suppress left out, the rollback bar at the end. `None` if the
/// studio has no such preset.
pub fn generate(
    studio: &Element,
    preset: PresetId,
    units: &cadrs_sketch::units::Units,
) -> Option<Element> {
    let c = studio.configuration()?;
    let p = c.preset(preset)?;
    let mut el = studio.clone();
    el.id = configured_id(studio.id, preset);
    el.name = format!("{} ({})", studio.name, p.name);
    if let Some(c) = el.configuration_mut() {
        c.active = preset;
    }
    let base = el.variable_base();
    let manual = el.suppressed().to_vec();
    if let Some(fs) = el.features_mut() {
        crate::variables::refresh(&base, fs, &manual, units);
    }
    let gone = el.all_suppressed();
    if let ElementKind::PartStudio {
        features,
        suppressed,
        rollback,
        node_layout,
        ..
    } = &mut el.kind
    {
        features.retain(|f| !gone.contains(&f.id));
        for f in features.iter_mut() {
            f.suppress_by = None;
        }
        suppressed.clear();
        *rollback = None;
        *node_layout = None;
    }
    Some(el)
}

/// Generates the studio of the preset `id` names, if it is one that isn't generated yet (an
/// instance of it is about to be inserted). Whether `id` is now an element of the document.
pub fn ensure(doc: &mut Document, id: ElementId) -> bool {
    if doc.element(id).is_some() {
        return true;
    }
    let Some((studio, preset)) = configured_of(doc, id) else {
        return false;
    };
    let Some(el) = doc
        .elements
        .iter()
        .find(|e| e.id == studio)
        .and_then(|src| generate(src, preset, &doc.units))
    else {
        return false;
    };
    doc.configured.push(ConfiguredStudio {
        studio,
        preset,
        element: el,
    });
    true
}

/// Generates the presets the document's assemblies use (and drops the ones none uses), again
/// whenever their studio changed. Runs after every edit, undo and redo
/// ([`crate::derived::refresh`]) and when a document is opened.
pub fn refresh(doc: &mut Document) {
    let mut used: Vec<ElementId> = Vec::new();
    for el in &doc.elements {
        for i in &el.assembly.instances {
            let id = i.source.element();
            if !used.contains(&id) && doc.elements.iter().all(|e| e.id != id) {
                used.push(id);
            }
        }
    }
    if used.is_empty() && doc.configured.is_empty() {
        return;
    }
    let mut out: Vec<ConfiguredStudio> = Vec::new();
    for id in used {
        let Some((studio, preset)) = configured_of(doc, id) else {
            continue;
        };
        let Some(src) = doc.elements.iter().find(|e| e.id == studio) else {
            continue;
        };
        let Some(el) = generate(src, preset, &doc.units) else {
            continue;
        };
        out.push(ConfiguredStudio {
            studio,
            preset,
            element: el,
        });
    }
    if out != doc.configured {
        doc.configured = out;
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

fn studio_mut(doc: &mut Document, element: ElementId) -> Result<&mut Element, CommandError> {
    let el = doc
        .element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?;
    if !matches!(el.kind, ElementKind::PartStudio { .. }) {
        return Err(CommandError::Invalid(
            "configurations need a Part Studio".into(),
        ));
    }
    Ok(el)
}

fn config_mut(doc: &mut Document, element: ElementId) -> Result<&mut Configuration, CommandError> {
    studio_mut(doc, element)?
        .configuration_mut()
        .ok_or_else(|| CommandError::Invalid("the Part Studio has no configuration".into()))
}

/// Re-evaluates the studio with its (changed) configuration.
fn refresh_studio(doc: &mut Document, element: ElementId) {
    crate::commands::refresh_studio(doc, element);
}

/// Rewrites `#from` as `#to` in an expression (whole names only).
pub fn rename_in(expr: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    let mut rest = expr;
    while let Some(i) = rest.find('#') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let end = after
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        if &after[..end] == from {
            out.push('#');
            out.push_str(to);
        } else {
            out.push('#');
            out.push_str(&after[..end]);
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// Every expression of a feature list (feature fields, sketch dimensions, Variables and
/// suppression variables), rewritten by `f`.
fn rewrite_expressions(features: &mut [Feature], f: &dyn Fn(&str) -> String) {
    for feat in features.iter_mut() {
        if let Some(r) = &mut feat.suppress_by {
            r.expr = f(&r.expr);
        }
        match &mut feat.kind {
            FeatureKind::Sketch(sk) => {
                for (_, e) in sk.geometry.expressions.iter_mut() {
                    if e.contains('#') {
                        *e = f(e);
                    }
                }
            }
            FeatureKind::Variable(v) => v.expr = f(&v.expr),
            k => crate::variables::slots(k, &mut |s| {
                if s.expr.contains('#') {
                    *s.expr = f(s.expr);
                }
            }),
        }
    }
}

/// Gives a Part Studio a configuration (one preset, "Default"), or removes it (`false`): the
/// expressions that name its inputs keep their values as numbers.
#[derive(Debug, Clone)]
pub struct SetConfigured {
    pub element: ElementId,
    pub configured: bool,
}

impl Command for SetConfigured {
    fn label(&self) -> String {
        if self.configured {
            "Add configuration".into()
        } else {
            "Remove configuration".into()
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.element)?;
        match (self.configured, el.configuration().is_some()) {
            (true, false) => {
                if let ElementKind::PartStudio { configuration, .. } = &mut el.kind {
                    *configuration = Some(Box::default());
                }
            }
            (false, true) => {
                let inputs: Vec<InputId> = el
                    .configuration()
                    .map(|c| c.inputs.iter().map(|i| i.id).collect())
                    .unwrap_or_default();
                for i in inputs {
                    bake_input(el, i);
                }
                if let ElementKind::PartStudio { configuration, .. } = &mut el.kind {
                    *configuration = None;
                }
            }
            _ => return Ok(()),
        }
        refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Writes the value of every expression that names `input` in its place (the input is going
/// away): fields keep their values as numbers, suppression by it is removed.
fn bake_input(el: &mut Element, input: InputId) {
    let Some(i) = el.configuration().and_then(|c| c.input(input)).cloned() else {
        return;
    };
    let name = i.name.clone();
    let Some(fs) = el.features_mut() else { return };
    for feat in fs.iter_mut() {
        if feat
            .suppress_by
            .as_ref()
            .is_some_and(|r| cadrs_sketch::units::variable_names(&r.expr).contains(&name))
        {
            feat.suppress_by = None;
        }
        match &mut feat.kind {
            FeatureKind::Sketch(sk) => {
                // The dimensions keep their values (no longer driven by an expression).
                sk.geometry
                    .expressions
                    .retain(|(_, e)| !cadrs_sketch::units::variable_names(e).contains(&name));
            }
            FeatureKind::Variable(v) => {
                if cadrs_sketch::units::variable_names(&v.expr).contains(&name) {
                    v.expr = v.kind.literal(v.value);
                }
            }
            k => crate::variables::slots(k, &mut |s| {
                if cadrs_sketch::units::variable_names(s.expr).contains(&name) {
                    *s.expr = VariableKind::of(s.quantity).literal(*s.value);
                }
            }),
        }
    }
}

/// Adds an input (and a configuration, if the studio has none yet).
#[derive(Debug, Clone)]
pub struct AddConfigInput {
    pub element: ElementId,
    pub input: ConfigInput,
}

impl Command for AddConfigInput {
    fn label(&self) -> String {
        format!("Add input #{}", self.input.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.element)?;
        let taken = taken_names(el);
        if let ElementKind::PartStudio { configuration, .. } = &mut el.kind {
            let c = configuration.get_or_insert_with(Box::default);
            if let Some(p) = c.name_problem(&self.input.name, None, &taken) {
                return Err(CommandError::Invalid(p));
            }
            c.inputs.push(self.input.clone());
        }
        refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Changes an input: its name (rewriting the expressions that name it), kind, default and
/// description.
#[derive(Debug, Clone)]
pub struct EditConfigInput {
    pub element: ElementId,
    pub input: ConfigInput,
}

impl Command for EditConfigInput {
    fn label(&self) -> String {
        format!("Edit input #{}", self.input.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.element)?;
        let taken = taken_names(el);
        let c = el
            .configuration_mut()
            .ok_or_else(|| CommandError::Invalid("the Part Studio has no configuration".into()))?;
        if let Some(p) = c.name_problem(&self.input.name, Some(self.input.id), &taken) {
            return Err(CommandError::Invalid(p));
        }
        let i = c
            .inputs
            .iter_mut()
            .find(|i| i.id == self.input.id)
            .ok_or_else(|| CommandError::Invalid("no such input".into()))?;
        let old = std::mem::replace(i, self.input.clone()).name;
        if old != self.input.name {
            let new = self.input.name.clone();
            if let Some(fs) = el.features_mut() {
                rewrite_expressions(fs, &|e| rename_in(e, &old, &new));
            }
        }
        refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Removes an input: the expressions that name it keep their values as numbers, and
/// suppression by it is removed.
#[derive(Debug, Clone)]
pub struct RemoveConfigInput {
    pub element: ElementId,
    pub input: InputId,
}

impl Command for RemoveConfigInput {
    fn label(&self) -> String {
        "Remove input".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.element)?;
        if el
            .configuration()
            .and_then(|c| c.input(self.input))
            .is_none()
        {
            return Err(CommandError::Invalid("no such input".into()));
        }
        bake_input(el, self.input);
        if let Some(c) = el.configuration_mut() {
            c.inputs.retain(|i| i.id != self.input);
            for p in &mut c.presets {
                p.values.remove(&self.input);
            }
        }
        refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Adds a preset with the values of `copy_of` (the active one's when `None`), and makes it
/// active.
#[derive(Debug, Clone)]
pub struct AddPreset {
    pub element: ElementId,
    pub preset: PresetId,
    pub name: String,
    pub copy_of: Option<PresetId>,
}

impl Command for AddPreset {
    fn label(&self) -> String {
        format!("Add preset {}", self.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = studio_mut(doc, self.element)?;
        let ElementKind::PartStudio { configuration, .. } = &mut el.kind else {
            unreachable!()
        };
        let c = configuration.get_or_insert_with(Box::default);
        let name = self.name.trim();
        if name.is_empty() {
            return Err(CommandError::Invalid("Enter a name".into()));
        }
        if c.preset_named(name).is_some() {
            return Err(CommandError::Invalid(format!(
                "There is already a preset {name}"
            )));
        }
        let values = match self.copy_of {
            Some(id) => c.preset(id).map(|p| p.values.clone()),
            None => c.active_preset().map(|p| p.values.clone()),
        }
        .unwrap_or_default();
        c.presets.push(Preset {
            id: self.preset,
            name: name.to_string(),
            values,
        });
        c.active = self.preset;
        refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Renames a preset.
#[derive(Debug, Clone)]
pub struct RenamePreset {
    pub element: ElementId,
    pub preset: PresetId,
    pub name: String,
}

impl Command for RenamePreset {
    fn label(&self) -> String {
        format!("Rename preset to {}", self.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let c = config_mut(doc, self.element)?;
        let name = self.name.trim();
        if name.is_empty() {
            return Err(CommandError::Invalid("Enter a name".into()));
        }
        if c.presets
            .iter()
            .any(|p| p.name == name && p.id != self.preset)
        {
            return Err(CommandError::Invalid(format!(
                "There is already a preset {name}"
            )));
        }
        c.preset_mut(self.preset)
            .ok_or_else(|| CommandError::Invalid("no such preset".into()))?
            .name = name.to_string();
        Ok(())
    }
}

/// Removes a preset (not the last one). The active preset moves to the first one left.
#[derive(Debug, Clone)]
pub struct RemovePreset {
    pub element: ElementId,
    pub preset: PresetId,
}

impl Command for RemovePreset {
    fn label(&self) -> String {
        "Remove preset".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let c = config_mut(doc, self.element)?;
        if c.preset(self.preset).is_none() {
            return Err(CommandError::Invalid("no such preset".into()));
        }
        if c.presets.len() == 1 {
            return Err(CommandError::Invalid(
                "A configuration keeps at least one preset".into(),
            ));
        }
        c.presets.retain(|p| p.id != self.preset);
        if c.active == self.preset {
            c.active = c.presets[0].id;
        }
        refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Sets an input's value in a preset (`None`: back to the input's default).
#[derive(Debug, Clone)]
pub struct SetPresetValue {
    pub element: ElementId,
    pub preset: PresetId,
    pub input: InputId,
    pub value: Option<f64>,
}

impl Command for SetPresetValue {
    fn label(&self) -> String {
        "Set preset value".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let c = config_mut(doc, self.element)?;
        let input = c
            .input(self.input)
            .cloned()
            .ok_or_else(|| CommandError::Invalid("no such input".into()))?;
        if let Some(v) = self.value
            && !v.is_finite()
        {
            return Err(CommandError::Invalid("not a number".into()));
        }
        let p = c
            .preset_mut(self.preset)
            .ok_or_else(|| CommandError::Invalid("no such preset".into()))?;
        match self.value {
            Some(v) => {
                let v = if input.kind == InputKind::Boolean {
                    if v.abs() < 1e-9 { 0.0 } else { 1.0 }
                } else {
                    v
                };
                p.values.insert(self.input, v);
            }
            None => {
                p.values.remove(&self.input);
            }
        }
        refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Makes a preset the one the studio shows.
#[derive(Debug, Clone)]
pub struct SetActivePreset {
    pub element: ElementId,
    pub preset: PresetId,
}

impl Command for SetActivePreset {
    fn label(&self) -> String {
        "Switch preset".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let c = config_mut(doc, self.element)?;
        if c.preset(self.preset).is_none() {
            return Err(CommandError::Invalid("no such preset".into()));
        }
        c.active = self.preset;
        refresh_studio(doc, self.element);
        Ok(())
    }
}

/// The labels of a feature's fields that can take an expression, with what they measure (the
/// node view's sockets).
pub fn parameter_slots(f: &Feature) -> Vec<(&'static str, Quantity, String)> {
    let mut out = Vec::new();
    if matches!(f.kind, FeatureKind::Sketch(_) | FeatureKind::Variable(_)) {
        return out;
    }
    let mut k = f.kind.clone();
    crate::variables::slots(&mut k, &mut |s| {
        out.push((s.label, s.quantity, s.expr.clone()));
    });
    out
}

/// Wires a feature's field to an expression (the node view's wire from an input: `#length`).
#[derive(Debug, Clone)]
pub struct ConnectParameter {
    pub element: ElementId,
    pub feature: FeatureId,
    /// The field's label, as [`crate::variables::slots`] names it ("Depth").
    pub slot: String,
    pub expr: String,
}

impl Command for ConnectParameter {
    fn label(&self) -> String {
        format!("Set {} to {}", self.slot, self.expr)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        set_slot(doc, self.element, self.feature, &self.slot, |s| {
            *s.expr = self.expr.trim().to_string();
        })?;
        refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Unwires a feature's field: its expression becomes its current value.
#[derive(Debug, Clone)]
pub struct DisconnectParameter {
    pub element: ElementId,
    pub feature: FeatureId,
    pub slot: String,
}

impl Command for DisconnectParameter {
    fn label(&self) -> String {
        format!("Disconnect {}", self.slot)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let units = doc.units;
        set_slot(doc, self.element, self.feature, &self.slot, |s| {
            *s.expr = units.with_unit(*s.value, s.quantity);
        })?;
        refresh_studio(doc, self.element);
        Ok(())
    }
}

fn set_slot(
    doc: &mut Document,
    element: ElementId,
    feature: FeatureId,
    slot: &str,
    f: impl Fn(crate::variables::Slot<'_>),
) -> Result<(), CommandError> {
    let feat = studio_mut(doc, element)?
        .features_mut()
        .and_then(|fs| fs.iter_mut().find(|x| x.id == feature))
        .ok_or_else(|| CommandError::Invalid("feature not found".into()))?;
    let mut found = false;
    crate::variables::slots(&mut feat.kind, &mut |s| {
        if !found && s.label == slot {
            found = true;
            f(s);
        }
    });
    if found {
        Ok(())
    } else {
        Err(CommandError::Invalid(format!(
            "{} has no field {slot}",
            feat.name
        )))
    }
}

/// Moves nodes of the node view (the Configuration node with `inputs_node`).
#[derive(Debug, Clone)]
pub struct MoveGraphNodes {
    pub element: ElementId,
    pub positions: Vec<(FeatureId, [f32; 2])>,
    pub inputs_node: Option<[f32; 2]>,
}

impl Command for MoveGraphNodes {
    fn label(&self) -> String {
        "Move nodes".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let l = studio_mut(doc, self.element)?
            .node_layout_mut()
            .ok_or_else(|| CommandError::Invalid("nodes need a Part Studio".into()))?;
        for (f, p) in &self.positions {
            l.positions.insert(*f, *p);
        }
        if let Some(p) = self.inputs_node {
            l.inputs_node = Some(p);
        }
        Ok(())
    }
}

/// Collapses a node of the node view to its header, or opens it.
#[derive(Debug, Clone)]
pub struct SetNodeCollapsed {
    pub element: ElementId,
    pub feature: FeatureId,
    pub collapsed: bool,
}

impl Command for SetNodeCollapsed {
    fn label(&self) -> String {
        if self.collapsed {
            "Collapse node".into()
        } else {
            "Expand node".into()
        }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let l = studio_mut(doc, self.element)?
            .node_layout_mut()
            .ok_or_else(|| CommandError::Invalid("nodes need a Part Studio".into()))?;
        if self.collapsed {
            l.collapsed.insert(self.feature);
        } else {
            l.collapsed.remove(&self.feature);
        }
        Ok(())
    }
}

/// Switches assembly instances to another preset of their Part Studio (their generated studio
/// becomes that preset's). Ids, poses and mates stay: the presets' parts have the same ids.
#[derive(Debug, Clone)]
pub struct SetInstancePreset {
    pub assembly: ElementId,
    pub instances: Vec<crate::assembly::InstanceId>,
    pub preset: PresetId,
}

impl Command for SetInstancePreset {
    fn label(&self) -> String {
        "Change preset".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let mut changes = Vec::new();
        {
            let asm = doc
                .element(self.assembly)
                .and_then(|e| e.assembly_model())
                .ok_or(CommandError::ElementNotFound(self.assembly))?;
            for id in &self.instances {
                let i = asm
                    .instances
                    .iter()
                    .find(|i| i.id == *id)
                    .ok_or_else(|| CommandError::Invalid("instance not found".into()))?;
                let src = i.source.element();
                let studio = match configured_of(doc, src) {
                    Some((s, _)) => s,
                    None => src,
                };
                let el = doc
                    .elements
                    .iter()
                    .find(|e| e.id == studio)
                    .ok_or_else(|| CommandError::Invalid("its Part Studio is gone".into()))?;
                if el
                    .configuration()
                    .and_then(|c| c.preset(self.preset))
                    .is_none()
                {
                    return Err(CommandError::Invalid(format!(
                        "{} has no such preset",
                        el.name
                    )));
                }
                changes.push((*id, configured_id(studio, self.preset)));
            }
        }
        // The new presets' studios, before anything looks for them.
        let asm_el = doc
            .element_mut(self.assembly)
            .and_then(|e| e.assembly_model_mut())
            .ok_or(CommandError::ElementNotFound(self.assembly))?;
        for (id, to) in &changes {
            if let Some(i) = asm_el.instances.iter_mut().find(|i| i.id == *id) {
                i.source = match i.source {
                    crate::assembly::InstanceSource::Part { part, .. } => {
                        crate::assembly::InstanceSource::Part { element: *to, part }
                    }
                    crate::assembly::InstanceSource::Studio { board, .. } => {
                        crate::assembly::InstanceSource::Studio {
                            element: *to,
                            board,
                        }
                    }
                    s @ crate::assembly::InstanceSource::Assembly { .. } => s,
                };
            }
        }
        refresh(doc);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
