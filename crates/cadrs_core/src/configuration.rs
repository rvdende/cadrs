//! **Configurations** (Onshape's Configuration panel): one Part Studio that builds as several
//! variants, picked in the Part Studio and per assembly instance.
//!
//! - **Inputs** ([`ConfigInput`]), in order: a **List** (a table of named rows, each a
//!   configuration: M3, M4, M5, …) and a **Checkbox** (`#name` is 1 or 0, so it can also
//!   suppress features by variable, IR5.5). Values typed in are Variable features or a Variable
//!   Studio's (a List's column can set a Variable feature's value).
//! - **Configured parameters** ([`ConfiguredParam`]) are the **configuration table**'s columns:
//!   a feature field (an extrude's depth, a fillet's radius), a sketch dimension, a Variable
//!   feature's value, or whether a feature is suppressed, with a value for each of its input's
//!   rows (a Checkbox has two: unchecked and checked). Fields are found through
//!   [`crate::variables::slots`], so every numeric field of every feature can be configured.
//! - **The stored features are the active configuration's** ([`Configuration::active`]):
//!   picking another one writes its row's values into the features ([`apply`]) and refreshes
//!   the studio. After every command, the configured fields are read back into the active
//!   row ([`capture`], from [`after_command`]), so editing a configured field in its dialog
//!   edits that configuration, as in Onshape. The Checkboxes reach expressions as Variable
//!   features ahead of the list ([`input_features`], [`crate::document::Element::studio_features`]).
//! - **Assembly instances** pick their own configuration ([`InstanceConfiguration`]). An
//!   instance with one names a **configured copy** of the studio ([`ConfiguredStudio`] in
//!   [`Document::configured`], found by [`Document::element`] like standard content): the
//!   studio's features with the instance's values applied, under an id derived from the studio
//!   and the values ([`copy_id`]), so two instances in the same configuration share it. The
//!   copies are regenerated when their studio changes and dropped when nothing uses them
//!   ([`refresh_copies`], after every command, undo and redo). The features keep their ids,
//!   so the parts, faces and edges do too, and mates survive a change of configuration.

use cadrs_sketch::units::{Quantity, Units, VarValue};

use serde::{Deserialize, Serialize};

use crate::assembly::InstanceId;
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element, ElementKind, Feature, FeatureKind};
use crate::ids::{ConfigInputId, ConfigRowId, ElementId, FeatureId};
use crate::variables::{VariableFeature, VariableKind};

/// A Part Studio's configuration: its inputs and the values it shows now.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Configuration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<ConfigInput>,
    /// The configuration the Part Studio shows (its features hold these values).
    #[serde(default, skip_serializing_if = "ConfigValues::is_empty")]
    pub active: ConfigValues,
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

    pub fn input_named(&self, name: &str) -> Option<&ConfigInput> {
        self.inputs.iter().find(|i| i.name == name)
    }

    /// Every input's value in `values`, or its default where `values` has none (or one that no
    /// longer fits: a deleted row).
    pub fn resolve(&self, values: &ConfigValues) -> Vec<Resolved> {
        self.inputs
            .iter()
            .map(|input| input.resolve_value(values.get(input.id)))
            .collect()
    }

    /// The default values: each List's default row, each Checkbox's state.
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
                _ => continue,
            };
            out.set(input.id, v);
        }
        out
    }

    /// How the values read, for lists and labels: "M5, Chamfered, No thread".
    pub fn describe(&self, values: &ConfigValues) -> String {
        let mut parts = Vec::new();
        for (input, r) in self.inputs.iter().zip(self.resolve(values)) {
            parts.push(match (r, &input.kind) {
                (Resolved::Row(i), InputKind::List { rows, .. }) => rows[i].name.clone(),
                (Resolved::Checked(true), _) => input.name.clone(),
                (Resolved::Checked(false), _) => format!("No {}", input.name),
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
                let boolean = matches!(p.target, ParamTarget::Suppressed { .. });
                if p.values.iter().any(|v| v.as_bool().is_some() != boolean) {
                    return Some(format!(
                        "{}: a configured parameter has a value of the wrong kind",
                        input.name
                    ));
                }
            }
        }
        let targets: Vec<&ParamTarget> = self
            .inputs
            .iter()
            .flat_map(|i| i.params.iter().map(|p| &p.target))
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
            .any(|i| i.params.iter().any(|p| p.target.feature() == feature))
    }
}

/// One input of a configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigInput {
    pub id: ConfigInputId,
    /// Its name: `#name` in expressions (a Checkbox's), and the label of its
    /// field in the Configuration panel.
    pub name: String,
    pub kind: InputKind,
    /// Its configured parameters: the configuration table's columns.
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

    /// How many values each of its configured parameters has: a List's rows, a Checkbox's
    /// two (unchecked, checked).
    pub fn row_count(&self) -> usize {
        match &self.kind {
            InputKind::List { rows, .. } => rows.len(),
            InputKind::Checkbox { .. } => 2,
        }
    }

    /// The labels of its rows, as the table heads them.
    pub fn row_names(&self) -> Vec<String> {
        match &self.kind {
            InputKind::List { rows, .. } => rows.iter().map(|r| r.name.clone()).collect(),
            InputKind::Checkbox { .. } => vec!["Unchecked".into(), "Checked".into()],
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
            (_, _) => Resolved::Checked(false),
        }
    }

    /// The row its value picks: a List's, a Checkbox's 0 (unchecked) or 1 (checked).
    pub fn row_of(&self, value: Option<&InputValue>) -> usize {
        match self.resolve_value(value) {
            Resolved::Row(i) => i,
            Resolved::Checked(c) => usize::from(c),
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
}

impl InputKind {
    pub fn label(&self) -> &'static str {
        match self {
            InputKind::List { .. } => "List",
            InputKind::Checkbox { .. } => "Checkbox",
        }
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
}

/// An input's value as the features see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resolved {
    /// The index of a List's row.
    Row(usize),
    Checked(bool),
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

/// A configured parameter: a column of the configuration table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredParam {
    pub target: ParamTarget,
    /// Its value for each of its input's rows.
    pub values: Vec<CellValue>,
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
}

impl ParamTarget {
    pub fn feature(&self) -> FeatureId {
        match self {
            ParamTarget::Field { feature, .. }
            | ParamTarget::Dimension { feature, .. }
            | ParamTarget::Variable { feature }
            | ParamTarget::Suppressed { feature } => *feature,
        }
    }

    /// The column's heading: "Extrude 1 · Depth", "Sketch 1 · D3", "Fillet 1 · Suppressed".
    pub fn label(&self, features: &[Feature]) -> String {
        let f = features.iter().find(|f| f.id == self.feature());
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
        }
    }

    /// What its values measure (none for suppression).
    pub fn quantity(&self, features: &[Feature]) -> Option<Quantity> {
        let f = features.iter().find(|f| f.id == self.feature())?;
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
            ParamTarget::Suppressed { .. } => None,
        }
    }
}

/// A cell of the configuration table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CellValue {
    /// Whether the feature is suppressed.
    Bool(bool),
    /// A field's, dimension's or variable's expression ("12 mm", "#Length - 2 mm").
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

/// The value a parameter has in the features now (for a new column, and [`capture`]). `None`
/// when the target is gone (its feature deleted, a field the feature no longer has).
pub fn current(
    target: &ParamTarget,
    features: &[Feature],
    suppressed: &[FeatureId],
) -> Option<CellValue> {
    let f = features.iter().find(|f| f.id == target.feature())?;
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
    })
}

/// A value as a cell's text, as it would be typed: "16 mm", "4.618802153517006 mm", "30 deg",
/// "3" (whatever the workspace units, so it reads back exactly).
fn plain(value: f64, q: Quantity) -> String {
    match q {
        Quantity::Length => format!("{value} mm"),
        Quantity::Angle => format!("{value} deg"),
        Quantity::Count => format!("{value}"),
    }
}

/// Writes `value` into its target. A field's expression is evaluated here when it names no
/// variable (one that does, [`crate::variables::refresh`] evaluates).
fn write(
    target: &ParamTarget,
    value: &CellValue,
    features: &mut [Feature],
    suppressed: &mut Vec<FeatureId>,
    units: &Units,
) {
    let Some(f) = features.iter_mut().find(|f| f.id == target.feature()) else {
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
                *s.expr = e.clone();
                if !e.contains('#')
                    && let Ok(v) = units.eval(e, s.quantity)
                {
                    *s.value = v;
                }
            });
        }
        (ParamTarget::Dimension { dimension, .. }, CellValue::Expr(e)) => {
            if let Some(sk) = f.sketch_mut() {
                let g = &mut sk.geometry;
                // A plain value is the dimension's value (the sketch solved to it), not an
                // expression; one naming a variable is kept for the refresh to evaluate.
                let plain = (!e.contains('#'))
                    .then(|| {
                        let q = g.dimensions.get(*dimension)?.kind.quantity();
                        units.eval(e, q).ok()
                    })
                    .flatten();
                g.expressions.retain(|(id, _)| id != dimension);
                match plain {
                    Some(value) => {
                        let before = g.clone();
                        let op = cadrs_sketch::SketchOp::SetDimensionValue {
                            id: *dimension,
                            value,
                        };
                        if op.apply(g).is_err() {
                            *g = before;
                        }
                    }
                    None => g.expressions.push((*dimension, e.clone())),
                }
            }
        }
        (ParamTarget::Variable { .. }, CellValue::Expr(e)) => {
            if let FeatureKind::Variable(v) = &mut f.kind {
                v.expr = e.clone();
            }
        }
        (ParamTarget::Suppressed { feature }, CellValue::Bool(b)) => {
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

/// The Variable features the inputs give the list (ahead of it, after the Variable Studios'):
/// each Checkbox's `#name` (1 or 0). Lists give none; their rows set parameters.
pub fn input_features(cfg: &Configuration, values: &ConfigValues) -> Vec<Feature> {
    cfg.inputs
        .iter()
        .zip(cfg.resolve(values))
        .filter_map(|(input, r)| {
            let Resolved::Checked(c) = r else {
                return None;
            };
            let (value, kind) = (if c { 1.0 } else { 0.0 }, VariableKind::Number);
            let var = VariableFeature {
                name: input.name.clone(),
                var_type: kind.var_type(),
                expr: kind.literal(value),
                value,
                kind,
                description: String::new(),
            };
            Some(Feature::new(
                FeatureId(input.id.0),
                format!("#{}", input.name),
                FeatureKind::Variable(var),
            ))
        })
        .collect()
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

/// Writes the configured parameters' values for `values` into the features and the
/// suppressed list. The caller refreshes the studio afterwards (variables, sketches).
pub fn apply(
    cfg: &Configuration,
    values: &ConfigValues,
    features: &mut [Feature],
    suppressed: &mut Vec<FeatureId>,
    units: &Units,
) {
    for input in &cfg.inputs {
        let row = input.row_of(values.get(input.id));
        for p in &input.params {
            if let Some(v) = p.values.get(row) {
                write(&p.target, v, features, suppressed, units);
            }
        }
    }
}

/// Reads the configured parameters back from the features into the active row of each input
/// (the configuration the studio shows), and drops the columns whose feature is gone. A cell
/// keeps its text while the field has the value the text gives ("4 mm / cos(30 deg)" stays,
/// though the sketch keeps only the number): only an edit of the field replaces it. Returns
/// whether anything changed.
pub fn capture(
    cfg: &mut Configuration,
    features: &[Feature],
    suppressed: &[FeatureId],
    units: &Units,
) -> bool {
    let active = cfg.active.clone();
    let mut changed = false;
    for input in &mut cfg.inputs {
        let row = input.row_of(active.get(input.id));
        let before = input.params.len();
        input
            .params
            .retain(|p| features.iter().any(|f| f.id == p.target.feature()));
        changed |= input.params.len() != before;
        for p in &mut input.params {
            let q = p.target.quantity(features);
            if let Some(now) = current(&p.target, features, suppressed)
                && let Some(cell) = p.values.get_mut(row)
                && *cell != now
                && !same_value(cell, &now, q, units)
            {
                *cell = now;
                changed = true;
            }
        }
    }
    changed
}

/// Whether two cells give the same value: plain expressions (no `#`) that evaluate alike.
fn same_value(a: &CellValue, b: &CellValue, q: Option<Quantity>, units: &Units) -> bool {
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

/// Makes a Part Studio show `values`: writes them into its features and refreshes it.
pub fn activate(
    doc: &mut Document,
    element: ElementId,
    values: &ConfigValues,
) -> Result<(), CommandError> {
    let units = doc.units;
    let el = doc
        .element_mut(element)
        .ok_or(CommandError::ElementNotFound(element))?;
    let values = el.configuration.complete(values);
    el.configuration.active = values.clone();
    let cfg = el.configuration.clone();
    let ElementKind::PartStudio {
        features,
        suppressed,
        ..
    } = &mut el.kind
    else {
        return Err(CommandError::Invalid(
            "configurations need a Part Studio".into(),
        ));
    };
    apply(&cfg, &values, features, suppressed, &units);
    crate::commands::refresh_studio(doc, element);
    Ok(())
}

/// After a command ([`crate::command::History::execute`]): each configured Part Studio's
/// edited fields go into its active configuration.
pub fn after_command(doc: &mut Document) {
    let units = doc.units;
    for el in &mut doc.elements {
        if el.configuration.is_empty() {
            continue;
        }
        let ElementKind::PartStudio {
            features,
            suppressed,
            ..
        } = &el.kind
        else {
            continue;
        };
        capture(&mut el.configuration, features, suppressed, &units);
    }
}

// ---- Assembly instances ------------------------------------------------------------------

/// An assembly instance's configuration: the Part Studio it comes from and its values. The
/// instance's source names the configured copy ([`copy_id`]).
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

/// A Part Studio built in a configuration other than its own, for assembly instances (not a
/// tab; [`Document::element`] finds it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredStudio {
    pub studio: ElementId,
    pub values: ConfigValues,
    /// The studio with the values applied, under [`copy_id`].
    pub element: Element,
    /// A hash of what it was generated from (the studio and the values): regenerated when the
    /// studio changes.
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

fn stamp(source: &str, values: &ConfigValues, units: &Units) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(source.as_bytes());
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
    let base = el.variable_base();
    let studio = el.studio_features();
    if let ElementKind::PartStudio {
        features,
        suppressed,
        rollback,
        ..
    } = &mut el.kind
    {
        *rollback = None;
        apply(&cfg, &cfg.active, features, suppressed, units);
        crate::variables::refresh(&base, features, suppressed, units);
        crate::parts::regenerate_now(&studio, features);
    }
    el
}

/// Every assembly instance's configured copy made and up to date, each instance's source
/// naming its copy, and the copies nothing uses dropped (after every command, undo and redo,
/// and when a document opens).
pub fn refresh_copies(doc: &mut Document) {
    if doc.configured.is_empty()
        && !doc.elements.iter().any(|e| {
            e.assembly_model()
                .is_some_and(|a| a.instances.iter().any(|i| i.configuration.is_some()))
        })
    {
        return;
    }
    let units = doc.units;
    let mut wanted: Vec<(ElementId, ConfigValues)> = Vec::new();
    for el in &mut doc.elements {
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
    // Each studio's text once, however many configurations of it are used.
    let mut texts: Vec<(ElementId, String)> = Vec::new();
    for (studio, values) in wanted {
        let Some(source) = doc.elements.iter().find(|e| e.id == studio) else {
            continue;
        };
        if !matches!(source.kind, ElementKind::PartStudio { .. }) {
            continue;
        }
        if !texts.iter().any(|(s, _)| *s == studio) {
            texts.push((studio, ron::to_string(source).unwrap_or_default()));
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

fn set_source_element(source: &mut crate::assembly::InstanceSource, id: ElementId) {
    use crate::assembly::InstanceSource as S;
    match source {
        S::Part { element, .. } | S::Studio { element, .. } => *element = id,
        S::Assembly { .. } => {}
    }
}

// ---- Commands ------------------------------------------------------------------------------

/// Replaces a Part Studio's configuration (its inputs, rows and table: the Configuration
/// panel's edits), then shows its active configuration (kept where it still exists).
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
        if let Some(p) = self.configuration.problem() {
            return Err(CommandError::Invalid(p));
        }
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if !matches!(el.kind, ElementKind::PartStudio { .. }) {
            return Err(CommandError::Invalid(
                "configurations need a Part Studio".into(),
            ));
        }
        for input in &self.configuration.inputs {
            for p in &input.params {
                if !el.features().iter().any(|f| f.id == p.target.feature()) {
                    return Err(CommandError::Invalid(format!(
                        "{}: a configured feature is not in the Part Studio",
                        input.name
                    )));
                }
            }
        }
        let active = self.configuration.active.clone();
        el.configuration = self.configuration.clone();
        activate(doc, self.element, &active)
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
/// the parameter's value now.
#[derive(Debug, Clone)]
pub struct ConfigureParameter {
    pub element: ElementId,
    pub input: ConfigInputId,
    pub target: ParamTarget,
}

impl Command for ConfigureParameter {
    fn label(&self) -> String {
        "Configure parameter".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        if el.configuration.param(&self.target).is_some() {
            return Err(CommandError::Invalid(
                "the parameter is already configured".into(),
            ));
        }
        let now = current(&self.target, el.features(), el.suppressed())
            .ok_or_else(|| CommandError::Invalid("no such parameter".into()))?;
        let input = el
            .configuration
            .input_mut(self.input)
            .ok_or_else(|| CommandError::Invalid("no such configuration input".into()))?;
        let n = input.row_count();
        input.params.push(ConfiguredParam {
            target: self.target.clone(),
            values: vec![now; n],
        });
        Ok(())
    }
}

/// Removes a configured parameter (the feature keeps the value it has now).
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
        let el = doc
            .element_mut(self.element)
            .ok_or(CommandError::ElementNotFound(self.element))?;
        let mut found = false;
        for i in &mut el.configuration.inputs {
            let n = i.params.len();
            i.params.retain(|p| p.target != self.target);
            found |= i.params.len() != n;
        }
        if !found {
            return Err(CommandError::Invalid(
                "the parameter is not configured".into(),
            ));
        }
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
        for (id, c, studio) in set {
            if let Some(i) = a.instance_mut(id) {
                set_source_element(
                    &mut i.source,
                    c.as_ref().map_or(studio, InstanceConfiguration::copy_id),
                );
                i.configuration = c;
            }
        }
        Ok(())
    }
}

/// The Part Studio tab an element is (itself), or is a configured copy of.
pub fn tab_of(doc: &Document, element: ElementId) -> ElementId {
    doc.configured
        .iter()
        .find(|c| c.element.id == element)
        .map_or(element, |c| c.studio)
}
