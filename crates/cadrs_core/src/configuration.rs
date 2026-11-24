//! **Configurations**: one Part Studio that builds as several variants, picked in the Part
//! Studio, per assembly instance, per drawing view and per Render Studio. Two views edit the
//! one model: the Configuration panel's table (as Onshape's) and the node view's Configuration
//! node, whose wires are the expressions below. (The model, the panel and the copies come from
//! Zach Strout's configurations, RTnhN's fork; the engine, inputs as variables wired by
//! expressions, from the node view's.)
//!
//! - **Inputs** ([`ConfigInput`]), in order: a **List** (a table of named rows, each a
//!   configuration: M3, M4, M5, …), a **Checkbox** (`#name` is 1 or 0) and a **Value** (`#name`
//!   a length, an angle or a number, typed per configuration: an instance's own length).
//! - **Columns** ([`ConfiguredParam`]): a List's or Checkbox's table has a column per configured
//!   parameter, with a value for each row (a Checkbox's two: unchecked, checked). **Each column
//!   is a variable, `#name`**: the parameter it was configured for (a feature field, a sketch
//!   dimension, a Variable's value, whether a feature is suppressed) reads `#name`, and any other
//!   field may too (the node view wires them). A part property's column is text, written into
//!   the part's settings instead.
//! - **The features never change with the configuration.** The active configuration's values
//!   are variables ahead of the whole list ([`input_variables`], through
//!   [`crate::document::Element::variable_base`]), so switching configurations changes those
//!   alone and the rebuild cache keeps its entries for each. A rebuild sees them as Variable
//!   features ([`input_features`]) with ids derived from their inputs'.
//! - **Editing a configured field edits the configuration shown**: after every command, a
//!   configured parameter whose expression was changed (a new depth typed into the Extrude
//!   dialog) has the new value put into the active row's cell and reads `#name` again
//!   ([`after_command`]), as in Onshape. Unconfiguring it leaves it the value it shows.
//! - **Copies.** An assembly instance, a drawing view or a Render Studio's model in a
//!   configuration other than the studio's names a **configured copy** ([`ConfiguredStudio`] in
//!   [`Document::configured`], found by [`Document::element`] like standard content): the studio
//!   in those values, under an id derived from the studio and the values ([`copy_id`]), so two
//!   instances in the same configuration share it. Copies are regenerated when their studio's
//!   model changes (not when it shows another configuration or its nodes move) and dropped when
//!   nothing names them ([`refresh_copies`]). The features keep their ids, so mates survive a
//!   change of configuration.
//! - **Part properties** can be configured ([`ParamTarget::Property`]): Name, Part number,
//!   Description, … (not Mass, Material or Appearance), text cells written into the part's
//!   [`PartProps`] for the configuration shown ([`apply_properties`]) and read back
//!   ([`after_command`]).
//! - **Documents from the fork** (their columns without names, the values written into the
//!   features) are read as they are and converted on load ([`migrate`]): each column named
//!   after its parameter, the parameter reading `#name`.

use std::collections::{BTreeMap, BTreeSet};

use cadrs_sketch::units::{Quantity, Units, VarValue};
use cadrs_vars::{VariableFeature, VariableKind};
use serde::{Deserialize, Serialize};

use crate::assembly::InstanceId;
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind, Feature, FeatureKind, PartProps};
use crate::ids::{ConfigInputId, ConfigRowId, ElementId, FeatureId, PartId};
use crate::properties::{PropertyKey, PropertySettings};

// ---------------------------------------------------------------------------------------------
// The model

/// A Part Studio's configuration: its inputs and the values it shows now.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Configuration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<ConfigInput>,
    /// The configuration the Part Studio shows.
    #[serde(default, skip_serializing_if = "ConfigValues::is_empty")]
    pub active: ConfigValues,
    /// Parts' names and properties set for one configuration (a configured instance's
    /// Properties, a BOM row's cells): its copies take them ([`generate`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub part_properties: Vec<ConfiguredPartProps>,
}

/// A part's own name and properties in one configuration (Part number, Description, …; its
/// material and appearance are the studio's in every configuration).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredPartProps {
    /// The configuration ([`Configuration::complete`]).
    pub values: ConfigValues,
    pub part: PartId,
    /// Its name in this configuration (else the studio's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "crate::properties::Properties::is_empty"
    )]
    pub properties: crate::properties::Properties,
}

impl Configuration {
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }

    pub fn input(&self, id: ConfigInputId) -> Option<&ConfigInput> {
        self.inputs.iter().find(|i| i.id == id)
    }

    pub fn input_mut(&mut self, id: ConfigInputId) -> Option<&mut ConfigInput> {
        self.inputs.iter_mut().find(|i| i.id == id)
    }

    /// The name and properties set for `part` in the configuration `values`.
    pub fn part_props(&self, values: &ConfigValues, part: PartId) -> Option<&ConfiguredPartProps> {
        let values = self.complete(values);
        self.part_properties
            .iter()
            .find(|p| p.part == part && p.values == values)
    }

    /// [`Configuration::part_props`] for writing, made from `now` (the part's name and
    /// properties in that configuration) if missing.
    pub fn part_props_mut(
        &mut self,
        values: &ConfigValues,
        part: PartId,
        now: (Option<String>, crate::properties::Properties),
    ) -> &mut ConfiguredPartProps {
        let values = self.complete(values);
        let i = match self
            .part_properties
            .iter()
            .position(|p| p.part == part && p.values == values)
        {
            Some(i) => i,
            None => {
                self.part_properties.push(ConfiguredPartProps {
                    values,
                    part,
                    name: now.0,
                    properties: now.1,
                });
                self.part_properties.len() - 1
            }
        };
        &mut self.part_properties[i]
    }

    /// The input named `name` (a Checkbox's or Value's `#name`, a List's title).
    pub fn input_named(&self, name: &str) -> Option<&ConfigInput> {
        self.inputs.iter().find(|i| i.name == name)
    }

    /// The column whose variable is `#name`, with its input.
    pub fn column_named(&self, name: &str) -> Option<(&ConfigInput, &ConfiguredParam)> {
        self.inputs.iter().find_map(|i| {
            i.params
                .iter()
                .find(|p| !p.name.is_empty() && p.name == name)
                .map(|p| (i, p))
        })
    }

    /// Every `#name` the configuration defines: its Checkboxes' and Values', its columns'.
    pub fn names(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in &self.inputs {
            if i.defines_variable() {
                out.push(i.name.clone());
            }
            out.extend(
                i.params
                    .iter()
                    .filter(|p| p.binds())
                    .map(|p| p.name.clone()),
            );
        }
        out
    }

    /// Every input's value in `values`, or its default where `values` has none (or one that no
    /// longer fits: a deleted row).
    pub fn resolve(&self, values: &ConfigValues) -> Vec<Resolved> {
        self.inputs
            .iter()
            .map(|input| input.resolve_value(values.get(input.id)))
            .collect()
    }

    /// The default values: each List's default row, each Checkbox's state, each Value's.
    pub fn defaults(&self) -> ConfigValues {
        let mut out = ConfigValues::default();
        for i in &self.inputs {
            out.set(i.id, i.default_value());
        }
        out
    }

    /// `values` with every input given (defaults filling in) and nothing else: what an
    /// instance or the active configuration keeps.
    pub fn complete(&self, values: &ConfigValues) -> ConfigValues {
        let mut out = ConfigValues::default();
        for (input, r) in self.inputs.iter().zip(self.resolve(values)) {
            let v = match (r, &input.kind) {
                (Resolved::Row(i), InputKind::List { rows, .. }) => InputValue::Row(rows[i].id),
                (Resolved::Checked(c), _) => InputValue::Checked(c),
                (Resolved::Number(x), _) => InputValue::Number(x),
                _ => continue,
            };
            out.set(input.id, v);
        }
        out
    }

    /// How the values read, for lists and labels: "M5, Chamfered, No thread, length 20".
    pub fn describe(&self, values: &ConfigValues) -> String {
        let mut parts = Vec::new();
        for (input, r) in self.inputs.iter().zip(self.resolve(values)) {
            parts.push(match (r, &input.kind) {
                (Resolved::Row(i), InputKind::List { rows, .. }) => rows[i].name.clone(),
                (Resolved::Checked(true), _) => input.name.clone(),
                (Resolved::Checked(false), _) => format!("No {}", input.name),
                (Resolved::Number(x), InputKind::Value { kind, .. }) => {
                    format!("{} {}", input.name, kind.plain(x))
                }
                _ => continue,
            });
        }
        parts.join(", ")
    }

    /// Why the configuration can't be used as it is (the first problem found): a bad or
    /// repeated name, a List without rows, a column whose values don't match its input's rows.
    pub fn problem(&self) -> Option<String> {
        for (k, input) in self.inputs.iter().enumerate() {
            if input.name.is_empty() {
                return Some("An input needs a name".into());
            }
            if !cadrs_sketch::units::is_variable_name(&input.name) {
                return Some(format!(
                    "{}: a name is letters, digits and _ (starting with a letter)",
                    input.name
                ));
            }
            if self.inputs[..k].iter().any(|i| i.name == input.name) {
                return Some(format!("Two inputs are named {}", input.name));
            }
            match &input.kind {
                InputKind::List { rows, default } => {
                    if rows.is_empty() {
                        return Some(format!("{}: a list needs a configuration", input.name));
                    }
                    if *default >= rows.len() {
                        return Some(format!("{}: the default is not a row", input.name));
                    }
                    for (j, r) in rows.iter().enumerate() {
                        if r.name.trim().is_empty() {
                            return Some(format!("{}: a configuration needs a name", input.name));
                        }
                        if rows[..j].iter().any(|x| x.name == r.name) {
                            return Some(format!(
                                "{}: two configurations are named {}",
                                input.name, r.name
                            ));
                        }
                    }
                }
                InputKind::Checkbox { .. } => {}
                InputKind::Value { .. } => {
                    if !input.params.is_empty() {
                        return Some(format!("{}: a value has no table", input.name));
                    }
                }
            }
            let n = input.row_count();
            for p in &input.params {
                if p.values.len() != n {
                    return Some(format!(
                        "{}: a configured parameter has {} values for {n} configurations",
                        input.name,
                        p.values.len()
                    ));
                }
                let boolean = p.kind_hint() == Some(ColumnKind::Boolean);
                if p.values.iter().any(|v| v.as_bool().is_some() != boolean) {
                    return Some(format!(
                        "{}: a configured parameter has a value of the wrong kind",
                        input.name
                    ));
                }
                if p.binds() && !cadrs_sketch::units::is_variable_name(&p.name) {
                    return Some(format!(
                        "#{}: a name is letters, digits and _ (starting with a letter)",
                        p.name
                    ));
                }
            }
        }
        let names = self.names();
        if let Some(n) = names
            .iter()
            .enumerate()
            .find(|(k, n)| names[..*k].contains(n))
            .map(|(_, n)| n)
        {
            return Some(format!("#{n} is defined twice"));
        }
        let targets: Vec<&ParamTarget> = self
            .inputs
            .iter()
            .flat_map(|i| i.params.iter().map(|p| &p.target))
            .filter(|t| !matches!(t, ParamTarget::Free { .. }))
            .collect();
        if targets
            .iter()
            .enumerate()
            .any(|(k, t)| targets[..k].contains(t))
        {
            return Some("A parameter is configured twice".into());
        }
        None
    }

    /// The parameter `target` and the input that configures it.
    pub fn param(&self, target: &ParamTarget) -> Option<(&ConfigInput, &ConfiguredParam)> {
        self.inputs.iter().find_map(|i| {
            i.params
                .iter()
                .find(|p| p.target == *target)
                .map(|p| (i, p))
        })
    }

    /// Whether the feature has a configured parameter.
    pub fn configures(&self, feature: FeatureId) -> bool {
        self.inputs
            .iter()
            .any(|i| i.params.iter().any(|p| p.target.feature() == Some(feature)))
    }

    /// The input and row whose cell gives `part`'s property `key` in the configuration
    /// `values`, when the property is configured.
    pub fn property_cell(
        &self,
        values: &ConfigValues,
        part: PartId,
        key: PropertyKey,
    ) -> Option<(ConfigInputId, usize)> {
        let (input, _) = self.param(&ParamTarget::Property { part, key })?;
        Some((input.id, input.row_of(values.get(input.id))))
    }

    /// Why `name` can't name a new input or column (or `own`, renamed), if it can't: `taken`
    /// are the other names in scope (the studio's Variables, its Variable Studios').
    pub fn name_problem(&self, name: &str, own: Option<&str>, taken: &[String]) -> Option<String> {
        if name.is_empty() {
            return Some("Enter a name".into());
        }
        if !cadrs_sketch::units::is_variable_name(name) {
            return Some("A name is letters, digits and _ (starting with a letter)".into());
        }
        if Some(name) != own
            && (self.names().iter().any(|n| n == name)
                || self.inputs.iter().any(|i| i.name == name))
        {
            return Some(format!("#{name} is already in the configuration"));
        }
        if taken.iter().any(|t| t == name) {
            return Some(format!("#{name} is already a variable"));
        }
        None
    }

    /// A name for a new column or input from `label` ("Extrude 1 · Depth" → `Extrude_1_Depth`),
    /// made unique among the configuration's names and `taken`.
    pub fn fresh_name(&self, label: &str, taken: &[String]) -> String {
        let mut base: String = label
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        while base.contains("__") {
            base = base.replace("__", "_");
        }
        let base = base.trim_matches('_').to_string();
        let base = match base.chars().next() {
            Some(c) if c.is_alphabetic() => base,
            Some(_) => format!("c_{base}"),
            None => "value".into(),
        };
        let used = |n: &str| {
            self.names().iter().any(|x| x == n)
                || self.inputs.iter().any(|i| i.name == n)
                || taken.iter().any(|t| t == n)
        };
        if !used(&base) {
            return base;
        }
        (2..)
            .map(|k| format!("{base}_{k}"))
            .find(|n| !used(n))
            .unwrap_or(base)
    }
}

/// One input of a configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigInput {
    pub id: ConfigInputId,
    /// Its name: `#name` in expressions (a Checkbox's, a Value's), and its title in the
    /// Configuration panel and node.
    pub name: String,
    pub kind: InputKind,
    /// Its configured parameters: the configuration table's columns (a List's, a Checkbox's).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ConfiguredParam>,
}

impl ConfigInput {
    /// A List input with `rows` (the first the default).
    pub fn list(name: &str, rows: &[&str]) -> Self {
        Self {
            id: ConfigInputId::new(),
            name: name.into(),
            kind: InputKind::List {
                rows: rows
                    .iter()
                    .map(|r| ListRow {
                        id: ConfigRowId::new(),
                        name: (*r).into(),
                    })
                    .collect(),
                default: 0,
            },
            params: Vec::new(),
        }
    }

    /// A Checkbox input.
    pub fn checkbox(name: &str, default: bool) -> Self {
        Self {
            id: ConfigInputId::new(),
            name: name.into(),
            kind: InputKind::Checkbox { default },
            params: Vec::new(),
        }
    }

    /// A Value input: `#name`, a length, an angle or a number, `default` (mm, degrees).
    pub fn value(name: &str, kind: ValueKind, default: f64) -> Self {
        Self {
            id: ConfigInputId::new(),
            name: name.into(),
            kind: InputKind::Value { kind, default },
            params: Vec::new(),
        }
    }

    /// Whether `#name` is a variable (a Checkbox's 1 or 0, a Value's value; a List's name is
    /// its title, its columns the variables).
    pub fn defines_variable(&self) -> bool {
        !matches!(self.kind, InputKind::List { .. })
    }

    /// The id of the Variable feature a rebuild sees for its `#name` ([`input_features`]):
    /// derived from the input's, so it is the same in every configuration and rebuild.
    pub fn feature_id(&self) -> FeatureId {
        FeatureId::from_u128(fnv(b"cadrs configuration input\0", self.id.0.as_bytes()))
    }

    /// How many values each of its configured parameters has: a List's rows, a Checkbox's
    /// two (unchecked, checked), a Value's none.
    pub fn row_count(&self) -> usize {
        match &self.kind {
            InputKind::List { rows, .. } => rows.len(),
            InputKind::Checkbox { .. } => 2,
            InputKind::Value { .. } => 0,
        }
    }

    /// The labels of its rows, as the table heads them.
    pub fn row_names(&self) -> Vec<String> {
        match &self.kind {
            InputKind::List { rows, .. } => rows.iter().map(|r| r.name.clone()).collect(),
            InputKind::Checkbox { .. } => vec!["Unchecked".into(), "Checked".into()],
            InputKind::Value { .. } => Vec::new(),
        }
    }

    pub fn default_value(&self) -> InputValue {
        match &self.kind {
            InputKind::List { rows, default } => InputValue::Row(
                rows.get(*default)
                    .or(rows.first())
                    .map(|r| r.id)
                    .unwrap_or_default(),
            ),
            InputKind::Checkbox { default } => InputValue::Checked(*default),
            InputKind::Value { default, .. } => InputValue::Number(*default),
        }
    }

    /// Whether `value` is one it can take (why not: a deleted row, the wrong kind of value).
    pub fn check_value(&self, value: &InputValue) -> Result<(), String> {
        match (&self.kind, value) {
            (InputKind::List { rows, .. }, InputValue::Row(r)) => rows
                .iter()
                .any(|x| x.id == *r)
                .then_some(())
                .ok_or_else(|| format!("{}: no such configuration", self.name)),
            (InputKind::Checkbox { .. }, InputValue::Checked(_)) => Ok(()),
            (InputKind::Value { .. }, InputValue::Number(x)) if x.is_finite() => Ok(()),
            _ => Err(format!("{}: the wrong kind of value", self.name)),
        }
    }

    /// Its value as the features see it: `value`, or its default where `value` is missing or
    /// doesn't fit.
    pub fn resolve_value(&self, value: Option<&InputValue>) -> Resolved {
        let value = value
            .filter(|v| self.check_value(v).is_ok())
            .cloned()
            .unwrap_or_else(|| self.default_value());
        match (&self.kind, value) {
            (InputKind::List { rows, default }, InputValue::Row(r)) => Resolved::Row(
                rows.iter()
                    .position(|x| x.id == r)
                    .unwrap_or((*default).min(rows.len().saturating_sub(1))),
            ),
            (InputKind::Checkbox { .. }, InputValue::Checked(c)) => Resolved::Checked(c),
            (InputKind::Value { .. }, InputValue::Number(x)) => Resolved::Number(x),
            (_, _) => Resolved::Checked(false),
        }
    }

    /// The row its value picks: a List's, a Checkbox's 0 (unchecked) or 1 (checked).
    pub fn row_of(&self, value: Option<&InputValue>) -> usize {
        match self.resolve_value(value) {
            Resolved::Row(i) => i,
            Resolved::Checked(c) => usize::from(c),
            Resolved::Number(_) => 0,
        }
    }
}

/// What kind of input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputKind {
    /// A table of named configurations.
    List {
        rows: Vec<ListRow>,
        /// The default row's index.
        #[serde(default)]
        default: usize,
    },
    /// On or off: `#name` is 1 or 0.
    Checkbox {
        #[serde(default)]
        default: bool,
    },
    /// A value typed per configuration: `#name`.
    Value {
        kind: ValueKind,
        /// mm, degrees or a number.
        #[serde(default)]
        default: f64,
    },
}

impl InputKind {
    pub fn label(&self) -> &'static str {
        match self {
            InputKind::List { .. } => "List",
            InputKind::Checkbox { .. } => "Checkbox",
            InputKind::Value { kind, .. } => kind.label(),
        }
    }
}

/// What a Value input measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ValueKind {
    /// Millimetres.
    #[default]
    Length,
    /// Degrees.
    Angle,
    /// A plain number (a count, a ratio).
    Number,
}

impl ValueKind {
    pub const ALL: [ValueKind; 3] = [ValueKind::Length, ValueKind::Angle, ValueKind::Number];

    pub fn label(self) -> &'static str {
        match self {
            ValueKind::Length => "Length",
            ValueKind::Angle => "Angle",
            ValueKind::Number => "Number",
        }
    }

    /// The kind of variable expressions see.
    pub fn variable_kind(self) -> VariableKind {
        match self {
            ValueKind::Length => VariableKind::Length,
            ValueKind::Angle => VariableKind::Angle,
            ValueKind::Number => VariableKind::Number,
        }
    }

    pub fn quantity(self) -> Quantity {
        self.variable_kind().quantity()
    }

    /// The kind for a field of quantity `q`.
    pub fn of(q: Quantity) -> Self {
        match q {
            Quantity::Length => ValueKind::Length,
            Quantity::Angle => ValueKind::Angle,
            Quantity::Count => ValueKind::Number,
        }
    }

    /// Whether a field of quantity `q` can read it.
    pub fn fits(self, q: Quantity) -> bool {
        match self {
            ValueKind::Number => q == Quantity::Count,
            k => k.quantity() == q,
        }
    }

    /// `value` as it would be typed ("16 mm", "30 deg", "3").
    pub fn plain(self, value: f64) -> String {
        plain(value, self.quantity())
    }
}

/// One row of a List: a configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListRow {
    pub id: ConfigRowId,
    pub name: String,
}

/// An input's value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputValue {
    /// A List's row.
    Row(ConfigRowId),
    /// A Checkbox's state.
    Checked(bool),
    /// A Value's value (mm, degrees or a number).
    Number(f64),
}

/// An input's value as the features see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resolved {
    /// The index of a List's row.
    Row(usize),
    Checked(bool),
    Number(f64),
}

/// The values of a configuration's inputs, by input (in input-id order, so equal values
/// compare and hash alike).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ConfigValues(pub Vec<(ConfigInputId, InputValue)>);

impl ConfigValues {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, input: ConfigInputId) -> Option<&InputValue> {
        self.0.iter().find(|(i, _)| *i == input).map(|(_, v)| v)
    }

    pub fn set(&mut self, input: ConfigInputId, value: InputValue) {
        match self.0.binary_search_by(|(i, _)| i.cmp(&input)) {
            Ok(k) => self.0[k].1 = value,
            Err(k) => self.0.insert(k, (input, value)),
        }
    }
}

/// A configured parameter: a column of the configuration table, the variable `#name` with a
/// value for each of its input's rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredParam {
    /// What it was configured for (its label, and where an edit is put back into the table); a
    /// property's column sets the property.
    pub target: ParamTarget,
    /// Its value for each of its input's rows.
    pub values: Vec<CellValue>,
    /// The variable it defines: `#name` (empty in documents from before columns were
    /// variables, named on load by [`migrate`]; a property's column has none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl ConfiguredParam {
    /// Whether it defines a variable (every column but a property's, once named).
    pub fn binds(&self) -> bool {
        !self.name.is_empty() && !matches!(self.target, ParamTarget::Property { .. })
    }

    /// What its values are, where it says without the features: a Free column's kind, a
    /// suppression's Boolean, a property's text.
    pub fn kind_hint(&self) -> Option<ColumnKind> {
        match &self.target {
            ParamTarget::Free { kind } => Some(*kind),
            ParamTarget::Suppressed { .. } => Some(ColumnKind::Boolean),
            ParamTarget::Property { .. } => Some(ColumnKind::Text),
            _ => None,
        }
    }

    /// What its values are: [`ConfiguredParam::kind_hint`], else its target's quantity.
    pub fn kind(&self, features: &[Feature]) -> ColumnKind {
        self.kind_hint().unwrap_or_else(|| {
            self.target
                .quantity(features)
                .map_or(ColumnKind::Number, ColumnKind::of)
        })
    }
}

/// What a column's values are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnKind {
    Length,
    Angle,
    Number,
    /// 1 or 0 (a feature suppressed or not).
    Boolean,
    /// A part property's text.
    Text,
}

impl ColumnKind {
    pub fn of(q: Quantity) -> Self {
        match q {
            Quantity::Length => ColumnKind::Length,
            Quantity::Angle => ColumnKind::Angle,
            Quantity::Count => ColumnKind::Number,
        }
    }

    /// What expressions see (a Boolean is a number, 1 or 0).
    pub fn quantity(self) -> Quantity {
        match self {
            ColumnKind::Length => Quantity::Length,
            ColumnKind::Angle => Quantity::Angle,
            _ => Quantity::Count,
        }
    }

    pub fn variable_kind(self) -> VariableKind {
        match self {
            ColumnKind::Length => VariableKind::Length,
            ColumnKind::Angle => VariableKind::Angle,
            _ => VariableKind::Number,
        }
    }
}

/// What a configured parameter sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParamTarget {
    /// A numeric field of a feature: the `occurrence`th field labelled `label` of
    /// [`crate::variables::slots`] ("Depth", "Radius", the second "Vertex radius").
    Field {
        feature: FeatureId,
        label: String,
        #[serde(default)]
        occurrence: u32,
    },
    /// A dimension of a sketch.
    Dimension {
        feature: FeatureId,
        dimension: cadrs_sketch::DimensionId,
    },
    /// A Variable feature's value.
    Variable { feature: FeatureId },
    /// Whether a feature is suppressed.
    Suppressed { feature: FeatureId },
    /// A part's property ([`PropertyKey::is_configurable`]): its text, empty for not set.
    Property { part: PartId, key: PropertyKey },
    /// No parameter: a variable of its own, read wherever an expression names it (a column
    /// added in the node view and wired to fields there).
    Free { kind: ColumnKind },
}

impl ParamTarget {
    /// The feature it is a parameter of.
    pub fn feature(&self) -> Option<FeatureId> {
        match self {
            ParamTarget::Field { feature, .. }
            | ParamTarget::Dimension { feature, .. }
            | ParamTarget::Variable { feature }
            | ParamTarget::Suppressed { feature } => Some(*feature),
            ParamTarget::Property { .. } | ParamTarget::Free { .. } => None,
        }
    }

    /// How the configuration table heads its column: "Extrude 1 · Depth", "Sketch 1 · D2",
    /// "Part 1 · Part number".
    pub fn label(
        &self,
        features: &[Feature],
        settings: &PropertySettings,
        part_name: &dyn Fn(PartId) -> String,
    ) -> String {
        if let ParamTarget::Property { part, key } = self {
            return format!("{} · {}", part_name(*part), key.label(settings));
        }
        if let ParamTarget::Free { kind } = self {
            return format!("{kind:?}");
        }
        let f = features.iter().find(|f| Some(f.id) == self.feature());
        let name = f.map_or("(deleted)", |f| f.name.as_str());
        match self {
            ParamTarget::Field {
                label, occurrence, ..
            } => {
                if *occurrence > 0 {
                    format!("{name} · {label} {}", occurrence + 1)
                } else {
                    format!("{name} · {label}")
                }
            }
            ParamTarget::Dimension { dimension, .. } => {
                let n = f
                    .and_then(|f| f.sketch())
                    .and_then(|s| {
                        s.geometry
                            .dimensions
                            .iter()
                            .position(|(id, _)| id == *dimension)
                    })
                    .map_or("Dimension".to_string(), |k| format!("D{}", k + 1));
                format!("{name} · {n}")
            }
            ParamTarget::Variable { .. } => format!("{name} · Value"),
            ParamTarget::Suppressed { .. } => format!("{name} · Suppressed"),
            ParamTarget::Property { .. } | ParamTarget::Free { .. } => unreachable!(),
        }
    }

    /// What its values measure (none for suppression, a property or a Free column).
    pub fn quantity(&self, features: &[Feature]) -> Option<Quantity> {
        let f = features.iter().find(|f| Some(f.id) == self.feature())?;
        match self {
            ParamTarget::Field {
                label, occurrence, ..
            } => {
                let mut k = f.kind.clone();
                with_slot(&mut k, label, *occurrence, |s| s.quantity)
            }
            ParamTarget::Dimension { dimension, .. } => f
                .sketch()?
                .geometry
                .dimensions
                .get(*dimension)
                .map(|d| d.kind.quantity()),
            ParamTarget::Variable { .. } => match &f.kind {
                FeatureKind::Variable(v) => Some(v.kind.quantity()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether its cells are text (a property's).
    pub fn is_text(&self) -> bool {
        matches!(self, ParamTarget::Property { .. })
    }
}

/// A cell of the configuration table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CellValue {
    /// Whether the feature is suppressed (a Boolean column's 1 or 0).
    Bool(bool),
    /// A value or an expression ("12 mm", "#Length - 2 mm"), a property's text.
    Expr(String),
}

impl CellValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            CellValue::Bool(b) => Some(*b),
            CellValue::Expr(_) => None,
        }
    }

    pub fn as_expr(&self) -> Option<&str> {
        match self {
            CellValue::Expr(e) => Some(e),
            CellValue::Bool(_) => None,
        }
    }
}

/// Calls `f` on the `occurrence`th slot labelled `label` of `kind` ([`ParamTarget::Field`]).
pub fn with_slot<R>(
    kind: &mut FeatureKind,
    label: &str,
    occurrence: u32,
    f: impl FnOnce(crate::variables::Slot<'_>) -> R,
) -> Option<R> {
    let mut f = Some(f);
    let mut out = None;
    let mut seen = 0;
    crate::variables::slots(kind, &mut |s| {
        if s.label == label {
            if seen == occurrence
                && let Some(f) = f.take()
            {
                out = Some(f(s));
            }
            seen += 1;
        }
    });
    out
}

/// The parameters of a feature that can be configured: its numeric fields (with their labels
/// and occurrences), a sketch's dimensions, a Variable's value, and its suppression.
pub fn configurable(feature: &Feature) -> Vec<ParamTarget> {
    let id = feature.id;
    let mut out = Vec::new();
    match &feature.kind {
        FeatureKind::Sketch(sk) => {
            for (d, _) in sk.geometry.dimensions.iter() {
                if !sk.geometry.dimensions.get(d).is_some_and(|x| x.driven) {
                    out.push(ParamTarget::Dimension {
                        feature: id,
                        dimension: d,
                    });
                }
            }
        }
        FeatureKind::Variable(_) => out.push(ParamTarget::Variable { feature: id }),
        k => {
            let mut k = k.clone();
            let mut seen: Vec<(&'static str, u32)> = Vec::new();
            crate::variables::slots(&mut k, &mut |s| {
                let n = match seen.iter_mut().find(|(l, _)| *l == s.label) {
                    Some((_, n)) => {
                        *n += 1;
                        *n
                    }
                    None => {
                        seen.push((s.label, 0));
                        0
                    }
                };
                out.push(ParamTarget::Field {
                    feature: id,
                    label: s.label.into(),
                    occurrence: n,
                });
            });
        }
    }
    out.push(ParamTarget::Suppressed { feature: id });
    out
}

/// The properties of `part` that can be configured: Name, Part number, Description, Revision,
/// Vendor, Category and the document's custom properties ([`PropertyKey::is_configurable`]).
pub fn configurable_properties(part: PartId, settings: &PropertySettings) -> Vec<ParamTarget> {
    settings
        .all_keys()
        .into_iter()
        .filter(PropertyKey::is_configurable)
        .map(|key| ParamTarget::Property { part, key })
        .collect()
}

/// The value a parameter has in the features (or a property in the parts' settings) now, as a
/// cell: a field's or dimension's expression (or its value as typed), whether a feature is
/// suppressed. `None` when the target is gone (its feature deleted, a field the feature no
/// longer has), or a Free column's.
pub fn current(
    target: &ParamTarget,
    features: &[Feature],
    suppressed: &[FeatureId],
    parts: &[PartProps],
) -> Option<CellValue> {
    if let ParamTarget::Property { part, key } = target {
        return Some(CellValue::Expr(crate::properties::part_text(
            parts, *part, *key,
        )));
    }
    let f = features.iter().find(|f| Some(f.id) == target.feature())?;
    Some(match target {
        ParamTarget::Field {
            label, occurrence, ..
        } => {
            let mut k = f.kind.clone();
            with_slot(&mut k, label, *occurrence, |s| {
                let e = s.expr.trim();
                CellValue::Expr(if e.is_empty() {
                    plain(*s.value, s.quantity)
                } else {
                    e.to_string()
                })
            })?
        }
        ParamTarget::Dimension { dimension, .. } => {
            let g = &f.sketch()?.geometry;
            let d = g.dimensions.get(*dimension)?;
            let e = g
                .expressions
                .iter()
                .find(|(id, _)| id == dimension)
                .map(|(_, e)| e.clone());
            CellValue::Expr(e.unwrap_or_else(|| plain(d.value, d.kind.quantity())))
        }
        ParamTarget::Variable { .. } => match &f.kind {
            FeatureKind::Variable(v) => CellValue::Expr(v.expr.clone()),
            _ => return None,
        },
        ParamTarget::Suppressed { feature } => CellValue::Bool(suppressed.contains(feature)),
        ParamTarget::Property { .. } | ParamTarget::Free { .. } => return None,
    })
}

/// A value as a cell's text, as it would be typed: "16 mm", "4.618802153517006 mm", "30 deg",
/// "3" (whatever the workspace units, so it reads back exactly).
pub fn plain(value: f64, q: Quantity) -> String {
    match q {
        Quantity::Length => format!("{value} mm"),
        Quantity::Angle => format!("{value} deg"),
        Quantity::Count => format!("{value}"),
    }
}

// ---------------------------------------------------------------------------------------------
// The engine: inputs and columns as variables, parameters reading them

/// The text a parameter configured by the column `#name` holds.
fn reference(name: &str) -> String {
    format!("#{name}")
}

/// The suppression rule of a feature whose suppression a Boolean column `#name` configures:
/// suppressed while the column's value is 1.
fn suppression(name: &str) -> crate::variables::SuppressByVariable {
    crate::variables::SuppressByVariable {
        expr: reference(name),
        invert: true,
    }
}

/// Whether the parameter `target` reads `#name` (as [`bind`] makes it).
fn is_bound(target: &ParamTarget, name: &str, features: &[Feature]) -> bool {
    let Some(f) = features.iter().find(|f| Some(f.id) == target.feature()) else {
        return false;
    };
    let r = reference(name);
    match target {
        ParamTarget::Field {
            label, occurrence, ..
        } => {
            let mut k = f.kind.clone();
            with_slot(&mut k, label, *occurrence, |s| s.expr.trim() == r).unwrap_or(false)
        }
        ParamTarget::Dimension { dimension, .. } => f.sketch().is_some_and(|s| {
            s.geometry
                .expressions
                .iter()
                .any(|(d, e)| d == dimension && e.trim() == r)
        }),
        ParamTarget::Variable { .. } => {
            matches!(&f.kind, FeatureKind::Variable(v) if v.expr.trim() == r)
        }
        ParamTarget::Suppressed { .. } => f.suppress_by.as_ref() == Some(&suppression(name)),
        ParamTarget::Property { .. } | ParamTarget::Free { .. } => true,
    }
}

/// Makes the parameter `target` read `#name` (a feature's field, a sketch dimension, a
/// Variable's value; a feature's suppression by it). The values themselves follow from the
/// variables' refresh.
fn bind(
    target: &ParamTarget,
    name: &str,
    features: &mut [Feature],
    suppressed: &mut Vec<FeatureId>,
) {
    let Some(f) = features.iter_mut().find(|f| Some(f.id) == target.feature()) else {
        return;
    };
    let r = reference(name);
    match target {
        ParamTarget::Field {
            label, occurrence, ..
        } => {
            with_slot(&mut f.kind, label, *occurrence, |s| *s.expr = r.clone());
        }
        ParamTarget::Dimension { dimension, .. } => {
            if let Some(sk) = f.sketch_mut() {
                let g = &mut sk.geometry;
                g.expressions.retain(|(d, _)| d != dimension);
                g.expressions.push((*dimension, r));
            }
        }
        ParamTarget::Variable { .. } => {
            if let FeatureKind::Variable(v) = &mut f.kind {
                v.expr = r;
            }
        }
        ParamTarget::Suppressed { feature } => {
            f.suppress_by = Some(suppression(name));
            suppressed.retain(|x| x != feature);
        }
        ParamTarget::Property { .. } | ParamTarget::Free { .. } => {}
    }
}

/// Makes the parameter `target` hold `value` itself (it is no longer configured): a field's or
/// dimension's value, a Variable's expression, a feature suppressed or not.
fn unbind(
    target: &ParamTarget,
    value: &CellValue,
    features: &mut [Feature],
    suppressed: &mut Vec<FeatureId>,
    units: &Units,
) {
    let Some(f) = features.iter_mut().find(|f| Some(f.id) == target.feature()) else {
        return;
    };
    match (target, value) {
        (
            ParamTarget::Field {
                label, occurrence, ..
            },
            CellValue::Expr(e),
        ) => {
            with_slot(&mut f.kind, label, *occurrence, |s| {
                // A plain value, as the field shows it (an expression naming other variables
                // stays, for the refresh to evaluate).
                if e.contains('#') {
                    *s.expr = e.clone();
                } else if let Ok(v) = units.eval(e, s.quantity) {
                    *s.value = v;
                    *s.expr = VariableKind::of(s.quantity).literal(v);
                }
            });
        }
        (ParamTarget::Dimension { dimension, .. }, CellValue::Expr(e)) => {
            if let Some(sk) = f.sketch_mut() {
                let g = &mut sk.geometry;
                g.expressions.retain(|(id, _)| id != dimension);
                if e.contains('#') {
                    g.expressions.push((*dimension, e.clone()));
                } else if let Some(q) = g.dimensions.get(*dimension).map(|d| d.kind.quantity())
                    && let Ok(value) = units.eval(e, q)
                {
                    let before = g.clone();
                    let op = cadrs_sketch::SketchOp::SetDimensionValue {
                        id: *dimension,
                        value,
                    };
                    if op.apply(g).is_err() {
                        *g = before;
                    }
                }
            }
        }
        (ParamTarget::Variable { .. }, CellValue::Expr(e)) => {
            if let FeatureKind::Variable(v) = &mut f.kind {
                v.expr = e.clone();
            }
        }
        (ParamTarget::Suppressed { feature }, CellValue::Bool(b)) => {
            f.suppress_by = None;
            let has = suppressed.contains(feature);
            if *b && !has {
                suppressed.push(*feature);
            } else if !*b && has {
                suppressed.retain(|x| x != feature);
            }
        }
        _ => {}
    }
}

/// The Variable features the inputs give the list (ahead of it, after the Variable Studios'),
/// in the configuration `values`: each Checkbox's and Value's `#name`, then each column's
/// `#name` with its row's value. Their ids are derived from the inputs' and the names, so a
/// rebuild sees the same features in every configuration.
pub fn input_features(cfg: &Configuration, values: &ConfigValues) -> Vec<Feature> {
    let units = Units::default();
    let mut env: Vec<(String, VarValue)> = Vec::new();
    let mut out = Vec::new();
    let mut push = |name: &str, kind: VariableKind, value: f64, id: FeatureId, env: &mut Vec<_>| {
        let var = VariableFeature {
            name: name.to_string(),
            var_type: kind.var_type(),
            expr: kind.literal(value),
            value,
            kind,
            description: String::new(),
        };
        env.push((name.to_string(), var.var_value()));
        out.push(Feature::new(
            id,
            format!("{INPUT_FEATURE}{name}"),
            FeatureKind::Variable(var),
        ));
    };
    for (input, r) in cfg.inputs.iter().zip(cfg.resolve(values)) {
        match (&input.kind, r) {
            (InputKind::Checkbox { .. }, Resolved::Checked(c)) => push(
                &input.name,
                VariableKind::Number,
                if c { 1.0 } else { 0.0 },
                input.feature_id(),
                &mut env,
            ),
            (InputKind::Value { kind, .. }, Resolved::Number(x)) => push(
                &input.name,
                kind.variable_kind(),
                x,
                input.feature_id(),
                &mut env,
            ),
            _ => {}
        }
        let row = match r {
            Resolved::Row(i) => i,
            Resolved::Checked(c) => usize::from(c),
            Resolved::Number(_) => continue,
        };
        for p in input.params.iter().filter(|p| p.binds()) {
            let Some(cell) = p.values.get(row) else {
                continue;
            };
            // (A column's kind without the features: a Boolean's or Free's from its target,
            // else what its cell reads as.)
            let kind = p.kind_hint().unwrap_or(ColumnKind::Number);
            let (value, vk) = match cell {
                CellValue::Bool(b) => (if *b { 1.0 } else { 0.0 }, VariableKind::Number),
                CellValue::Expr(e) => {
                    let q = cell_quantity(e).unwrap_or(kind.quantity());
                    match units.eval_vars(e, q, &env) {
                        Ok(v) => (v, ColumnKind::of(q).variable_kind()),
                        Err(_) => continue,
                    }
                }
            };
            push(
                &p.name,
                vk,
                value,
                column_feature_id(input.id, &p.name),
                &mut env,
            );
        }
    }
    out
}

/// What a cell's text measures, from its unit ("16 mm", "30 deg"): `None` for a bare number
/// or an expression (the column's kind then).
fn cell_quantity(text: &str) -> Option<Quantity> {
    for q in [Quantity::Length, Quantity::Angle] {
        if cadrs_sketch::units::eval(text, Quantity::Count).is_err()
            && cadrs_sketch::units::eval(text, q).is_ok()
        {
            return Some(q);
        }
    }
    None
}

/// The id of the Variable feature a rebuild sees for a column ([`input_features`]).
fn column_feature_id(input: ConfigInputId, name: &str) -> FeatureId {
    let mut data = input.0.as_bytes().to_vec();
    data.extend_from_slice(name.as_bytes());
    FeatureId::from_u128(fnv(b"cadrs configuration column\0", &data))
}

/// The inputs' variables, as expressions see them ([`input_features`]'s values).
pub fn input_variables(cfg: &Configuration, values: &ConfigValues) -> Vec<(String, VarValue)> {
    input_features(cfg, values)
        .into_iter()
        .filter_map(|f| match f.kind {
            FeatureKind::Variable(v) => Some((v.name.clone(), v.var_value())),
            _ => None,
        })
        .collect()
}

/// The name the Variable feature a rebuild sees for an input or a column has, before its name:
/// how [`crate::variables::check`] tells an input from a Variable (a Variable may not redefine
/// one: the configurations would stop reaching the features below it).
pub const INPUT_FEATURE: &str = "configuration #";

/// Writes the configured properties' cells for `values` into the parts' settings (the
/// configuration shown, or a copy's).
pub fn apply_properties(cfg: &Configuration, values: &ConfigValues, parts: &mut Vec<PartProps>) {
    for input in &cfg.inputs {
        let row = input.row_of(values.get(input.id));
        for p in &input.params {
            if let (ParamTarget::Property { part, key }, Some(CellValue::Expr(t))) =
                (&p.target, p.values.get(row))
            {
                crate::properties::set_part_text(parts, *part, *key, t);
            }
        }
    }
}

/// Makes a Part Studio show `values`: its configuration's active values, its properties, and
/// the features' values through the refresh.
pub fn activate(
    doc: &mut Document,
    element: ElementId,
    values: &ConfigValues,
) -> Result<(), CommandError> {
    let el = doc
        .element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?;
    let values = el.configuration.complete(values);
    el.configuration.active = values.clone();
    let cfg = el.configuration.clone();
    let ElementKind::PartStudio { parts, .. } = &mut el.kind else {
        return Err(CommandError::Invalid(
            "configurations need a Part Studio".into(),
        ));
    };
    apply_properties(&cfg, &values, parts);
    parts.retain(|p| !p.is_default());
    crate::commands::refresh_studio(doc, element);
    Ok(())
}

/// After a command ([`crate::command::History::execute`]), in every configured Part Studio: a
/// configured parameter edited in place (a depth typed into its dialog, a dimension edited in
/// its sketch, a configured feature suppressed from the list) has the new value put into the
/// active configuration's cell and reads `#name` again; a parameter that is gone (its feature
/// deleted) takes its column with it; a property edited goes into its cell. Returns whether
/// anything changed (the studios refreshed then).
pub fn after_command(doc: &mut Document) -> bool {
    migrate(doc);
    let units = doc.units;
    let mut refresh = Vec::new();
    for el in &mut doc.elements {
        if el.configuration.is_empty() {
            continue;
        }
        let active = el.configuration.active.clone();
        let mut cfg = std::mem::take(&mut el.configuration);
        let mut changed = false;
        if let ElementKind::PartStudio {
            features,
            suppressed,
            parts,
            ..
        } = &mut el.kind
        {
            for input in &mut cfg.inputs {
                let row = input.row_of(active.get(input.id));
                let before = input.params.len();
                input.params.retain(|p| match p.target.feature() {
                    Some(id) => features.iter().any(|f| f.id == id),
                    None => true,
                });
                changed |= input.params.len() != before;
                for p in &mut input.params {
                    if let ParamTarget::Property { .. } = p.target {
                        if let Some(now) = current(&p.target, features, suppressed, parts)
                            && let Some(cell) = p.values.get_mut(row)
                            && *cell != now
                        {
                            *cell = now;
                            changed = true;
                        }
                        continue;
                    }
                    if !p.binds() || p.target.feature().is_none() {
                        continue;
                    }
                    if is_bound(&p.target, &p.name, features)
                        && !suppressed_now(&p.target, suppressed)
                    {
                        continue;
                    }
                    // Edited in place: the new value is this configuration's. (A configured
                    // feature suppressed from the list is suppressed in it; its suppression
                    // rule removed, it is not.)
                    let now = match &p.target {
                        ParamTarget::Suppressed { feature } => {
                            Some(CellValue::Bool(suppressed.contains(feature)))
                        }
                        t => current(t, features, suppressed, parts),
                    };
                    if let Some(now) = now
                        && let Some(cell) = p.values.get_mut(row)
                    {
                        let now = normalize(&now, p.target.quantity(features), &units);
                        if !same_value(cell, &now, p.target.quantity(features), &units) {
                            *cell = now;
                        }
                    }
                    bind(&p.target, &p.name, features, suppressed);
                    changed = true;
                }
            }
        }
        el.configuration = cfg;
        if changed {
            refresh.push(el.id);
        }
    }
    for id in &refresh {
        crate::commands::refresh_studio(doc, *id);
    }
    !refresh.is_empty()
}

/// Whether the feature a Suppressed column configures was suppressed from the list (in the
/// studio's suppressed list again).
fn suppressed_now(target: &ParamTarget, suppressed: &[FeatureId]) -> bool {
    matches!(target, ParamTarget::Suppressed { feature } if suppressed.contains(feature))
}

/// A cell's text as kept: a bare number, typed in the workspace's units, written with its
/// unit ("16" in inches → "406.4 mm"), so it reads the same whatever the units; anything else
/// ("16 mm", "10 mm / cos(60 deg)", "#d / 2") stays as typed.
fn normalize(cell: &CellValue, q: Option<Quantity>, units: &Units) -> CellValue {
    match (cell, q) {
        (CellValue::Expr(e), Some(q)) if e.trim().parse::<f64>().is_ok() => {
            match units.eval(e, q) {
                Ok(v) => CellValue::Expr(plain(v, q)),
                Err(_) => cell.clone(),
            }
        }
        _ => cell.clone(),
    }
}

/// Whether two cells give the same value: plain expressions (no `#`) that evaluate alike.
fn same_value(a: &CellValue, b: &CellValue, q: Option<Quantity>, units: &Units) -> bool {
    if a == b {
        return true;
    }
    let (CellValue::Expr(a), CellValue::Expr(b), Some(q)) = (a, b, q) else {
        return false;
    };
    if a.contains('#') || b.contains('#') {
        return false;
    }
    match (units.eval(a, q), units.eval(b, q)) {
        (Ok(x), Ok(y)) => (x - y).abs() <= 1e-9 * x.abs().max(1.0),
        _ => false,
    }
}

/// Documents from before columns were variables (RTnhN's fork: a column's values were written
/// into its parameter): each such column named after its parameter, the parameter reading
/// `#name` (it had the active configuration's value, which `#name` gives it again). Run on load
/// and after every command; a configuration without such columns is left alone.
pub fn migrate(doc: &mut Document) {
    for el in &mut doc.elements {
        if !el.configuration.inputs.iter().any(|i| {
            i.params
                .iter()
                .any(|p| p.name.is_empty() && !p.target.is_text())
        }) {
            continue;
        }
        let taken = taken_names(el);
        let settings = doc.properties.clone();
        let mut cfg = std::mem::take(&mut el.configuration);
        if let ElementKind::PartStudio {
            features,
            suppressed,
            ..
        } = &mut el.kind
        {
            for k in 0..cfg.inputs.len() {
                for j in 0..cfg.inputs[k].params.len() {
                    let p = &cfg.inputs[k].params[j];
                    if !p.name.is_empty() || p.target.is_text() {
                        continue;
                    }
                    let label = p.target.label(features, &settings, &|_| String::new());
                    let name = cfg.fresh_name(&label, &taken);
                    cfg.inputs[k].params[j].name = name.clone();
                    let target = cfg.inputs[k].params[j].target.clone();
                    bind(&target, &name, features, suppressed);
                }
            }
        }
        el.configuration = cfg;
    }
}

// ---------------------------------------------------------------------------------------------
// The node view's layout

/// Where the node view puts a Part Studio's nodes (graph coordinates, px at zoom 1), and which
/// are collapsed. Features without a position are laid out by the app.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeLayout {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub positions: BTreeMap<FeatureId, [f32; 2]>,
    /// The Configuration node's position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs_node: Option<[f32; 2]>,
    /// The Parts node's position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts_node: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub collapsed: BTreeSet<FeatureId>,
}

impl NodeLayout {
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
            && self.inputs_node.is_none()
            && self.parts_node.is_none()
            && self.collapsed.is_empty()
    }
}

/// The names a Part Studio's features define as Variables, and its Variable Studios'
/// variables: what an input or column can't be called.
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
// Copies: assembly instances, drawing views, Render Studios

/// An assembly instance's configuration (or a drawing view's, a Render Studio's model's): the
/// Part Studio it comes from and its values. The reference names the configured copy
/// ([`copy_id`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstanceConfiguration {
    pub studio: ElementId,
    pub values: ConfigValues,
}

impl InstanceConfiguration {
    /// The configured copy's id.
    pub fn copy_id(&self) -> ElementId {
        copy_id(self.studio, &self.values)
    }
}

/// A Part Studio built in a configuration other than its own (not a tab;
/// [`Document::element`] finds it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredStudio {
    pub studio: ElementId,
    pub values: ConfigValues,
    /// The studio in the values, under [`copy_id`].
    pub element: Element,
    /// A hash of what it was generated from (the studio's model and the values): regenerated
    /// when that changes.
    #[serde(default)]
    pub stamp: u64,
}

/// The id of `studio`'s copy in the configuration `values` (the same for the same values, so
/// instances share it, and undo and redo find it again).
pub fn copy_id(studio: ElementId, values: &ConfigValues) -> ElementId {
    let mut h = blake3::Hasher::new();
    h.update(b"cadrs configured studio");
    h.update(studio.0.as_bytes());
    h.update(ron::to_string(values).unwrap_or_default().as_bytes());
    let b = h.finalize();
    let mut x = [0u8; 16];
    x.copy_from_slice(&b.as_bytes()[..16]);
    // A version 4 UUID's bits, as ids are.
    ElementId(uuid::Builder::from_random_bytes(x).into_uuid())
}

/// The studio's model as its copies are built from it: without the configuration it shows or
/// where its nodes are (neither changes a copy, so neither regenerates them).
fn model_text(source: &Element) -> String {
    let mut el = source.clone();
    el.configuration.active = ConfigValues::default();
    if let ElementKind::PartStudio { node_layout, .. } = &mut el.kind {
        *node_layout = None;
    }
    ron::to_string(&el).unwrap_or_default()
}

fn stamp(model: &str, values: &ConfigValues, units: &Units) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(model.as_bytes());
    h.update(ron::to_string(values).unwrap_or_default().as_bytes());
    h.update(ron::to_string(units).unwrap_or_default().as_bytes());
    let b = h.finalize();
    u64::from_le_bytes(b.as_bytes()[..8].try_into().unwrap_or_default())
}

/// `source` built in the configuration `values`, under `id`.
pub fn generate(source: &Element, values: &ConfigValues, id: ElementId, units: &Units) -> Element {
    let mut el = source.clone();
    el.id = id;
    el.contexts.clear();
    el.open_context = None;
    el.configuration.active = el.configuration.complete(values);
    let cfg = el.configuration.clone();
    // The parts' names and properties set for this configuration (a configured instance's),
    // then the configured properties' cells over them.
    let own: Vec<ConfiguredPartProps> = cfg
        .part_properties
        .iter()
        .filter(|p| p.values == cfg.active)
        .cloned()
        .collect();
    if let Some(props) = el.part_props_mut() {
        for o in own {
            let i = match props.iter().position(|p| p.part == o.part) {
                Some(i) => i,
                None => {
                    props.push(crate::document::PartProps::new(o.part));
                    props.len() - 1
                }
            };
            if o.name.is_some() {
                props[i].name = o.name;
            }
            props[i].properties = o.properties;
        }
    }
    let base = el.variable_base();
    let studio = el.studio_features();
    if let ElementKind::PartStudio {
        features,
        suppressed,
        rollback,
        parts,
        node_layout,
        ..
    } = &mut el.kind
    {
        *rollback = None;
        *node_layout = None;
        apply_properties(&cfg, &cfg.active, parts);
        parts.retain(|p| !p.is_default());
        crate::variables::refresh(&base, features, suppressed, units);
        crate::parts::regenerate_now(&studio, features);
    }
    el
}

/// The configured copy `element` is, if it is one: its studio and values.
pub fn copy_of(doc: &Document, element: ElementId) -> Option<(ElementId, ConfigValues)> {
    doc.configured
        .iter()
        .find(|c| c.element.id == element)
        .map(|c| (c.studio, c.values.clone()))
}

/// How a part's configuration reads (the BOM's Configuration column): a configured copy's
/// values, a configured studio's active ones ("M5, 20, Hole"); empty for a studio without one.
pub fn describe_element(doc: &Document, element: ElementId) -> String {
    let (studio, values) = match copy_of(doc, element) {
        Some(x) => x,
        None => match doc.element(element) {
            Some(e) => (e.id, e.configuration.active.clone()),
            None => return String::new(),
        },
    };
    doc.elements
        .iter()
        .find(|e| e.id == studio)
        .map(|e| e.configuration.describe(&values))
        .unwrap_or_default()
}

/// Every configured copy made and up to date (an assembly instance's, a drawing view's, a
/// Render Studio's), each instance's source naming its copy, and the copies nothing names
/// dropped (after every command, undo and redo, and when a document opens).
pub fn refresh_copies(doc: &mut Document) {
    migrate(doc);
    if doc.configured.is_empty()
        && !doc.elements.iter().any(|e| {
            !e.configured_sources.is_empty()
                || e.assembly_model()
                    .is_some_and(|a| a.instances.iter().any(|i| i.configuration.is_some()))
        })
    {
        return;
    }
    let units = doc.units;
    let mut wanted: Vec<(ElementId, ConfigValues)> = Vec::new();
    for el in &mut doc.elements {
        // A drawing's or a Render Studio's configured references (the ones still named).
        if !el.configured_sources.is_empty() {
            let named = references(el);
            el.configured_sources
                .retain(|c| named.contains(&c.copy_id()));
            for c in &el.configured_sources {
                if !wanted.iter().any(|(s, v)| *s == c.studio && *v == c.values) {
                    wanted.push((c.studio, c.values.clone()));
                }
            }
        }
        let Some(a) = el.assembly_model_mut() else {
            continue;
        };
        for i in &mut a.instances {
            let Some(c) = &i.configuration else {
                continue;
            };
            let id = c.copy_id();
            set_source_element(&mut i.source, id);
            if !wanted.iter().any(|(s, v)| *s == c.studio && *v == c.values) {
                wanted.push((c.studio, c.values.clone()));
            }
        }
    }
    let mut copies = std::mem::take(&mut doc.configured);
    copies.retain(|c| wanted.iter().any(|(s, v)| *s == c.studio && *v == c.values));
    // Each studio's model once, however many configurations of it are used.
    let mut texts: Vec<(ElementId, String)> = Vec::new();
    for (studio, values) in wanted {
        let Some(source) = doc.elements.iter().find(|e| e.id == studio) else {
            continue;
        };
        if !matches!(source.kind, ElementKind::PartStudio { .. }) {
            continue;
        }
        if !texts.iter().any(|(s, _)| *s == studio) {
            texts.push((studio, model_text(source)));
        }
        let text = texts
            .iter()
            .find(|(s, _)| *s == studio)
            .map_or("", |(_, t)| t.as_str());
        let st = stamp(text, &values, &units);
        match copies
            .iter_mut()
            .find(|c| c.studio == studio && c.values == values)
        {
            Some(c) if c.stamp == st => {}
            Some(c) => {
                c.element = generate(source, &values, c.element.id, &units);
                c.stamp = st;
            }
            None => copies.push(ConfiguredStudio {
                studio,
                element: generate(source, &values, copy_id(studio, &values), &units),
                values,
                stamp: st,
            }),
        }
    }
    doc.configured = copies;
}

/// The elements `el` references that can be configured copies: a drawing's views and sheets,
/// a Render Studio's model.
pub fn references(el: &Element) -> Vec<ElementId> {
    match &el.kind {
        ElementKind::Drawing(d) => {
            let mut out: Vec<ElementId> = Vec::new();
            let mut add = |id: uuid::Uuid| {
                if !out.contains(&ElementId(id)) {
                    out.push(ElementId(id));
                }
            };
            for s in &d.sheets {
                if let Some(r) = &s.reference {
                    add(r.element);
                }
                for v in &s.views {
                    add(v.reference.element);
                }
            }
            // (Not its recorded sources: a source no view shows any more is never read.)
            out
        }
        ElementKind::Render(r) => r.source.into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The configuration `element` names when it is a configured copy (of an assembly instance, a
/// drawing view, a Render Studio's model): its studio and values.
pub fn configuration_of(doc: &Document, element: ElementId) -> Option<InstanceConfiguration> {
    copy_of(doc, element).map(|(studio, values)| InstanceConfiguration { studio, values })
}

/// The configuration a reference to `studio` takes: `values` completed, when the studio is a
/// configured Part Studio tab (`None` otherwise, or when `studio` is itself a copy).
pub fn for_studio(
    doc: &Document,
    studio: ElementId,
    values: &ConfigValues,
) -> Option<InstanceConfiguration> {
    let el = doc.elements.iter().find(|e| e.id == studio)?;
    if el.configuration.is_empty() || !matches!(el.kind, ElementKind::PartStudio { .. }) {
        return None;
    }
    Some(InstanceConfiguration {
        studio,
        values: el.configuration.complete(values),
    })
}

/// Makes `c`'s configured copy now (if missing), so a command can look at it before
/// [`refresh_copies`] runs.
pub fn ensure_copy(doc: &mut Document, c: &InstanceConfiguration) -> Result<(), CommandError> {
    let id = c.copy_id();
    if doc.configured.iter().any(|x| x.element.id == id) {
        return Ok(());
    }
    let source = doc
        .elements
        .iter()
        .find(|e| e.id == c.studio && matches!(e.kind, ElementKind::PartStudio { .. }))
        .ok_or_else(|| CommandError::Invalid("no such Part Studio".into()))?;
    if source.configuration.is_empty() {
        return Err(CommandError::Invalid(format!(
            "{} has no configuration",
            source.name
        )));
    }
    let units = doc.units;
    let text = model_text(source);
    let element = generate(source, &c.values, id, &units);
    doc.configured.push(ConfiguredStudio {
        studio: c.studio,
        values: c.values.clone(),
        element,
        stamp: stamp(&text, &c.values, &units),
    });
    Ok(())
}

/// `doc` with `c`'s configured copy, for a preview of a reference not made yet (a drawing
/// view's ghost); `None` when `doc` has it already.
pub fn with_copy(doc: &Document, c: &InstanceConfiguration) -> Option<Document> {
    if doc.element(c.copy_id()).is_some() {
        return None;
    }
    let mut d = doc.clone();
    ensure_copy(&mut d, c).ok()?;
    Some(d)
}

/// Another command whose references name `configuration`'s copy (a drawing view placed in a
/// configuration, a Render Studio's model): `element` keeps the configuration
/// ([`Element::configured_sources`]) and the copy is made before the command runs, in one undo
/// step. The command's scope must be `element`'s.
#[derive(Debug, Clone)]
pub struct WithConfiguration<C: Command> {
    pub element: ElementId,
    pub configuration: InstanceConfiguration,
    pub command: C,
}

impl<C: Command> Command for WithConfiguration<C> {
    fn label(&self) -> String {
        self.command.label()
    }
    fn scope(&self) -> Scope {
        self.command.scope()
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        ensure_copy(doc, &self.configuration)?;
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if !el.configured_sources.contains(&self.configuration) {
            el.configured_sources.push(self.configuration.clone());
        }
        self.command.apply(doc)
    }
}

fn set_source_element(source: &mut crate::assembly::InstanceSource, id: ElementId) {
    use crate::assembly::InstanceSource as S;
    match source {
        S::Part { element, .. } | S::Studio { element, .. } => *element = id,
        S::Assembly { .. } => {}
    }
}

/// The assemblies with instances whose configuration is of `studio` (and, given, that row of
/// `input`), by name, once each.
pub fn instances_using(
    doc: &Document,
    studio: ElementId,
    row: Option<(ConfigInputId, ConfigRowId)>,
) -> Vec<String> {
    let mut out = Vec::new();
    for el in &doc.elements {
        let Some(a) = el.assembly_model() else {
            continue;
        };
        for i in &a.instances {
            if let Some(c) = &i.configuration
                && c.studio == studio
                && row.is_none_or(|(input, r)| c.values.get(input) == Some(&InputValue::Row(r)))
            {
                if !out.contains(&el.name) {
                    out.push(el.name.clone());
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Commands

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

/// Writes the value of every expression that names `name` in its place (`name` is going
/// away): fields keep their values as numbers, suppression by it is removed, dimensions keep
/// their values.
fn bake_name(features: &mut [Feature], name: &str) {
    for feat in features.iter_mut() {
        if feat.suppress_by.as_ref().is_some_and(|r| {
            cadrs_sketch::units::variable_names(&r.expr)
                .iter()
                .any(|n| n == name)
        }) {
            feat.suppress_by = None;
        }
        match &mut feat.kind {
            FeatureKind::Sketch(sk) => {
                sk.geometry.expressions.retain(|(_, e)| {
                    !cadrs_sketch::units::variable_names(e)
                        .iter()
                        .any(|n| n == name)
                });
            }
            FeatureKind::Variable(v) => {
                if cadrs_sketch::units::variable_names(&v.expr)
                    .iter()
                    .any(|n| n == name)
                {
                    v.expr = v.kind.literal(v.value);
                }
            }
            k => crate::variables::slots(k, &mut |s| {
                if cadrs_sketch::units::variable_names(s.expr)
                    .iter()
                    .any(|n| n == name)
                {
                    *s.expr = VariableKind::of(s.quantity).literal(*s.value);
                }
            }),
        }
    }
}

/// Replaces a Part Studio's configuration (its inputs, rows and table: the Configuration
/// panel's and the node's edits), then shows its active configuration (kept where it still
/// exists). The expressions follow: a parameter configured anew reads its column's `#name`
/// (a column without a name is given one), a column or input renamed is renamed wherever it is
/// read, and one removed leaves what read it the value it shows.
#[derive(Debug, Clone)]
pub struct EditConfiguration {
    pub element: ElementId,
    pub configuration: Configuration,
    /// The undo label ("Add configuration input", "Edit configuration table").
    pub label: String,
}

impl Command for EditConfiguration {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if !matches!(el.kind, ElementKind::PartStudio { .. }) {
            return Err(CommandError::Invalid(
                "configurations need a Part Studio".into(),
            ));
        }
        let taken = taken_names(el);
        let old = el.configuration.clone();
        let mut configuration = self.configuration.clone();
        // Columns without a name: named after their parameter.
        for k in 0..configuration.inputs.len() {
            for j in 0..configuration.inputs[k].params.len() {
                let p = &configuration.inputs[k].params[j];
                if p.name.is_empty() && !p.target.is_text() {
                    let label = p
                        .target
                        .label(el.features(), &doc.properties, &|_| String::new());
                    let name = configuration.fresh_name(&label, &taken);
                    configuration.inputs[k].params[j].name = name;
                }
            }
        }
        if let Some(p) = configuration.problem() {
            return Err(CommandError::Invalid(p));
        }
        if let Some(n) = configuration.names().iter().find(|n| taken.contains(n)) {
            return Err(CommandError::Invalid(format!("#{n} is already a variable")));
        }
        check_properties(&mut configuration, &doc.properties)?;
        for input in &configuration.inputs {
            for p in &input.params {
                if let Some(id) = p.target.feature()
                    && !el.features().iter().any(|f| f.id == id)
                {
                    return Err(CommandError::Invalid(format!(
                        "{}: a configured feature is not in the Part Studio",
                        input.name
                    )));
                }
            }
        }
        // Cells typed in the workspace's units, kept with their units.
        let units = doc.units;
        let features = el.features().to_vec();
        for input in &mut configuration.inputs {
            for p in &mut input.params {
                let q = p.target.quantity(&features).or(match p.target {
                    ParamTarget::Free { kind } if kind != ColumnKind::Boolean => {
                        Some(kind.quantity())
                    }
                    _ => None,
                });
                for v in &mut p.values {
                    *v = normalize(v, q, &units);
                }
            }
        }
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        let ElementKind::PartStudio {
            features,
            suppressed,
            ..
        } = &mut el.kind
        else {
            unreachable!()
        };
        // Renamed: inputs by id, columns by target (a Free column keeps its name; renaming one
        // is [`RenameConfigName`]).
        let mut renames: Vec<(String, String)> = Vec::new();
        for i in &configuration.inputs {
            if let Some(o) = old.input(i.id)
                && o.defines_variable()
                && i.defines_variable()
                && o.name != i.name
            {
                renames.push((o.name.clone(), i.name.clone()));
            }
            for p in i.params.iter().filter(|p| p.binds()) {
                if let Some((_, op)) = old.param(&p.target)
                    && op.binds()
                    && op.name != p.name
                    && !matches!(p.target, ParamTarget::Free { .. })
                {
                    renames.push((op.name.clone(), p.name.clone()));
                }
            }
        }
        for (from, to) in &renames {
            rewrite_expressions(features, &|e| rename_in(e, from, to));
        }
        // Gone: what read them keeps the value it shows.
        let now_names = configuration.names();
        for i in &old.inputs {
            let row = i.row_of(old.active.get(i.id));
            if i.defines_variable()
                && !now_names.contains(&i.name)
                && !renames.iter().any(|(f, _)| *f == i.name)
            {
                bake_name(features, &i.name);
            }
            for p in i.params.iter().filter(|p| p.binds()) {
                let kept = configuration.param(&p.target).is_some()
                    && !matches!(p.target, ParamTarget::Free { .. })
                    || now_names.contains(&p.name);
                if kept {
                    continue;
                }
                if let Some(v) = p.values.get(row)
                    && p.target.feature().is_some()
                {
                    unbind(&p.target, v, features, suppressed, &units);
                }
                bake_name(features, &p.name);
            }
        }
        // New: the parameters read their columns.
        for i in &configuration.inputs {
            for p in i.params.iter().filter(|p| p.binds()) {
                if !is_bound(&p.target, &p.name, features) {
                    bind(&p.target, &p.name, features, suppressed);
                }
            }
        }
        let active = configuration.active.clone();
        el.configuration = configuration;
        activate(doc, self.element, &active)
    }
}

/// Checks the configured properties' cells and writes them as they are kept: a property that
/// can be configured, a custom one defined in the document with values of its kind (a
/// Boolean's "yes" kept as "true"), text trimmed.
fn check_properties(
    cfg: &mut Configuration,
    settings: &PropertySettings,
) -> Result<(), CommandError> {
    for input in &mut cfg.inputs {
        for p in &mut input.params {
            let ParamTarget::Property { key, .. } = p.target else {
                continue;
            };
            if !key.is_configurable() {
                return Err(CommandError::Invalid(format!(
                    "{}: {} can't be configured",
                    input.name,
                    key.label(settings)
                )));
            }
            for v in &mut p.values {
                let CellValue::Expr(t) = v else {
                    return Err(CommandError::Invalid(format!(
                        "{}: a property's value is text",
                        input.name
                    )));
                };
                *t = match key {
                    PropertyKey::Custom(id) => settings
                        .definition(id)
                        .ok_or_else(|| CommandError::Invalid("no such property".into()))?
                        .kind
                        .check(t)
                        .map_err(|e| {
                            CommandError::Invalid(format!("{}: {e}", key.label(settings)))
                        })?,
                    _ => t.trim().to_string(),
                };
            }
        }
    }
    Ok(())
}

/// Renames an input's or a column's `#name` (whatever its target), wherever it is read.
#[derive(Debug, Clone)]
pub struct RenameConfigName {
    pub element: ElementId,
    pub from: String,
    pub to: String,
}

impl Command for RenameConfigName {
    fn label(&self) -> String {
        format!("Rename #{} to #{}", self.from, self.to)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        let taken = taken_names(el);
        if let Some(p) = el
            .configuration
            .name_problem(&self.to, Some(&self.from), &taken)
        {
            return Err(CommandError::Invalid(p));
        }
        let mut found = false;
        for i in &mut el.configuration.inputs {
            if i.defines_variable() && i.name == self.from {
                i.name = self.to.clone();
                found = true;
            }
            for p in &mut i.params {
                if p.binds() && p.name == self.from {
                    p.name = self.to.clone();
                    found = true;
                }
            }
        }
        if !found {
            return Err(CommandError::Invalid(format!(
                "no #{} in the configuration",
                self.from
            )));
        }
        if let Some(fs) = el.features_mut() {
            rewrite_expressions(fs, &|e| rename_in(e, &self.from, &self.to));
        }
        crate::commands::refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Picks the configuration a Part Studio shows.
#[derive(Debug, Clone)]
pub struct SetActiveConfiguration {
    pub element: ElementId,
    pub values: ConfigValues,
}

impl Command for SetActiveConfiguration {
    fn label(&self) -> String {
        "Change configuration".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        for (id, v) in &self.values.0 {
            let input = el
                .configuration
                .input(*id)
                .ok_or_else(|| CommandError::Invalid("no such configuration input".into()))?;
            input.check_value(v).map_err(CommandError::Invalid)?;
        }
        let mut values = el.configuration.active.clone();
        for (id, v) in &self.values.0 {
            values.set(*id, v.clone());
        }
        activate(doc, self.element, &values)
    }
}

/// Adds a configured parameter to an input (a column of its table), each row starting with
/// the parameter's value now; the parameter then reads the column's `#name` (`name`, or one made
/// from the parameter's label).
#[derive(Debug, Clone)]
pub struct ConfigureParameter {
    pub element: ElementId,
    pub input: ConfigInputId,
    pub target: ParamTarget,
    pub name: Option<String>,
}

impl Command for ConfigureParameter {
    fn label(&self) -> String {
        "Configure parameter".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let settings = doc.properties.clone();
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if el.configuration.param(&self.target).is_some() {
            return Err(CommandError::Invalid(
                "the parameter is already configured".into(),
            ));
        }
        if let ParamTarget::Property { key, .. } = self.target
            && !key.is_configurable()
        {
            return Err(CommandError::Invalid(
                "the property can't be configured".into(),
            ));
        }
        let now = match &self.target {
            ParamTarget::Free {
                kind: ColumnKind::Boolean,
            } => CellValue::Bool(false),
            ParamTarget::Free { kind } => CellValue::Expr(plain(0.0, kind.quantity())),
            t => current(t, el.features(), el.suppressed(), el.part_props())
                .ok_or_else(|| CommandError::Invalid("no such parameter".into()))?,
        };
        let taken = taken_names(el);
        let name = if self.target.is_text() {
            String::new()
        } else {
            match &self.name {
                Some(n) => {
                    if let Some(p) = el.configuration.name_problem(n, None, &taken) {
                        return Err(CommandError::Invalid(p));
                    }
                    n.clone()
                }
                None => {
                    let label = self
                        .target
                        .label(el.features(), &settings, &|_| String::new());
                    el.configuration.fresh_name(&label, &taken)
                }
            }
        };
        let input = el
            .configuration
            .input_mut(self.input)
            .ok_or_else(|| CommandError::Invalid("no such configuration input".into()))?;
        if matches!(input.kind, InputKind::Value { .. }) {
            return Err(CommandError::Invalid(format!(
                "{}: a value has no table; configure with a List or a Checkbox",
                input.name
            )));
        }
        let n = input.row_count();
        input.params.push(ConfiguredParam {
            target: self.target.clone(),
            values: vec![now; n],
            name: name.clone(),
        });
        if !name.is_empty()
            && let ElementKind::PartStudio {
                features,
                suppressed,
                ..
            } = &mut el.kind
        {
            bind(&self.target, &name, features, suppressed);
        }
        crate::commands::refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Removes a configured parameter (the feature keeps the value it has now; other expressions
/// that read the column keep theirs as numbers).
#[derive(Debug, Clone)]
pub struct UnconfigureParameter {
    pub element: ElementId,
    pub target: ParamTarget,
}

impl Command for UnconfigureParameter {
    fn label(&self) -> String {
        "Remove configured parameter".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let units = doc.units;
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        let active = el.configuration.active.clone();
        let mut gone: Vec<(ConfiguredParam, usize)> = Vec::new();
        for i in &mut el.configuration.inputs {
            let row = i.row_of(active.get(i.id));
            let (out, keep): (Vec<_>, Vec<_>) =
                i.params.drain(..).partition(|p| p.target == self.target);
            i.params = keep;
            gone.extend(out.into_iter().map(|p| (p, row)));
        }
        if gone.is_empty() {
            return Err(CommandError::Invalid(
                "the parameter is not configured".into(),
            ));
        }
        if let ElementKind::PartStudio {
            features,
            suppressed,
            ..
        } = &mut el.kind
        {
            for (p, row) in gone {
                if let Some(v) = p.values.get(row) {
                    unbind(&p.target, v, features, suppressed, &units);
                }
                if p.binds() {
                    bake_name(features, &p.name);
                }
            }
        }
        crate::commands::refresh_studio(doc, self.element);
        Ok(())
    }
}

/// Sets the configuration of assembly instances of a configured Part Studio (`None`: the
/// studio's own active configuration as it is, unconfigured).
#[derive(Debug, Clone)]
pub struct SetInstanceConfiguration {
    pub assembly: ElementId,
    pub instances: Vec<InstanceId>,
    pub values: Option<ConfigValues>,
}

impl Command for SetInstanceConfiguration {
    fn label(&self) -> String {
        "Change instance configuration".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.assembly)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let studios: Vec<(InstanceId, ElementId)> = {
            let a = doc
                .element(self.assembly)
                .and_then(|e| e.assembly_model())
                .ok_or(CommandError::ElementNotFound(self.assembly))?;
            self.instances
                .iter()
                .map(|id| {
                    let i = a
                        .instance(*id)
                        .ok_or_else(|| CommandError::Invalid("no such instance".into()))?;
                    let studio = i
                        .configuration
                        .as_ref()
                        .map(|c| c.studio)
                        .unwrap_or_else(|| i.source.element());
                    Ok((*id, studio))
                })
                .collect::<Result<_, CommandError>>()?
        };
        let mut set: Vec<(InstanceId, Option<InstanceConfiguration>, ElementId)> = Vec::new();
        for (id, studio) in studios {
            let el = doc
                .elements
                .iter()
                .find(|e| e.id == studio)
                .ok_or_else(|| {
                    CommandError::Invalid("the instance's Part Studio is not a tab here".into())
                })?;
            if el.configuration.is_empty() {
                return Err(CommandError::Invalid(format!(
                    "{} has no configuration",
                    el.name
                )));
            }
            let c = match &self.values {
                Some(v) => {
                    for (input, x) in &v.0 {
                        el.configuration
                            .input(*input)
                            .ok_or_else(|| {
                                CommandError::Invalid("no such configuration input".into())
                            })?
                            .check_value(x)
                            .map_err(CommandError::Invalid)?;
                    }
                    let values = el.configuration.complete(v);
                    Some(InstanceConfiguration { studio, values })
                }
                None => None,
            };
            set.push((id, c, studio));
        }
        let a = doc
            .element_mut(self.assembly)
            .and_then(|e| e.assembly_model_mut())
            .ok_or(CommandError::ElementNotFound(self.assembly))?;
        let had = a.instances.iter().any(|i| i.configuration.is_some());
        for (id, c, studio) in set {
            if let Some(i) = a.instance_mut(id) {
                set_source_element(
                    &mut i.source,
                    c.as_ref().map_or(studio, InstanceConfiguration::copy_id),
                );
                i.configuration = c;
            }
        }
        show_bom_column(a, had);
        Ok(())
    }
}

/// Shows the BOM's Configuration column (after Name) when an assembly gets its first
/// configured instance (`had`: it had one before); a column removed afterwards stays removed.
pub fn show_bom_column(asm: &mut crate::assembly::Assembly, had: bool) {
    use crate::assembly::bom::BomColumn;
    let col = BomColumn::Property(PropertyKey::Configuration);
    if had
        || !asm.instances.iter().any(|i| i.configuration.is_some())
        || asm.bom.columns.contains(&col)
    {
        return;
    }
    let at = asm
        .bom
        .columns
        .iter()
        .position(|c| *c == BomColumn::Property(PropertyKey::Name))
        .map_or(asm.bom.columns.len(), |i| i + 1);
    asm.bom.columns.insert(at, col);
}

/// The Part Studio tab an element is (itself), or is a configured copy of.
pub fn tab_of(doc: &Document, element: ElementId) -> ElementId {
    doc.configured
        .iter()
        .find(|c| c.element.id == element)
        .map_or(element, |c| c.studio)
}

// ---------------------------------------------------------------------------------------------
// The node view's wires: any field, dimension or suppression reading an expression

/// A feature's fields the node view can wire: (label, quantity, expression). None for a
/// sketch (its dimensions: [`dimension_slots`]) or a Variable.
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

/// Wires a feature's field to an expression (the node view's wire from an input or column:
/// `#length`).
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
            *s.expr = self.expr.clone();
        })
    }
}

/// A sketch dimension the node view can drive (a driving one, not a reference): `D1`, `D2`, …
/// in the sketch's order (as the configuration table names them), what it measures, its value
/// and the expression it holds, if any (`#input1`).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionSlot {
    pub id: cadrs_sketch::DimensionId,
    pub label: String,
    pub quantity: Quantity,
    pub value: f64,
    pub expr: Option<String>,
}

impl DimensionSlot {
    /// Whether an expression naming a variable drives it.
    pub fn is_driven(&self) -> bool {
        self.expr.as_deref().is_some_and(|e| e.contains('#'))
    }
}

/// A sketch's driving dimensions ([`DimensionSlot`]); none for another feature.
pub fn dimension_slots(f: &Feature) -> Vec<DimensionSlot> {
    let Some(sk) = f.sketch() else {
        return Vec::new();
    };
    let g = &sk.geometry;
    g.dimensions
        .iter()
        .enumerate()
        .filter(|(_, (_, d))| !d.driven)
        .map(|(k, (id, d))| DimensionSlot {
            id,
            label: format!("D{}", k + 1),
            quantity: d.kind.quantity(),
            value: d.value,
            expr: g
                .expressions
                .iter()
                .find(|(e, _)| *e == id)
                .map(|(_, x)| x.clone()),
        })
        .collect()
}

/// Drives a sketch dimension with an expression (the node view's wire from an input into the
/// sketch: `#input1`); the variables' refresh sets its value and solves the sketch.
#[derive(Debug, Clone)]
pub struct ConnectDimension {
    pub element: ElementId,
    pub feature: FeatureId,
    pub dimension: cadrs_sketch::DimensionId,
    pub expr: String,
}

impl Command for ConnectDimension {
    fn label(&self) -> String {
        format!("Drive a dimension with {}", self.expr)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        crate::commands::EditSketch {
            element: self.element,
            feature: self.feature,
            op: cadrs_sketch::SketchOp::SetDimensionExpr {
                id: self.dimension,
                expr: Some(self.expr.clone()),
            },
        }
        .apply(doc)
    }
}

/// Stops driving a sketch dimension: it keeps the value it has (a configured one is
/// unconfigured, [`UnconfigureParameter`]).
#[derive(Debug, Clone)]
pub struct DisconnectDimension {
    pub element: ElementId,
    pub feature: FeatureId,
    pub dimension: cadrs_sketch::DimensionId,
}

impl Command for DisconnectDimension {
    fn label(&self) -> String {
        "Stop driving a dimension".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let target = ParamTarget::Dimension {
            feature: self.feature,
            dimension: self.dimension,
        };
        if doc
            .element(self.element)
            .is_some_and(|e| e.configuration.param(&target).is_some())
        {
            return UnconfigureParameter {
                element: self.element,
                target,
            }
            .apply(doc);
        }
        crate::commands::EditSketch {
            element: self.element,
            feature: self.feature,
            op: cadrs_sketch::SketchOp::SetDimensionExpr {
                id: self.dimension,
                expr: None,
            },
        }
        .apply(doc)
    }
}

/// Unwires a feature's field: its expression becomes its current value (a configured field is
/// unconfigured, [`UnconfigureParameter`]).
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
        let target = ParamTarget::Field {
            feature: self.feature,
            label: self.slot.clone(),
            occurrence: 0,
        };
        if doc
            .element(self.element)
            .is_some_and(|e| e.configuration.param(&target).is_some())
        {
            return UnconfigureParameter {
                element: self.element,
                target,
            }
            .apply(doc);
        }
        set_slot(doc, self.element, self.feature, &self.slot, |s| {
            *s.expr = VariableKind::of(s.quantity).literal(*s.value);
        })
    }
}

fn set_slot(
    doc: &mut Document,
    element: ElementId,
    feature: FeatureId,
    slot: &str,
    f: impl FnOnce(crate::variables::Slot<'_>),
) -> Result<(), CommandError> {
    let el = doc
        .element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?;
    let feat = el
        .features_mut()
        .and_then(|fs| fs.iter_mut().find(|x| x.id == feature))
        .ok_or_else(|| CommandError::Invalid("no such feature".into()))?;
    with_slot(&mut feat.kind, slot, 0, f)
        .ok_or_else(|| CommandError::Invalid(format!("no field {slot}")))?;
    crate::commands::refresh_studio(doc, element);
    Ok(())
}

/// Moves nodes of the node view (and the Configuration and Parts nodes).
#[derive(Debug, Clone)]
pub struct MoveGraphNodes {
    pub element: ElementId,
    pub positions: Vec<(FeatureId, [f32; 2])>,
    pub inputs_node: Option<[f32; 2]>,
    pub parts_node: Option<[f32; 2]>,
}

impl Command for MoveGraphNodes {
    fn label(&self) -> String {
        "Move nodes".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let layout = doc
            .element_mut(self.element)
            .and_then(|e| e.node_layout_mut())
            .ok_or(CommandError::ElementNotFound(self.element))?;
        for (f, p) in &self.positions {
            layout.positions.insert(*f, *p);
        }
        if let Some(p) = self.inputs_node {
            layout.inputs_node = Some(p);
        }
        if let Some(p) = self.parts_node {
            layout.parts_node = Some(p);
        }
        Ok(())
    }
}

/// Forgets where the nodes were put (Auto arrange lays them out again).
#[derive(Debug, Clone)]
pub struct ResetNodeLayout {
    pub element: ElementId,
}

impl Command for ResetNodeLayout {
    fn label(&self) -> String {
        "Arrange nodes".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let layout = doc
            .element_mut(self.element)
            .and_then(|e| e.node_layout_mut())
            .ok_or(CommandError::ElementNotFound(self.element))?;
        layout.positions.clear();
        layout.inputs_node = None;
        layout.parts_node = None;
        Ok(())
    }
}

/// Collapses or expands a node.
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
        let layout = doc
            .element_mut(self.element)
            .and_then(|e| e.node_layout_mut())
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if self.collapsed {
            layout.collapsed.insert(self.feature);
        } else {
            layout.collapsed.remove(&self.feature);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
