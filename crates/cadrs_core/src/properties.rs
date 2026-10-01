//! **Part and assembly properties** (P3B.6; `intro-to-assemblies.md` A20, `test-drive.md` TD9,
//! and later the drawings' title blocks and BOM tables, P3C.4–P3C.5).
//!
//! Every part of a Part Studio, every standard content part (A19.3) and every Assembly tab has
//! the same set of properties, read and written through this module whatever the owner:
//!
//! | Property | Part of a Part Studio | Standard content part | Assembly |
//! |---|---|---|---|
//! | Name | the part's rename (else its default name) | the generated part's name | the tab's name |
//! | Part number, Description | [`Properties`] on its [`PartProps`] | [`StandardPart`]'s own fields | [`Properties`] on its [`Assembly`] |
//! | Material, Appearance | [`PartProps::material`], [`PartProps::appearance`] | the same, in its generated studio | – (not set) |
//! | Revision, Vendor, Unit of measure, Category, Mass override, custom | [`Properties`] | [`Properties`] | [`Properties`] |
//!
//! So a Part number typed in the BOM is the part's own property: the Properties dialog shows it,
//! and the other way round (A20.10, TD9.3: "two-way", one source of truth).
//!
//! **Custom properties** are defined per document ([`PropertySettings::definitions`]: a name and
//! a [`PropertyKind`]); their values are kept per owner by definition id. The per-document
//! settings also hold the sequential **part numbering** scheme ([`NumberingScheme`], A20.11,
//! instead of Onshape's company setting) and the saved BOM templates (A20.7, local to the
//! document; see [`crate::assembly::bom`]).
//!
//! Every edit goes through the commands at the end of this module ([`SetProperties`],
//! [`AddPropertyDefinition`], …), one undo step each.
//!
//! [`StandardPart`]: crate::assembly::standard::StandardPart
//! [`Assembly`]: crate::assembly::Assembly

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use crate::assembly::InstanceSource;
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, PartProps};
use crate::ids::{ElementId, PartId};
use crate::material::Material;
use crate::rebuild::Build;

/// What has properties: a part (of a Part Studio or a standard content configuration) or an
/// Assembly tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PropertyOwner {
    Part { element: ElementId, part: PartId },
    Assembly { element: ElementId },
    /// An assembly's non-geometric **Item** (P3B.8, A1.7, [`crate::assembly::items`]).
    Item { element: ElementId, item: crate::assembly::items::ItemId },
}

impl PropertyOwner {
    /// The Part Studio (or standard content studio) or Assembly tab.
    pub fn element(&self) -> ElementId {
        match self {
            PropertyOwner::Part { element, .. } | PropertyOwner::Assembly { element } | PropertyOwner::Item { element, .. } => *element,
        }
    }

    pub fn is_assembly(&self) -> bool {
        matches!(self, PropertyOwner::Assembly { .. })
    }
}

impl From<InstanceSource> for PropertyOwner {
    fn from(s: InstanceSource) -> Self {
        match s {
            InstanceSource::Part { element, part } => PropertyOwner::Part { element, part },
            // A rigid Part Studio instance has no properties of its own (its parts do): the BOM
            // and the mass list its parts.
            InstanceSource::Assembly { element } | InstanceSource::Studio { element } => PropertyOwner::Assembly { element },
        }
    }
}

/// The id of a custom property definition (unique in its document).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PropertyDefId(pub u32);

/// A property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PropertyKey {
    Name,
    PartNumber,
    Description,
    Material,
    Revision,
    Vendor,
    UnitOfMeasure,
    Category,
    Appearance,
    /// The mass (kg): the override if set, else density × volume (read only in the BOM).
    Mass,
    /// An assembly's **Subassembly BOM behavior** (A20.5).
    BomBehavior,
    /// A property defined in the document ([`PropertySettings::definitions`]).
    Custom(PropertyDefId),
}

impl PropertyKey {
    /// The built-in properties, in the order the Properties dialog and "Add column" list them.
    pub const BUILT_IN: [PropertyKey; 10] = [
        PropertyKey::Name,
        PropertyKey::PartNumber,
        PropertyKey::Description,
        PropertyKey::Material,
        PropertyKey::Revision,
        PropertyKey::Vendor,
        PropertyKey::UnitOfMeasure,
        PropertyKey::Category,
        PropertyKey::Appearance,
        PropertyKey::Mass,
    ];

    /// Its label ("Part number"); a custom property's name from `settings`.
    pub fn label(&self, settings: &PropertySettings) -> String {
        match self {
            PropertyKey::Name => "Name".into(),
            PropertyKey::PartNumber => "Part number".into(),
            PropertyKey::Description => "Description".into(),
            PropertyKey::Material => "Material".into(),
            PropertyKey::Revision => "Revision".into(),
            PropertyKey::Vendor => "Vendor".into(),
            PropertyKey::UnitOfMeasure => "Unit of measure".into(),
            PropertyKey::Category => "Category".into(),
            PropertyKey::Appearance => "Appearance".into(),
            PropertyKey::Mass => "Mass".into(),
            PropertyKey::BomBehavior => "Subassembly BOM behavior".into(),
            PropertyKey::Custom(id) => settings.definition(*id).map(|d| d.name.clone()).unwrap_or_else(|| "(deleted property)".into()),
        }
    }

    /// A text property typed in (a BOM cell edited in place, A20.10): not Material (a picker),
    /// Appearance, Mass or the BOM behaviour.
    pub fn is_text(&self) -> bool {
        !matches!(self, PropertyKey::Material | PropertyKey::Appearance | PropertyKey::Mass | PropertyKey::BomBehavior)
    }
}

/// How a subassembly shows in its parent's BOM (A20.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SubassemblyBom {
    /// **Show assembly and components** (the default): the assembly's row, its parts indented.
    #[default]
    AssemblyAndComponents,
    /// **Show assembly only**: one row (a purchased assembly).
    AssemblyOnly,
    /// **Show components only**: its parts as if they were inserted in the parent.
    ComponentsOnly,
}

impl SubassemblyBom {
    pub const ALL: [SubassemblyBom; 3] = [SubassemblyBom::AssemblyAndComponents, SubassemblyBom::AssemblyOnly, SubassemblyBom::ComponentsOnly];

    pub fn label(self) -> &'static str {
        match self {
            SubassemblyBom::AssemblyAndComponents => "Show assembly and components",
            SubassemblyBom::AssemblyOnly => "Show assembly only",
            SubassemblyBom::ComponentsOnly => "Show components only",
        }
    }

    fn is_default(&self) -> bool {
        *self == SubassemblyBom::AssemblyAndComponents
    }
}

/// The units of measure offered (Onshape's list, abridged); "Each" is the default.
pub const UNITS_OF_MEASURE: [&str; 12] = ["Each", "Millimeter", "Centimeter", "Meter", "Inch", "Foot", "Gram", "Kilogram", "Ounce", "Pound", "Liter", "Gallon"];

/// The categories offered; any text is accepted.
pub const CATEGORIES: [&str; 5] = ["Manufactured", "Purchased", "Standard content", "Reference", "Assembly"];

/// A part's or assembly's own property values (those not kept elsewhere; see the module's
/// table). `None` / empty means not set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Properties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// One of [`UNITS_OF_MEASURE`] (else "Each").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_of_measure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// A mass given instead of the computed one (kg): the Mass properties panel and the BOM use it
    /// (`ex3-step16.png`'s Mass "Override").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mass_override: Option<f64>,
    /// Custom property values, by definition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<(PropertyDefId, String)>,
    /// An assembly's Subassembly BOM behavior (A20.5).
    #[serde(default, skip_serializing_if = "SubassemblyBom::is_default")]
    pub bom_behavior: SubassemblyBom,
}

impl Properties {
    pub fn is_empty(&self) -> bool {
        *self == Properties::default()
    }

    /// A custom property's value, if set.
    pub fn custom(&self, id: PropertyDefId) -> Option<&str> {
        self.custom.iter().find(|(d, _)| *d == id).map(|(_, v)| v.as_str())
    }

    fn text_slot(&mut self, key: PropertyKey) -> Option<&mut Option<String>> {
        Some(match key {
            PropertyKey::PartNumber => &mut self.part_number,
            PropertyKey::Description => &mut self.description,
            PropertyKey::Revision => &mut self.revision,
            PropertyKey::Vendor => &mut self.vendor,
            PropertyKey::UnitOfMeasure => &mut self.unit_of_measure,
            PropertyKey::Category => &mut self.category,
            _ => return None,
        })
    }

    fn text(&self, key: PropertyKey) -> Option<&str> {
        match key {
            PropertyKey::PartNumber => self.part_number.as_deref(),
            PropertyKey::Description => self.description.as_deref(),
            PropertyKey::Revision => self.revision.as_deref(),
            PropertyKey::Vendor => self.vendor.as_deref(),
            PropertyKey::UnitOfMeasure => self.unit_of_measure.as_deref(),
            PropertyKey::Category => self.category.as_deref(),
            PropertyKey::Custom(id) => self.custom(id),
            _ => None,
        }
    }
}

/// What a custom property holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyKind {
    Text,
    /// A number (the value must parse).
    Number,
    /// A checkbox: "true" or "false".
    Boolean,
    /// One of the choices.
    List(Vec<String>),
}

impl PropertyKind {
    pub fn label(&self) -> &'static str {
        match self {
            PropertyKind::Text => "Text",
            PropertyKind::Number => "Number",
            PropertyKind::Boolean => "Boolean",
            PropertyKind::List(_) => "List",
        }
    }

    /// `value` made valid for the kind (an empty value clears).
    pub fn check(&self, value: &str) -> Result<String, CommandError> {
        let v = value.trim();
        if v.is_empty() {
            return Ok(String::new());
        }
        match self {
            PropertyKind::Text => Ok(v.to_string()),
            PropertyKind::Number => v.parse::<f64>().ok().filter(|x| x.is_finite()).map(|_| v.to_string()).ok_or_else(|| CommandError::Invalid(format!("\"{v}\" is not a number"))),
            PropertyKind::Boolean => match v.to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" => Ok("true".into()),
                "false" | "no" | "0" => Ok("false".into()),
                _ => Err(CommandError::Invalid(format!("\"{v}\" is not true or false"))),
            },
            PropertyKind::List(choices) => choices.iter().find(|c| c.eq_ignore_ascii_case(v)).cloned().ok_or_else(|| CommandError::Invalid(format!("\"{v}\" is not one of the choices"))),
        }
    }
}

/// A custom property defined in a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyDef {
    pub id: PropertyDefId,
    pub name: String,
    pub kind: PropertyKind,
}

/// Sequential part numbers (A20.11): `prefix` and `next`, zero-padded to `digits` ("PRT-000001").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NumberingScheme {
    pub prefix: String,
    pub digits: u8,
    /// The next number to give.
    pub next: u32,
}

impl Default for NumberingScheme {
    fn default() -> Self {
        Self { prefix: "PRT-".into(), digits: 6, next: 1 }
    }
}

impl NumberingScheme {
    /// The part number for `n`.
    pub fn format(&self, n: u32) -> String {
        format!("{}{:0width$}", self.prefix, n, width = self.digits as usize)
    }
}

/// A document's property settings: custom definitions, the numbering scheme and BOM templates.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PropertySettings {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definitions: Vec<PropertyDef>,
    #[serde(default)]
    pub numbering: NumberingScheme,
    /// BOM templates saved with **Save as template** (A20.7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bom_templates: Vec<crate::assembly::bom::BomTemplate>,
}

impl PropertySettings {
    pub fn is_default(&self) -> bool {
        *self == PropertySettings::default()
    }

    pub fn definition(&self, id: PropertyDefId) -> Option<&PropertyDef> {
        self.definitions.iter().find(|d| d.id == id)
    }

    /// Every property, built-in then custom (the "Add column" list).
    pub fn all_keys(&self) -> Vec<PropertyKey> {
        let mut out = PropertyKey::BUILT_IN.to_vec();
        out.extend(self.definitions.iter().map(|d| PropertyKey::Custom(d.id)));
        out
    }
}

/// A property value to set.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// Text (empty clears); for Name, Part number, Description, Revision, Vendor, Unit of
    /// measure, Category and custom properties.
    Text(String),
    Material(Option<Material>),
    Appearance(Option<Appearance>),
    /// Mass override, kg (`None`: computed).
    Mass(Option<f64>),
    BomBehavior(SubassemblyBom),
}

// ---------------------------------------------------------------------------------------------
// Reading

/// The source Part Studio's (or standard content studio's) settings of a part.
fn part_props(doc: &Document, element: ElementId, part: PartId) -> Option<&PartProps> {
    doc.element(element)?.part_prop(part)
}

/// The owner's own [`Properties`] (empty when it has none).
pub fn properties(doc: &Document, owner: PropertyOwner) -> Properties {
    match owner {
        PropertyOwner::Part { element, part } => part_props(doc, element, part).map(|p| p.properties.clone()).unwrap_or_default(),
        PropertyOwner::Assembly { element } => doc.element(element).and_then(|e| e.assembly_model()).map(|a| a.properties.clone()).unwrap_or_default(),
        PropertyOwner::Item { element, item } => item_of(doc, element, item).map(|i| i.properties.clone()).unwrap_or_default(),
    }
}

fn item_of(doc: &Document, element: ElementId, item: crate::assembly::items::ItemId) -> Option<&crate::assembly::items::Item> {
    doc.element(element)?.assembly_model()?.item(item)
}

/// The owner exists (its studio or tab is in the document).
pub fn exists(doc: &Document, owner: PropertyOwner) -> bool {
    match owner {
        PropertyOwner::Part { element, .. } => doc.element(element).is_some_and(|e| e.assembly_model().is_none()),
        PropertyOwner::Assembly { element } => doc.element(element).is_some_and(|e| e.assembly_model().is_some()),
        PropertyOwner::Item { element, item } => item_of(doc, element, item).is_some(),
    }
}

/// The owner's name: a part's rename or its default name from `build` (its studio's rebuild),
/// an assembly's tab name.
pub fn name(doc: &Document, owner: PropertyOwner, build: Option<&Build>) -> String {
    let source = match owner {
        PropertyOwner::Part { element, part } => InstanceSource::Part { element, part },
        PropertyOwner::Assembly { element } => InstanceSource::Assembly { element },
        PropertyOwner::Item { element, item } => return item_of(doc, element, item).map(|i| i.name.clone()).unwrap_or_default(),
    };
    crate::assembly::source_part_name(doc, &source, build)
}

/// A text property ("" when not set). Part number and Description of a standard content part
/// are its configuration's (A19.3); its Category defaults to "Standard content". Name needs
/// `build` for a part's default name; Material gives the material's name, Appearance its
/// colour as `#RRGGBB`; Mass is left to [`mass`]; the BOM behaviour gives its label.
pub fn text(doc: &Document, owner: PropertyOwner, key: PropertyKey, build: Option<&Build>) -> String {
    let std = match owner {
        PropertyOwner::Part { element, .. } => doc.standard_part(element),
        PropertyOwner::Assembly { .. } | PropertyOwner::Item { .. } => None,
    };
    match key {
        PropertyKey::Name => name(doc, owner, build),
        PropertyKey::PartNumber if std.is_some() => std.map(|s| s.part_number.clone()).unwrap_or_default(),
        PropertyKey::Description if std.is_some() => std.map(|s| s.description.clone()).unwrap_or_default(),
        PropertyKey::Material => material(doc, owner).map(|m| m.name).unwrap_or_default(),
        PropertyKey::Appearance => appearance(doc, owner, build).map(|a| format!("#{:02X}{:02X}{:02X}", a.rgb[0], a.rgb[1], a.rgb[2])).unwrap_or_default(),
        PropertyKey::Mass => String::new(),
        PropertyKey::BomBehavior => properties(doc, owner).bom_behavior.label().to_string(),
        PropertyKey::UnitOfMeasure => properties(doc, owner).unit_of_measure.unwrap_or_else(|| UNITS_OF_MEASURE[0].to_string()),
        PropertyKey::Category => {
            let own = properties(doc, owner).category;
            own.or_else(|| std.map(|_| "Standard content".to_string())).unwrap_or_default()
        }
        _ => properties(doc, owner).text(key).unwrap_or_default().to_string(),
    }
}

/// A part's material (an assembly has none).
pub fn material(doc: &Document, owner: PropertyOwner) -> Option<Material> {
    match owner {
        PropertyOwner::Part { element, part } => part_props(doc, element, part).and_then(|p| p.material.clone()),
        PropertyOwner::Assembly { .. } | PropertyOwner::Item { .. } => None,
    }
}

/// A part's appearance: its own, else its palette colour from `build`.
pub fn appearance(doc: &Document, owner: PropertyOwner, build: Option<&Build>) -> Option<Appearance> {
    let PropertyOwner::Part { element, part } = owner else { return None };
    part_props(doc, element, part)
        .and_then(|p| p.appearance)
        .or_else(|| {
            let props = doc.element(element).map(|e| e.part_props()).unwrap_or_default();
            build.and_then(|b| b.part(part)).map(|p| crate::appearance::default_appearance(p, props))
        })
}

/// The mass of one of the owner (kg): its override, else a part's density × volume (`None`
/// without a material), else an assembly's parts summed (`None` if one has no mass).
/// `build_of` gives each Part Studio's rebuild.
pub fn mass(doc: &Document, owner: PropertyOwner, build_of: &mut dyn FnMut(ElementId) -> Option<Arc<Build>>) -> Option<f64> {
    if let Some(m) = properties(doc, owner).mass_override {
        return Some(m);
    }
    match owner {
        PropertyOwner::Part { element, part } => {
            let rho = material(doc, owner)?.density_kg_mm3();
            let b = build_of(element)?;
            Some(rho * b.part(part)?.mass?.volume)
        }
        PropertyOwner::Assembly { element } => {
            let asm = doc.element(element)?.assembly_model()?;
            let mut total = 0.0;
            for inst in asm.instances.iter().filter(|i| !i.suppressed) {
                match inst.source {
                    // A rigid Part Studio instance: its parts.
                    InstanceSource::Studio { element } => {
                        for part in &inst.parts {
                            total += mass(doc, PropertyOwner::Part { element, part: *part }, build_of)?;
                        }
                    }
                    s => total += mass(doc, s.into(), build_of)?,
                }
            }
            Some(total)
        }
        // An item's mass is its override only.
        PropertyOwner::Item { .. } => None,
    }
}

/// The part number every owner of the document has now (standard content parts included).
pub fn part_numbers_in_use(doc: &Document) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in &doc.elements {
        for p in e.part_props() {
            out.extend(p.properties.part_number.clone());
        }
        if let Some(a) = e.assembly_model() {
            out.extend(a.properties.part_number.clone());
        }
    }
    for s in &doc.standard_content {
        out.push(s.part_number.clone());
    }
    out.retain(|n| !n.is_empty());
    out
}

// ---------------------------------------------------------------------------------------------
// Writing

/// The owner's [`PartProps`] entry, made if missing (a part of a Part Studio tab or of a
/// standard content studio).
fn part_props_mut(doc: &mut Document, element: ElementId, part: PartId) -> Result<&mut PartProps, CommandError> {
    let props = if doc.elements.iter().any(|e| e.id == element) {
        doc.element_mut(element).and_then(|e| e.part_props_mut())
    } else {
        doc.standard_content.iter_mut().find(|p| p.element.id == element).and_then(|p| p.element.part_props_mut())
    }
    .ok_or(CommandError::ElementNotFound(element))?;
    if let Some(i) = props.iter().position(|p| p.part == part) {
        return Ok(&mut props[i]);
    }
    props.push(PartProps::new(part));
    Ok(props.last_mut().expect("just pushed"))
}

fn tidy(doc: &mut Document, element: ElementId) {
    let props = if doc.elements.iter().any(|e| e.id == element) {
        doc.element_mut(element).and_then(|e| e.part_props_mut())
    } else {
        doc.standard_content.iter_mut().find(|p| p.element.id == element).and_then(|p| p.element.part_props_mut())
    };
    if let Some(p) = props {
        p.retain(|p| !p.is_default());
    }
}

fn opt(v: &str) -> Option<String> {
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Sets one property of `owner` (see [`SetProperties`] for the rules).
pub fn set(doc: &mut Document, owner: PropertyOwner, key: PropertyKey, value: &PropertyValue) -> Result<(), CommandError> {
    let is_std = matches!(owner, PropertyOwner::Part { element, .. } if doc.standard_part(element).is_some());
    let wrong = || CommandError::Invalid(format!("wrong value for {key:?}"));
    // Text of a custom property is checked against its kind.
    let value = match (key, value) {
        (PropertyKey::Custom(id), PropertyValue::Text(t)) => {
            let def = doc.properties.definition(id).ok_or_else(|| CommandError::Invalid("no such property".into()))?;
            PropertyValue::Text(def.kind.check(t)?)
        }
        _ => value.clone(),
    };
    match (owner, key, &value) {
        (_, PropertyKey::Name, PropertyValue::Text(t)) => {
            let n = opt(t).ok_or_else(|| CommandError::Invalid("the name can't be empty".into()))?;
            match owner {
                PropertyOwner::Part { element, part } => part_props_mut(doc, element, part)?.name = Some(n),
                PropertyOwner::Assembly { element } => doc.element_mut(element).ok_or(CommandError::ElementNotFound(element))?.name = n,
                PropertyOwner::Item { element, item } => {
                    crate::assembly::commands::assembly_mut(doc, element)?.item_mut(item).ok_or_else(|| CommandError::Invalid("item not found".into()))?.name = n;
                }
            }
        }
        (PropertyOwner::Part { element, .. }, PropertyKey::PartNumber | PropertyKey::Description, PropertyValue::Text(t)) if is_std => {
            let s = doc.standard_content.iter_mut().find(|p| p.element.id == element).ok_or(CommandError::ElementNotFound(element))?;
            if key == PropertyKey::PartNumber {
                s.part_number = t.trim().to_string();
            } else {
                s.description = t.trim().to_string();
            }
        }
        (PropertyOwner::Part { element, part }, PropertyKey::Material, PropertyValue::Material(m)) => {
            part_props_mut(doc, element, part)?.material = m.clone();
        }
        (PropertyOwner::Part { element, part }, PropertyKey::Appearance, PropertyValue::Appearance(a)) => {
            part_props_mut(doc, element, part)?.appearance = *a;
        }
        (PropertyOwner::Assembly { .. } | PropertyOwner::Item { .. }, PropertyKey::Material | PropertyKey::Appearance, _) => {
            return Err(CommandError::Invalid("an assembly has no material or appearance of its own".into()));
        }
        (_, PropertyKey::Mass, PropertyValue::Mass(m)) => {
            if m.is_some_and(|m| !(m.is_finite() && m >= 0.0)) {
                return Err(CommandError::Invalid("the mass must be a positive number".into()));
            }
            own_mut(doc, owner, &mut |p| p.mass_override = *m)?;
        }
        (PropertyOwner::Assembly { .. }, PropertyKey::BomBehavior, PropertyValue::BomBehavior(b)) => {
            own_mut(doc, owner, &mut |p| p.bom_behavior = *b)?;
        }
        (_, PropertyKey::Custom(id), PropertyValue::Text(t)) => {
            let t = t.clone();
            own_mut(doc, owner, &mut |p| {
                p.custom.retain(|(d, _)| *d != id);
                if !t.is_empty() {
                    p.custom.push((id, t.clone()));
                    p.custom.sort_by_key(|(d, _)| *d);
                }
            })?;
        }
        (_, PropertyKey::UnitOfMeasure, PropertyValue::Text(t)) => {
            let v = opt(t);
            if let Some(u) = &v
                && !UNITS_OF_MEASURE.iter().any(|x| x.eq_ignore_ascii_case(u))
            {
                return Err(CommandError::Invalid(format!("\"{u}\" is not a unit of measure")));
            }
            let v = v.map(|u| UNITS_OF_MEASURE.iter().find(|x| x.eq_ignore_ascii_case(&u)).map(|x| x.to_string()).unwrap_or(u)).filter(|u| u != "Each");
            own_mut(doc, owner, &mut |p| p.unit_of_measure = v.clone())?;
        }
        (_, PropertyKey::PartNumber | PropertyKey::Description | PropertyKey::Revision | PropertyKey::Vendor | PropertyKey::Category, PropertyValue::Text(t)) => {
            let v = opt(t);
            own_mut(doc, owner, &mut |p| {
                if let Some(slot) = p.text_slot(key) {
                    *slot = v.clone();
                }
            })?;
        }
        _ => return Err(wrong()),
    }
    if let PropertyOwner::Part { element, .. } = owner {
        tidy(doc, element);
    }
    Ok(())
}

/// Edits the owner's own [`Properties`].
fn own_mut(doc: &mut Document, owner: PropertyOwner, f: &mut dyn FnMut(&mut Properties)) -> Result<(), CommandError> {
    match owner {
        PropertyOwner::Part { element, part } => f(&mut part_props_mut(doc, element, part)?.properties),
        PropertyOwner::Assembly { element } => f(&mut crate::assembly::commands::assembly_mut(doc, element)?.properties),
        PropertyOwner::Item { element, item } => {
            f(&mut crate::assembly::commands::assembly_mut(doc, element)?.item_mut(item).ok_or_else(|| CommandError::Invalid("item not found".into()))?.properties)
        }
    }
    Ok(())
}

/// Sets properties of one or more owners (the Properties dialog's Save, a BOM cell edited in
/// place, a material picked in the BOM): one undo step. Rules: Name can't be empty; an empty
/// text clears the property; Part number and Description of a standard content part are its
/// configuration's; Unit of measure must be one of [`UNITS_OF_MEASURE`]; a custom property's
/// value must suit its kind; an assembly has no Material or Appearance.
#[derive(Debug, Clone)]
pub struct SetProperties {
    pub owners: Vec<PropertyOwner>,
    pub values: Vec<(PropertyKey, PropertyValue)>,
    /// Shown in the undo menu.
    pub label: String,
}

impl Command for SetProperties {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.owners.is_empty() {
            return Err(CommandError::Invalid("nothing to set".into()));
        }
        for o in &self.owners {
            if !exists(doc, *o) {
                return Err(CommandError::ElementNotFound(o.element()));
            }
            for (k, v) in &self.values {
                set(doc, *o, *k, v)?;
            }
        }
        Ok(())
    }
}

/// Adds a custom property definition (its id is the next free one). One undo step.
#[derive(Debug, Clone)]
pub struct AddPropertyDefinition {
    pub name: String,
    pub kind: PropertyKind,
}

impl AddPropertyDefinition {
    /// The id the definition gets in `doc`.
    pub fn id_in(doc: &Document) -> PropertyDefId {
        PropertyDefId(doc.properties.definitions.iter().map(|d| d.id.0).max().unwrap_or(0) + 1)
    }
}

impl Command for AddPropertyDefinition {
    fn label(&self) -> String {
        format!("Add property {}", self.name.trim())
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let name = opt(&self.name).ok_or_else(|| CommandError::Invalid("the name can't be empty".into()))?;
        let taken = PropertyKey::BUILT_IN.iter().any(|k| k.label(&doc.properties).eq_ignore_ascii_case(&name))
            || doc.properties.definitions.iter().any(|d| d.name.eq_ignore_ascii_case(&name));
        if taken {
            return Err(CommandError::Invalid(format!("a property \"{name}\" exists")));
        }
        if let PropertyKind::List(c) = &self.kind
            && c.is_empty()
        {
            return Err(CommandError::Invalid("a list needs choices".into()));
        }
        let id = Self::id_in(doc);
        doc.properties.definitions.push(PropertyDef { id, name, kind: self.kind.clone() });
        Ok(())
    }
}

/// Removes a custom property definition, its values and its BOM columns. One undo step.
#[derive(Debug, Clone)]
pub struct RemovePropertyDefinition {
    pub id: PropertyDefId,
}

impl Command for RemovePropertyDefinition {
    fn label(&self) -> String {
        "Remove property".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if doc.properties.definition(self.id).is_none() {
            return Err(CommandError::Invalid("no such property".into()));
        }
        let id = self.id;
        doc.properties.definitions.retain(|d| d.id != id);
        let col = crate::assembly::bom::BomColumn::Property(PropertyKey::Custom(id));
        for t in &mut doc.properties.bom_templates {
            t.columns.retain(|c| *c != col);
        }
        let mut studios: Vec<ElementId> = Vec::new();
        for e in &mut doc.elements {
            if let Some(props) = e.part_props_mut() {
                for p in props.iter_mut() {
                    p.properties.custom.retain(|(d, _)| *d != id);
                }
                studios.push(e.id);
            }
            if let Some(a) = e.assembly_model_mut() {
                a.properties.custom.retain(|(d, _)| *d != id);
                a.bom.columns.retain(|c| *c != col);
            }
        }
        for s in &mut doc.standard_content {
            if let Some(props) = s.element.part_props_mut() {
                for p in props.iter_mut() {
                    p.properties.custom.retain(|(d, _)| *d != id);
                }
            }
        }
        for s in studios {
            tidy(doc, s);
        }
        Ok(())
    }
}

/// Sets the document's part numbering scheme (A20.11). One undo step.
#[derive(Debug, Clone)]
pub struct SetNumberingScheme {
    pub scheme: NumberingScheme,
}

impl Command for SetNumberingScheme {
    fn label(&self) -> String {
        "Part numbering".into()
    }
    fn scope(&self) -> Scope {
        Scope::Document
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.scheme.digits > 12 {
            return Err(CommandError::Invalid("at most 12 digits".into()));
        }
        doc.properties.numbering = self.scheme.clone();
        Ok(())
    }
}

/// **Generate missing part numbers** (A20.11, TD9.4): each of `owners` without a Part number
/// gets the next sequential one of the document's scheme, skipping numbers already in use; the
/// ones that have a Part number keep it. One undo step.
#[derive(Debug, Clone)]
pub struct GenerateMissingPartNumbers {
    /// In the order to number them (the BOM's).
    pub owners: Vec<PropertyOwner>,
}

impl GenerateMissingPartNumbers {
    /// The numbers the owners without one would get, in order.
    pub fn plan(&self, doc: &Document) -> Vec<(PropertyOwner, String)> {
        let used = part_numbers_in_use(doc);
        let mut next = doc.properties.numbering.next.max(1);
        let mut out: Vec<(PropertyOwner, String)> = Vec::new();
        let mut seen: Vec<PropertyOwner> = Vec::new();
        for o in &self.owners {
            if seen.contains(o) || !exists(doc, *o) || !text(doc, *o, PropertyKey::PartNumber, None).is_empty() {
                continue;
            }
            seen.push(*o);
            let number = loop {
                let n = doc.properties.numbering.format(next);
                next += 1;
                if !used.contains(&n) {
                    break n;
                }
            };
            out.push((*o, number));
        }
        out
    }
}

impl Command for GenerateMissingPartNumbers {
    fn label(&self) -> String {
        "Generate missing part numbers".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let plan = self.plan(doc);
        if plan.is_empty() {
            return Ok(());
        }
        let last = plan.last().map(|(_, n)| n.clone()).unwrap_or_default();
        for (o, n) in plan {
            set(doc, o, PropertyKey::PartNumber, &PropertyValue::Text(n))?;
        }
        // The scheme continues after the last number given.
        let scheme = &mut doc.properties.numbering;
        if let Some(k) = last.strip_prefix(scheme.prefix.as_str()).and_then(|d| d.parse::<u32>().ok()) {
            scheme.next = k + 1;
        }
        Ok(())
    }
}
