//! The MCP server's configuration tools ([`cadrs_core::configuration`]): read a Part Studio's
//! configuration, define it whole, pick the one it shows, and set assembly instances'. Inputs,
//! rows and parameters are named as the Configuration panel shows them; each change is one
//! undo step of the open document.

use std::collections::BTreeMap;

use bevy::prelude::*;
use cadrs_core::configuration::{
    self as cfg, CellValue, ConfigInput, ConfigValues, Configuration, ConfiguredParam,
    EditConfiguration, InputKind, InputValue, InstanceConfiguration, ListRow, ParamTarget,
    SetActiveConfiguration, SetInstanceConfiguration,
};
use cadrs_core::document::Element;
use cadrs_core::variables::VariableType;
use cadrs_core::{ConfigInputId, ConfigRowId, ElementId};
use cadrs_mcp::tools;
use cadrs_sketch::units::{Quantity, Units};
use serde_json::{Value, json};

use crate::mcp::{assembly, describe_assembly, doc, doc_mut, part_studio};

fn units(world: &World) -> Units {
    world.resource::<crate::WorkspaceUnits>().0
}

fn studio_el(world: &World, name: Option<&str>) -> Result<(ElementId, Element), String> {
    let d = doc(world)?;
    let id = part_studio(d, name)?;
    let el = d.doc.element(id).ok_or("the Part Studio is gone")?.clone();
    Ok((id, el))
}

/// A JSON value as an expression of quantity `q` (a number takes the workspace unit).
fn expr_of(v: &Value, q: Option<Quantity>, units: &Units) -> Result<String, String> {
    let t = match q {
        Some(Quantity::Length) => VariableType::Length,
        Some(Quantity::Angle) => VariableType::Angle,
        _ => VariableType::Number,
    };
    match v {
        Value::String(s) if !s.trim().is_empty() => Ok(t.with_unit(s, units)),
        Value::Number(n) => Ok(t.with_unit(&n.to_string(), units)),
        _ => Err(format!("{v} is not an expression or a number")),
    }
}

fn bool_of(v: &Value) -> Result<bool, String> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => Ok(n.as_f64().is_some_and(|x| x != 0.0)),
        Value::String(s) if s.eq_ignore_ascii_case("true") => Ok(true),
        Value::String(s) if s.eq_ignore_ascii_case("false") => Ok(false),
        _ => Err(format!("{v} is not true or false")),
    }
}

/// An input's value from JSON.
fn value_of(input: &ConfigInput, v: &Value) -> Result<InputValue, String> {
    Ok(match &input.kind {
        InputKind::List { rows, .. } => {
            let row = match v {
                Value::String(s) => rows.iter().find(|r| r.name == *s),
                Value::Number(n) => n.as_u64().and_then(|i| rows.get(i as usize)),
                _ => None,
            }
            .ok_or_else(|| {
                let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
                format!(
                    "{}: no configuration {v}; its configurations are {}",
                    input.name,
                    names.join(", ")
                )
            })?;
            InputValue::Row(row.id)
        }
        InputKind::Checkbox { .. } => {
            InputValue::Checked(bool_of(v).map_err(|e| format!("{}: {e}", input.name))?)
        }
        // A number (mm, degrees) or a value with its unit ("12 mm", "0.5 in").
        InputKind::Value { kind, .. } => InputValue::Number(match v {
            Value::Number(n) => n
                .as_f64()
                .ok_or_else(|| format!("{}: {v} is not a number", input.name))?,
            Value::String(t) => cadrs_sketch::units::eval(t, kind.quantity())
                .map_err(|e| format!("{}: {t}: {e}", input.name))?,
            _ => return Err(format!("{}: {v} is not a value", input.name)),
        }),
    })
}

/// Values from JSON (input name → value) over `base`.
fn values_of(
    c: &Configuration,
    map: &BTreeMap<String, Value>,
    base: &ConfigValues,
) -> Result<ConfigValues, String> {
    let mut out = base.clone();
    for (name, v) in map {
        let input = c.input_named(name).ok_or_else(|| {
            let names: Vec<&str> = c.inputs.iter().map(|i| i.name.as_str()).collect();
            format!(
                "no configuration input {name:?}; the inputs are {}",
                names.join(", ")
            )
        })?;
        let value = value_of(input, v)?;
        input.check_value(&value)?;
        out.set(input.id, value);
    }
    Ok(out)
}

/// The values as JSON: input name → row name or boolean.
fn values_json(c: &Configuration, values: &ConfigValues) -> Value {
    let mut out = serde_json::Map::new();
    for (input, r) in c.inputs.iter().zip(c.resolve(values)) {
        let v = match (r, &input.kind) {
            (cfg::Resolved::Row(i), InputKind::List { rows, .. }) => json!(rows[i].name),
            (cfg::Resolved::Checked(b), _) => json!(b),
            _ => continue,
        };
        out.insert(input.name.clone(), v);
    }
    Value::Object(out)
}

/// A parameter's name as the tools take it ("Extrude 1 · Depth", "Part 1 · Part number": a part
/// named as the Parts list shows it).
fn label(world: &World, el: &Element, t: &ParamTarget) -> String {
    crate::configuration_ui::target_label(world, el, t)
}

/// The studio's parts the tools can configure the properties of: the ones the Parts list shows
/// when it is the open tab, and those with settings or configured properties.
fn parts_of(world: &World, el: &Element) -> Vec<cadrs_core::PartId> {
    let mut out: Vec<cadrs_core::PartId> = Vec::new();
    let active = doc(world)
        .ok()
        .and_then(|d| d.active_element())
        .is_some_and(|a| a.id == el.id);
    if active && let Some(cache) = world.get_resource::<crate::parts::PartCache>() {
        out.extend(cache.parts.iter().map(|p| p.id));
    }
    out.extend(el.part_props().iter().map(|p| p.part));
    for i in &el.configuration.inputs {
        for p in &i.params {
            if let ParamTarget::Property { part, .. } = p.target {
                out.push(part);
            }
        }
    }
    let mut seen = Vec::new();
    out.retain(|p| {
        let new = !seen.contains(p);
        seen.push(*p);
        new
    });
    out
}

/// Every configurable parameter of the studio's features and parts, by the names the tools
/// take.
fn configurable(world: &World, el: &Element) -> Vec<(ParamTarget, String)> {
    let settings = doc(world)
        .map(|d| d.doc.properties.clone())
        .unwrap_or_default();
    el.features()
        .iter()
        .flat_map(cfg::configurable)
        .chain(
            parts_of(world, el)
                .into_iter()
                .flat_map(|p| cfg::configurable_properties(p, &settings)),
        )
        .map(|t| {
            let label = label(world, el, &t);
            (t, label)
        })
        .collect()
}

fn target_named(world: &World, el: &Element, name: &str) -> Result<ParamTarget, String> {
    let norm = |s: &str| {
        s.replace(" · ", "/")
            .replace('·', "/")
            .replace(" / ", "/")
            .to_lowercase()
    };
    let want = norm(name.trim());
    // A plain output (an Input Configuration's row of its own): its kind.
    let free = match want.as_str() {
        "length" => Some(cfg::ColumnKind::Length),
        "angle" => Some(cfg::ColumnKind::Angle),
        "number" => Some(cfg::ColumnKind::Number),
        _ => None,
    };
    if let Some(kind) = free {
        return Ok(ParamTarget::Free { kind });
    }
    configurable(world, el)
        .into_iter()
        .find(|(_, l)| norm(l) == want)
        .map(|(t, _)| t)
        .ok_or_else(|| {
            format!(
                "no configurable parameter {name:?}: get_configuration lists them under `configurable` (\"Extrude 1 · Depth\")"
            )
        })
}

/// The configuration as get_configuration answers it.
pub fn describe(world: &World, element: ElementId) -> Result<Value, String> {
    let d = doc(world)?;
    let el = d.doc.element(element).ok_or("the Part Studio is gone")?;
    let c = &el.configuration;
    let inputs: Vec<Value> = c
        .inputs
        .iter()
        .map(|i| {
            let mut v = json!({ "name": i.name, "kind": i.kind.label() });
            match &i.kind {
                InputKind::List { rows, default } => {
                    v["rows"] = json!(rows.iter().map(|r| &r.name).collect::<Vec<_>>());
                    v["default"] = json!(rows.get(*default).map(|r| &r.name));
                }
                InputKind::Checkbox { default } => v["default"] = json!(default),
                InputKind::Value { default, .. } => v["default"] = json!(default),
            }
            // `#name` in expressions: a Checkbox's and a Value's own, each column's.
            if i.defines_variable() {
                v["variable"] = json!(format!("#{}", i.name));
            }
            {
                v["parameters"] = json!(
                    i.params
                        .iter()
                        .map(|p| json!({
                            "parameter": label(world, el, &p.target),
                            "variable": p.binds().then(|| format!("#{}", p.name)),
                            "values": p.values.iter().map(|x| match x {
                                CellValue::Bool(b) => json!(b),
                                CellValue::Expr(e) => json!(e),
                            }).collect::<Vec<_>>(),
                        }))
                        .collect::<Vec<_>>()
                );
            }
            v
        })
        .collect();
    let configurable: Vec<String> = configurable(world, el)
        .into_iter()
        .map(|(_, l)| l)
        .collect();
    Ok(json!({
        "part_studio": el.name,
        "inputs": inputs,
        "active": values_json(c, &c.active),
        "shows": c.describe(&c.active),
        "problem": c.problem(),
        "configurable": configurable,
    }))
}

pub fn get_configuration(world: &mut World, p: &tools::GetConfiguration) -> Result<Value, String> {
    let (element, _) = studio_el(world, p.part_studio.as_deref())?;
    describe(world, element)
}

/// The configuration a spec describes, keeping the ids of the inputs and rows it names again.
fn build(
    world: &World,
    el: &Element,
    specs: &[tools::ConfigInputSpec],
    units: &Units,
) -> Result<Configuration, String> {
    let old = &el.configuration;
    let mut c = Configuration {
        inputs: Vec::new(),
        active: old.active.clone(),
        // (The parts' properties per configuration are kept.)
        part_properties: old.part_properties.clone(),
    };
    for s in specs {
        let before = old.input_named(&s.name);
        let id = before.map_or_else(ConfigInputId::new, |i| i.id);
        let kind = match s.kind.to_ascii_lowercase().as_str() {
            "list" => {
                if s.rows.is_empty() {
                    return Err(format!("{}: a List needs `rows`", s.name));
                }
                let old_rows: &[ListRow] = match before.map(|i| &i.kind) {
                    Some(InputKind::List { rows, .. }) => rows,
                    _ => &[],
                };
                let rows: Vec<ListRow> = s
                    .rows
                    .iter()
                    .map(|n| ListRow {
                        id: old_rows
                            .iter()
                            .find(|r| r.name == *n)
                            .map_or_else(ConfigRowId::new, |r| r.id),
                        name: n.clone(),
                    })
                    .collect();
                let default = match &s.default {
                    None => 0,
                    Some(v) => s
                        .rows
                        .iter()
                        .position(|r| v.as_str() == Some(r.as_str()))
                        .ok_or_else(|| format!("{}: the default {v} is not a row", s.name))?,
                };
                InputKind::List { rows, default }
            }
            "checkbox" => InputKind::Checkbox {
                default: s
                    .default
                    .as_ref()
                    .map(bool_of)
                    .transpose()?
                    .unwrap_or(false),
            },
            k @ ("length" | "angle" | "number") => {
                let kind = match k {
                    "length" => cadrs_core::configuration::ValueKind::Length,
                    "angle" => cadrs_core::configuration::ValueKind::Angle,
                    _ => cadrs_core::configuration::ValueKind::Number,
                };
                let default = match &s.default {
                    None => 0.0,
                    Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
                    Some(Value::String(t)) => units
                        .eval(t, kind.quantity())
                        .map_err(|e| format!("{}: {t}: {e}", s.name))?,
                    Some(v) => return Err(format!("{}: the default {v} is not a value", s.name)),
                };
                InputKind::Value { kind, default }
            }
            k => {
                return Err(format!(
                    "{}: no input kind {k:?} (List, Checkbox, Length, Angle, Number)",
                    s.name
                ));
            }
        };
        let n = match &kind {
            InputKind::List { rows, .. } => rows.len(),
            InputKind::Checkbox { .. } => 2,
            InputKind::Value { .. } => 0,
        };
        let mut params = Vec::new();
        for p in &s.parameters {
            let target = target_named(world, el, &p.parameter)?;
            if p.values.len() != n {
                return Err(format!(
                    "{}: {} has {} values for {n} rows",
                    s.name,
                    p.parameter,
                    p.values.len()
                ));
            }
            let q = match &target {
                ParamTarget::Free { kind } => Some(kind.quantity()),
                t => t.quantity(el.features()),
            };
            let values = p
                .values
                .iter()
                .map(|v| match target {
                    ParamTarget::Suppressed { .. } => bool_of(v).map(CellValue::Bool),
                    // A property's text as given (a number or boolean as it reads).
                    ParamTarget::Property { .. } => Ok(CellValue::Expr(match v {
                        Value::String(s) => s.clone(),
                        Value::Null => String::new(),
                        v => v.to_string(),
                    })),
                    _ => expr_of(v, q, units).map(CellValue::Expr),
                })
                .collect::<Result<Vec<_>, String>>()
                .map_err(|e| format!("{}: {}: {e}", s.name, p.parameter))?;
            params.push(ConfiguredParam {
                target,
                values,
                name: p.name.clone().unwrap_or_default(),
            });
        }
        c.inputs.push(ConfigInput {
            id,
            name: s.name.clone(),
            kind,
            params,
        });
    }
    Ok(c)
}

pub fn set_configuration(world: &mut World, p: &tools::SetConfiguration) -> Result<Value, String> {
    let units = units(world);
    let (element, el) = studio_el(world, p.part_studio.as_deref())?;
    let mut c = build(world, &el, &p.inputs, &units)?;
    if let Some(map) = &p.active {
        c.active = values_of(&c, map, &c.active)?;
    }
    doc_mut(world)?
        .execute(&EditConfiguration {
            element,
            configuration: c,
            label: "Edit configuration".into(),
        })
        .map_err(|e| e.to_string())?;
    describe(world, element)
}

pub fn set_active_configuration(
    world: &mut World,
    p: &tools::SetActiveConfiguration,
) -> Result<Value, String> {
    let (element, el) = studio_el(world, p.part_studio.as_deref())?;
    let c = &el.configuration;
    if c.is_empty() {
        return Err(format!("{} has no configuration", el.name));
    }
    let values = values_of(c, &p.values, &ConfigValues::default())?;
    doc_mut(world)?
        .execute(&SetActiveConfiguration { element, values })
        .map_err(|e| e.to_string())?;
    let mut out = describe(world, element)?;
    out["document"] = crate::mcp::describe(world)?;
    Ok(out)
}

/// The instances a call names (as get_assembly lists them, or by id).
fn instances_named(
    world: &World,
    assembly: ElementId,
    names: &[String],
) -> Result<Vec<cadrs_core::assembly::InstanceId>, String> {
    let listed = describe_assembly(doc(world)?, assembly)?;
    let all = listed["instances"].as_array().cloned().unwrap_or_default();
    names
        .iter()
        .map(|n| {
            let hit = all
                .iter()
                .find(|i| i["name"].as_str() == Some(n) || i["id"].as_str() == Some(n))
                .ok_or_else(|| format!("no instance {n:?}: get_assembly lists them"))?;
            let id = hit["id"].as_str().unwrap_or_default();
            uuid::Uuid::parse_str(id)
                .map(cadrs_core::assembly::InstanceId)
                .map_err(|e| e.to_string())
        })
        .collect()
}

pub fn set_instance_configuration(
    world: &mut World,
    p: &tools::SetInstanceConfiguration,
) -> Result<Value, String> {
    let element = assembly(doc(world)?, p.assembly.as_deref())?;
    let instances = instances_named(world, element, &p.instances)?;
    let values = match &p.configuration {
        None => None,
        Some(map) => {
            let d = doc(world)?;
            let a = d
                .doc
                .element(element)
                .and_then(|e| e.assembly_model())
                .ok_or("not an Assembly")?;
            let first = a.instance(instances[0]).ok_or("the instance is gone")?;
            let studio = first
                .configuration
                .as_ref()
                .map_or(first.source.element(), |c| c.studio);
            let el = d
                .doc
                .elements
                .iter()
                .find(|e| e.id == studio)
                .ok_or("the instance's Part Studio is not a tab here")?;
            let base = first
                .configuration
                .as_ref()
                .map_or_else(|| el.configuration.active.clone(), |c| c.values.clone());
            Some(values_of(&el.configuration, map, &base)?)
        }
    };
    doc_mut(world)?
        .execute(&SetInstanceConfiguration {
            assembly: element,
            instances,
            values,
        })
        .map_err(|e| e.to_string())?;
    describe_assembly(doc(world)?, element)
}

/// An instance's configuration from insert_instance's `configuration`.
pub fn instance_configuration(
    world: &World,
    studio: ElementId,
    map: &BTreeMap<String, Value>,
) -> Result<InstanceConfiguration, String> {
    let d = doc(world)?;
    let el = d
        .doc
        .elements
        .iter()
        .find(|e| e.id == studio)
        .ok_or("the Part Studio is gone")?;
    if el.configuration.is_empty() {
        return Err(format!("{} has no configuration", el.name));
    }
    let values = values_of(&el.configuration, map, &el.configuration.active)?;
    Ok(InstanceConfiguration { studio, values })
}

/// What get_assembly says of an instance's configuration ("M5, Chamfered").
pub fn instance_label(d: &crate::ActiveDocument, c: &InstanceConfiguration) -> Value {
    let Some(el) = d.doc.elements.iter().find(|e| e.id == c.studio) else {
        return Value::Null;
    };
    json!({
        "values": values_json(&el.configuration, &c.values),
        "shows": el.configuration.describe(&c.values),
    })
}
