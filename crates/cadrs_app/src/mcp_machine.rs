//! The MCP server's machine tools (issue #278): read and set an assembly's machine definition
//! (`get_machine`, `set_machine`), jog it (`jog_machine`), and read a CAM Studio and choose
//! its job's machine (`get_cam`, `set_cam_machine`). What the Machine Builder and the CAM
//! Machine dialog do by hand goes through the same draft and the same commands, so each
//! change is one undo step of the open document.

use bevy::prelude::*;
use cadrs_core::ElementId;
use cadrs_core::assembly::commands::MoveInstances;
use cadrs_core::assembly::connector::{ConnectorFrame, MateConnector};
use cadrs_core::assembly::machine::{self, Resolved, Severity};
use cadrs_core::assembly::{Assembly, InstanceId};
use cadrs_core::cam::cadrs_cam::Dialect;
use cadrs_core::cam::cadrs_cam::machine::catalog;
use cadrs_core::cam::{CamStudio, Edit, EditCam, MachineSource, Mating, OpKind};
use cadrs_mcp::tools;
use serde_json::{Value, json};

use crate::ActiveDocument;
use crate::assembly::machine_builder::{Draft, MachineBuilder, Mount};

fn doc(world: &World) -> Result<&ActiveDocument, String> {
    world
        .get_resource::<ActiveDocument>()
        .ok_or_else(|| "no document is open: call open_document first".into())
}

fn round(x: f64) -> f64 {
    let r = (x * 1000.0).round() / 1000.0;
    if r == 0.0 { 0.0 } else { r }
}

/// The Assembly tab a call names, or the active tab.
fn assembly(d: &ActiveDocument, name: Option<&str>) -> Result<ElementId, String> {
    let e = match name {
        Some(n) => d
            .doc
            .elements
            .iter()
            .find(|e| e.name == n)
            .ok_or_else(|| format!("no tab named {n:?}"))?,
        None => d.active_element().ok_or("no tab is active")?,
    };
    if e.assembly_model().is_none() {
        return Err(format!("{:?} is not an Assembly", e.name));
    }
    Ok(e.id)
}

fn model(d: &ActiveDocument, element: ElementId) -> Result<&Assembly, String> {
    d.doc
        .element(element)
        .and_then(|e| e.assembly_model())
        .ok_or_else(|| "the Assembly is gone".into())
}

/// The names of the assembly's occurrences ("Swivel head <1>"), by their ids in the solver's
/// model, with the top-level instance each belongs to.
fn occurrence_names(d: &ActiveDocument, asm: &Assembly) -> Vec<(InstanceId, InstanceId, String)> {
    let mut builds = std::collections::HashMap::new();
    cadrs_core::assembly::structure::occurrences(&d.doc, asm)
        .into_iter()
        .map(|o| {
            let build = builds.entry(o.element).or_insert_with(|| {
                d.doc
                    .element(o.element)
                    .map(|s| cadrs_core::rebuild::build(&s.part_features()))
            });
            let props = d
                .doc
                .element(o.element)
                .map(|e| e.part_props().to_vec())
                .unwrap_or_default();
            let part = build
                .as_ref()
                .and_then(|b| b.part(o.part))
                .map(|p| cadrs_core::parts::display_name(p, &props).to_string())
                .unwrap_or_else(|| "Part".into());
            (o.id, o.top, format!("{part} <{}>", o.index))
        })
        .collect()
}

/// The draft of an assembly's machine, worked out on its parts as built (so connectors picked
/// on faces resolve as they do in the app).
fn draft_of(d: &ActiveDocument, element: ElementId) -> Result<Draft, String> {
    let asm = model(d, element)?;
    let mut draft = Draft::new(element, asm.machine.as_deref().cloned());
    refresh(d, &mut draft)?;
    Ok(draft)
}

fn refresh(d: &ActiveDocument, draft: &mut Draft) -> Result<(), String> {
    let asm = model(d, draft.element)?;
    let flat = machine::flat_model(&d.doc, asm);
    // Fixed frames need no parts; a connector picked on a face needs its part rebuilt.
    let stored = [&draft.def.tool_mount, &draft.def.table_mount]
        .iter()
        .all(|c| {
            c.as_ref()
                .is_none_or(|c| machine::stored_frames(c).is_some())
        });
    if stored {
        draft.refresh(&flat, &machine::stored_frames);
    } else {
        let solids = cadrs_core::assembly::document_occurrence_solids(&d.doc, draft.element);
        draft.refresh(&flat, &machine::frames_on(&solids));
    }
    Ok(())
}

fn frame_json(f: &ConnectorFrame) -> Value {
    json!({ "origin": f.origin.map(round), "z": f.z.map(round), "x": f.x.map(round) })
}

/// The tool mount as the table mount sees it at the axis values `q`: where the spindle's nose
/// is over the table, and which way it points.
fn tool_in_table(r: &Resolved, q: &[f64]) -> Value {
    let (Some(tool), Some(table)) = r.mounts(q) else {
        return Value::Null;
    };
    let rel = tool.pose().then(&table.pose().inverse());
    json!({
        "origin": rel.translation.map(round),
        "z": rel.rotate([0.0, 0.0, 1.0]).map(round),
        "x": rel.rotate([1.0, 0.0, 0.0]).map(round),
    })
}

/// The axes of `draft` beyond their travel at its jog.
fn limit_hits(draft: &Draft) -> Vec<Value> {
    draft
        .beyond_travel()
        .iter()
        .filter_map(|name| {
            let i = draft.axis_named(name)?;
            let a = &draft.def.axes[i];
            Some(json!({
                "axis": name,
                "value": round(draft.value(i)),
                "min": round(a.min),
                "max": round(a.max),
            }))
        })
        .collect()
}

fn describe_machine(d: &ActiveDocument, draft: &Draft) -> Result<Value, String> {
    let asm = model(d, draft.element)?;
    let names = occurrence_names(d, asm);
    let mount = |c: Option<&MateConnector>| match c {
        None => Value::Null,
        Some(c) => {
            let mut v = frame_json(&c.local_frame(None));
            v["instance"] = json!(if c.is_origin() {
                "Origin".to_string()
            } else {
                names
                    .iter()
                    .find(|n| n.0 == c.instance)
                    .map_or_else(|| c.instance.to_string(), |n| n.2.clone())
            });
            v
        }
    };
    let def = &draft.def;
    let axes: Vec<Value> = def
        .axes
        .iter()
        .enumerate()
        .map(|(i, a)| {
            json!({
                "name": a.name,
                "mate": draft.axis_info[i].name,
                "side": match a.side { machine::Side::Tool => "tool", machine::Side::Table => "table" },
                "kind": if a.is_rotary() { "rotary" } else { "linear" },
                "unit": if a.is_rotary() { "deg" } else { "mm" },
                "value": round(draft.value(i)),
                "reversed": a.reversed,
                "home": round(a.home),
                "min": round(a.min),
                "max": round(a.max),
                "limits_from_mate": a.limits_from_mate,
                "continuous": a.continuous,
                "rapid": round(a.rapid),
                "max_feed": round(a.max_feed),
            })
        })
        .collect();
    let actuators: Vec<Value> = def
        .actuators
        .iter()
        .enumerate()
        .map(|(k, a)| {
            json!({
                "name": a.name,
                "mate": draft.actuator_info[k].as_ref().map(|i| i.name.clone()),
                "unit": if a.dof.is_angle() { "deg" } else { "mm" },
                "value": round(draft.actuator_value(k)),
                "positions": a.positions.iter().map(|p| json!({ "name": p.0, "value": round(p.1) })).collect::<Vec<_>>(),
                "magazine": def.tool_change.magazine == Some(a.id),
            })
        })
        .collect();
    let issues: Vec<Value> = draft
        .issues
        .iter()
        .map(|i| {
            json!({
                "severity": if i.severity == Severity::Error { "error" } else { "warning" },
                "text": i.text,
            })
        })
        .collect();
    let c = &def.controller;
    Ok(json!({
        "assembly": d.doc.element(draft.element).map(|e| e.name.clone()),
        "set_up": draft.stored.is_some(),
        "is_machine": draft.resolved.is_some(),
        "tool_mount": mount(def.tool_mount.as_ref()),
        "table_mount": mount(def.table_mount.as_ref()),
        "axes": axes,
        "actuators": actuators,
        "free_mates": draft.free.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        "tool_change": {
            "position": def.tool_change.position.iter().map(|p| json!({ "axis": p.0, "value": round(p.1) })).collect::<Vec<_>>(),
            "pockets": def.tool_change.pockets.iter().map(|p| json!({ "number": p.number, "value": round(p.value) })).collect::<Vec<_>>(),
        },
        "controller": {
            "machine": c.title(),
            "dialect": c.dialect.label(),
            "tcp": c.post.rotary.tcp,
            "tool_changer": c.tool_changer,
            "max_spindle": c.max_spindle,
        },
        "tool_in_table": draft.resolved.as_ref().map(|r| tool_in_table(r, &draft.values())),
        "issues": issues,
    }))
}

/// `get_machine`: an assembly's machine definition, the axes its mates make with where each
/// is now, its actuators and what stands in its way.
pub fn get_machine(world: &mut World, p: &tools::AssemblyName) -> Result<Value, String> {
    let d = doc(world)?;
    let element = assembly(d, p.assembly.as_deref())?;
    // An open Machine Builder's draft is what the user sees: its jog is where the axes are.
    if let Some(b) = world
        .get_resource::<MachineBuilder>()
        .filter(|b| b.draft.element == element && !b.draft.changed())
    {
        return describe_machine(d, &b.draft);
    }
    describe_machine(d, &draft_of(d, element)?)
}

/// A mount at a fixed frame of an instance, as `add_mate` takes its connectors.
fn mount_of(
    d: &ActiveDocument,
    asm: &Assembly,
    f: &tools::MateFrame,
) -> Result<MateConnector, String> {
    let z = f.z.unwrap_or([0.0, 0.0, 1.0]);
    let x =
        f.x.unwrap_or_else(|| cadrs_core::assembly::connector::x_for(z));
    let frame = ConnectorFrame::new(f.origin, z, x);
    if f.instance.eq_ignore_ascii_case("origin") {
        let mut c = MateConnector::origin();
        c.frame = frame;
        return Ok(c);
    }
    let names = occurrence_names(d, asm);
    // By the occurrence's name or id, or the top-level instance's id (its first part).
    let found = names
        .iter()
        .find(|n| n.2 == f.instance || n.0.to_string() == f.instance)
        .or_else(|| names.iter().find(|n| n.1.to_string() == f.instance))
        .or_else(|| {
            // "Spindle" for "Spindle <1>" when there is one such.
            let mut by_part = names.iter().filter(|n| {
                n.2.rsplit_once(" <")
                    .is_some_and(|(part, _)| part == f.instance)
            });
            let first = by_part.next()?;
            by_part.next().is_none().then_some(first)
        })
        .ok_or_else(|| {
            format!(
                "no instance named {:?}; the instances are {}",
                f.instance,
                names
                    .iter()
                    .map(|n| n.2.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    Ok(MateConnector::at(found.0, frame))
}

fn dialect(name: &str) -> Result<Dialect, String> {
    let key = |s: &str| s.to_ascii_lowercase().replace([' ', '-', '/'], "");
    Dialect::ALL
        .into_iter()
        .find(|d| key(d.label()).starts_with(&key(name)) || key(&format!("{d:?}")) == key(name))
        .ok_or_else(|| {
            format!(
                "unknown dialect {name:?}: {}",
                Dialect::ALL.map(|d| d.label()).join(", ")
            )
        })
}

fn catalogue_machine(name: &str) -> Result<cadrs_core::cam::cadrs_cam::Machine, String> {
    let cat = catalog();
    cat.iter()
        .find(|m| m.title() == name || m.model == name)
        .cloned()
        .ok_or_else(|| {
            format!(
                "no catalogue machine named {name:?}; the catalogue has {}",
                cat.iter()
                    .map(|m| m.model.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// The edits of a `set_machine` call on a draft (the document is not touched).
fn apply_set_machine(
    d: &ActiveDocument,
    draft: &mut Draft,
    p: &tools::SetMachine,
) -> Result<(), String> {
    let asm = model(d, draft.element)?;
    if let Some(f) = &p.tool_mount {
        draft.set_mount(Mount::Tool, Some(mount_of(d, asm, f)?));
    }
    if let Some(f) = &p.table_mount {
        draft.set_mount(Mount::Table, Some(mount_of(d, asm, f)?));
    }
    if !draft.mounted() {
        return Err("a machine needs both mounts: give tool_mount and table_mount".into());
    }
    // The axes the mounts give, then the overrides on them.
    refresh(d, draft)?;
    for o in p.axes.iter().flatten() {
        let i = (0..draft.def.axes.len())
            .find(|i| draft.axis_info[*i].name == o.mate)
            .or_else(|| draft.axis_named(&o.mate))
            .ok_or_else(|| {
                format!(
                    "no axis on a mate named {:?}; the axes' mates are {}",
                    o.mate,
                    draft
                        .axis_info
                        .iter()
                        .map(|i| i.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        if let Some(name) = &o.name
            && draft.def.axes[i].name != *name
            && !draft.rename_axis(i, name)
        {
            return Err(format!("the axis name {name:?} is empty or another axis's"));
        }
        if o.reversed.is_some_and(|r| r != draft.def.axes[i].reversed) {
            draft.reverse_axis(i);
        }
        if let Some(on) = o.continuous {
            if on && !draft.def.axes[i].is_rotary() {
                return Err(format!("{:?} is a slide: it cannot be continuous", o.mate));
            }
            draft.set_continuous(i, on);
        }
        if let Some(v) = o.min {
            draft.set_travel(i, false, v);
        }
        if let Some(v) = o.max {
            draft.set_travel(i, true, v);
        }
        if let Some(v) = o.home {
            draft.set_home(i, v);
        }
    }
    refresh(d, draft)?;
    for o in p.actuators.iter().flatten() {
        let k =
            match draft
                .actuator_info
                .iter()
                .position(|i| i.as_ref().is_some_and(|i| i.name == o.mate))
            {
                Some(k) => k,
                None => {
                    let free = draft.free.iter().position(|f| f.name == o.mate).ok_or_else(|| {
                    format!(
                        "no free mate named {:?} to make an actuator of; the free mates are {}",
                        o.mate,
                        draft.free.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")
                    )
                })?;
                    draft.add_actuator(free);
                    refresh(d, draft)?;
                    draft.def.actuators.len() - 1
                }
            };
        if let Some(name) = &o.name
            && draft.def.actuators[k].name != *name
            && !draft.rename_actuator(k, name)
        {
            return Err(format!(
                "the actuator name {name:?} is empty or another actuator's"
            ));
        }
        if let Some(positions) = &o.positions {
            draft.def.actuators[k].positions = positions
                .iter()
                .map(|p| (p.name.clone(), p.value))
                .collect();
            draft.def.actuators[k].rest = 0;
        }
        if o.magazine == Some(true) {
            draft.set_magazine(Some(k));
            draft.pockets_from_positions();
        } else if o.magazine == Some(false) && draft.magazine() == Some(k) {
            draft.set_magazine(None);
        }
    }
    if let Some(c) = &p.controller {
        if let Some(name) = &c.catalogue {
            draft.def.controller = catalogue_machine(name)?;
            draft.def.controller.chain = None;
        }
        let m = &mut draft.def.controller;
        if let Some(name) = &c.dialect {
            m.dialect = dialect(name)?;
        }
        if let Some(on) = c.tcp {
            m.post.rotary.tcp = on;
        }
        if let Some(on) = c.tool_changer {
            m.tool_changer = on;
        }
        if let Some(v) = c.max_spindle {
            m.max_spindle = v;
        }
    }
    refresh(d, draft)
}

/// `set_machine`: the mounts given, the axes the mates make between them, the overrides on
/// those, stored as the assembly's machine definition in one undo step (`clear`: removed).
pub fn set_machine(world: &mut World, p: &tools::SetMachine) -> Result<Value, String> {
    let d = doc(world)?;
    let element = assembly(d, p.assembly.as_deref())?;
    let mut draft = draft_of(d, element)?;
    if p.clear {
        if draft.stored.is_none() {
            return Err("the assembly is not set up as a machine".into());
        }
        let cmd = draft.remove_command();
        world
            .resource_mut::<ActiveDocument>()
            .execute(&cmd)
            .map_err(|e| e.to_string())?;
        return Ok(json!({ "removed": true }));
    }
    apply_set_machine(d, &mut draft, p)?;
    if draft.changed() {
        let cmd = draft.command();
        world
            .resource_mut::<ActiveDocument>()
            .execute(&cmd)
            .map_err(|e| e.to_string())?;
    }
    let d = doc(world)?;
    describe_machine(d, &draft_of(d, element)?)
}

/// `jog_machine`: the machine's axes (and actuators) to the values given, by name. A preview
/// by default: the Machine Builder opens on the assembly with the jog, and the document is
/// not changed. `commit`: the instances are moved there, one undo step.
pub fn jog_machine(world: &mut World, p: &tools::JogMachine) -> Result<Value, String> {
    let d = doc(world)?;
    let element = assembly(d, p.assembly.as_deref())?;
    // On an open builder's draft (its jog goes on from where it is), else a fresh one.
    let open = world
        .get_resource::<MachineBuilder>()
        .filter(|b| b.draft.element == element)
        .map(|b| b.draft.clone());
    let mut draft = match open {
        Some(mut draft) => {
            refresh(d, &mut draft)?;
            draft
        }
        None => draft_of(d, element)?,
    };
    if draft.resolved.is_none() {
        let why = draft
            .issues
            .iter()
            .find(|i| i.severity == Severity::Error)
            .or(draft.issues.first())
            .map_or("it is not set up as a machine".to_string(), |i| {
                i.text.clone()
            });
        return Err(format!("the assembly is not a working machine: {why}"));
    }
    if p.home {
        draft.go_home();
    }
    for (name, v) in &p.axes {
        let i = draft.axis_named(name).ok_or_else(|| {
            format!(
                "no axis named {name:?}; the axes are {}",
                draft
                    .def
                    .axes
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if !v.is_finite() {
            return Err(format!("{name}: not a number"));
        }
        draft.jog(i, *v);
    }
    for (name, v) in p.actuators.iter().flatten() {
        let k = draft
            .def
            .actuators
            .iter()
            .position(|a| a.name == *name)
            .ok_or_else(|| format!("no actuator named {name:?}"))?;
        // A position's name, or a number (mm or degrees).
        let value = match v {
            tools::ActuatorTarget::Value(x) => *x,
            tools::ActuatorTarget::Position(pos) => draft.def.actuators[k]
                .positions
                .iter()
                .find(|q| q.0 == *pos)
                .map(|q| q.1)
                .ok_or_else(|| format!("{name} has no position named {pos:?}"))?,
        };
        draft.jog_actuator(k, value);
    }
    let r = draft.resolved.clone().ok_or("not a machine")?;
    let q = draft.values();
    let mut out = json!({
        "axes": draft.def.axes.iter().enumerate().map(|(i, a)| (a.name.clone(), json!(round(q[i])))).collect::<serde_json::Map<_, _>>(),
        "actuators": draft.def.actuators.iter().enumerate().map(|(k, a)| (a.name.clone(), json!(round(draft.actuator_value(k))))).collect::<serde_json::Map<_, _>>(),
        "tool_in_table": tool_in_table(&r, &q),
        "limit_hits": limit_hits(&draft),
    });
    if p.commit {
        let poses = draft.poses();
        out["moved"] = json!(poses.len());
        if !poses.is_empty() {
            world
                .resource_mut::<ActiveDocument>()
                .execute(&MoveInstances {
                    element,
                    poses,
                    label: "Jog machine".into(),
                })
                .map_err(|e| e.to_string())?;
        }
        // The builder, if open, is where the document now has the machine.
        if let Some(mut b) = world.get_resource_mut::<MachineBuilder>()
            && b.draft.element == element
        {
            b.script_reset();
        }
        out["committed"] = json!(true);
        return Ok(out);
    }
    // The preview is the Machine Builder's: it opens on the assembly with the jog.
    world.resource_mut::<ActiveDocument>().set_active(element);
    if let Some(mut panel) = world.get_resource_mut::<crate::side_panel::SidePanel>() {
        *panel = crate::side_panel::SidePanel::MachineBuilder;
    }
    world.insert_resource(MachineBuilder::with_draft(draft));
    out["committed"] = json!(false);
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// CAM

/// The CAM Studio tab a call names, else the active tab when it is one, else the document's
/// only one.
fn cam_studio<'a>(
    d: &'a ActiveDocument,
    name: Option<&str>,
) -> Result<(ElementId, &'a CamStudio), String> {
    let all: Vec<_> = d
        .doc
        .elements
        .iter()
        .filter_map(|e| e.cam().map(|c| (e, c)))
        .collect();
    let found = match name {
        Some(n) => all.iter().find(|(e, _)| e.name == n).ok_or_else(|| {
            format!(
                "no CAM Studio named {n:?}; the CAM Studios are {}",
                all.iter()
                    .map(|(e, _)| e.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?,
        None => all
            .iter()
            .find(|(e, _)| d.active == Some(e.id))
            .or(if all.len() == 1 { all.first() } else { None })
            .ok_or("name the CAM Studio (cam): the active tab is not one")?,
    };
    Ok((found.0.id, found.1))
}

fn describe_cam(world: &World, element: ElementId) -> Result<Value, String> {
    let d = doc(world)?;
    let e = d.doc.element(element).ok_or("the CAM Studio is gone")?;
    let cam = e.cam().ok_or("not a CAM Studio")?;
    // Toolpaths are calculated for the tab that is open.
    let computed = world
        .get_resource::<crate::cam::Computed>()
        .filter(|_| d.active == Some(element));
    let active = cam.active().map(|j| j.id);
    let jobs: Vec<Value> = cam
        .jobs
        .iter()
        .map(|j| {
            let machine = j.machine.as_ref().map(|m| {
                json!({
                    "name": m.title(),
                    "kind": m.kind.label(),
                    "dialect": m.dialect.label(),
                    "axes": crate::cam::dialogs::axes_summary(m),
                    "rotary_axes": m.rotary_axes(),
                    "from": match &j.machine_source {
                        Some(s) => json!({ "assembly": s.source_name, "label": s.label() }),
                        None => json!("catalogue"),
                    },
                })
            });
            let mut warnings: Vec<String> = Vec::new();
            let setups: Vec<Value> = j
                .setups
                .iter()
                .map(|s| {
                    let operations: Vec<Value> = s
                        .operations
                        .iter()
                        .map(|o| {
                            let mut v = json!({ "name": o.name, "suppressed": o.suppressed });
                            match &o.kind {
                                OpKind::Tool(t) => {
                                    v["kind"] = json!("tool");
                                    v["tool"] = json!(t.tool.name);
                                    v["number"] = json!(t.number);
                                }
                                OpKind::Toolpath(tp) => {
                                    v["kind"] = json!("toolpath");
                                    v["pattern"] = json!(tp.params.pattern.label());
                                    let done = computed.and_then(|c| c.current(o.id));
                                    v["computed"] = json!(done.is_some());
                                    if let Some(done) = done {
                                        v["warnings"] = json!(done.toolpath.warnings);
                                        warnings.extend(
                                            done.toolpath
                                                .warnings
                                                .iter()
                                                .map(|w| format!("{}: {w}", o.name)),
                                        );
                                    }
                                }
                                OpKind::WorkPlane(_) => v["kind"] = json!("work plane"),
                            }
                            v
                        })
                        .collect();
                    json!({
                        "name": s.name,
                        "program": s.program_number,
                        "suppressed": s.suppressed,
                        "mating": match s.mating {
                            Mating::Automatic => json!("automatic"),
                            Mating::Manual(o) => json!({ "manual": o.map(round) }),
                            Mating::Mount { origin, z, x } => json!({ "mount": {
                                "origin": origin.map(round), "z": z.map(round), "x": x.map(round),
                            } }),
                        },
                        "operations": operations,
                    })
                })
                .collect();
            json!({
                "name": j.name,
                "active": active == Some(j.id),
                "machine": machine,
                "setups": setups,
                "warnings": warnings,
            })
        })
        .collect();
    Ok(json!({
        "cam": e.name,
        "components": cam.components.iter().map(|c| json!({ "name": c.name, "role": format!("{:?}", c.role) })).collect::<Vec<_>>(),
        "jobs": jobs,
    }))
}

/// `get_cam`: a CAM Studio's jobs, each with its machine and where that comes from, its
/// setups with their mating, and their operations.
pub fn get_cam(world: &mut World, p: &tools::GetCam) -> Result<Value, String> {
    let (element, _) = cam_studio(doc(world)?, p.cam.as_deref())?;
    describe_cam(world, element)
}

/// `set_cam_machine`: a job's machine from the catalogue, or from an assembly of the document
/// set up as a machine (followed from then on).
pub fn set_cam_machine(world: &mut World, p: &tools::SetCamMachine) -> Result<Value, String> {
    let d = doc(world)?;
    let (element, cam) = cam_studio(d, p.cam.as_deref())?;
    let job = match &p.job {
        Some(n) => cam.jobs.iter().find(|j| j.name == *n).ok_or_else(|| {
            format!(
                "no job named {n:?}; the jobs are {}",
                cam.jobs
                    .iter()
                    .map(|j| j.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?,
        None => cam.active().ok_or("the CAM Studio has no job")?,
    };
    let (job_id, followed) = (job.id, job.machine_source.is_some());
    let edits: Vec<Edit> = match (&p.catalogue, &p.assembly) {
        (Some(_), Some(_)) => return Err("give catalogue or assembly, not both".into()),
        (None, None) => {
            return Err("give catalogue (a machine's name) or assembly (a tab's name)".into());
        }
        (Some(name), None) => {
            let machine = catalogue_machine(name)?;
            let mut edits = Vec::new();
            if followed {
                edits.push(Edit::SetMachineSource {
                    job: job_id,
                    source: None,
                    links: Vec::new(),
                });
            }
            edits.push(Edit::SetMachine {
                job: job_id,
                machine: Some(Box::new(machine)),
            });
            edits
        }
        (None, Some(name)) => {
            let asm = assembly(d, Some(name))?;
            let source = MachineSource::workspace(&d.doc, asm).ok_or("not an Assembly")?;
            vec![Edit::SetMachineSource {
                job: job_id,
                source: Some(Box::new(source)),
                links: Vec::new(),
            }]
        }
    };
    for e in edits {
        world
            .resource_mut::<ActiveDocument>()
            .execute(&EditCam::new(element, e))
            .map_err(|e| e.to_string())?;
    }
    describe_cam(world, element)
}

/// A definition as `set_machine` would leave it, without touching the document.
#[cfg(test)]
fn preview_set_machine(
    d: &ActiveDocument,
    element: ElementId,
    p: &tools::SetMachine,
) -> Result<machine::MachineDef, String> {
    let mut draft = draft_of(d, element)?;
    apply_set_machine(d, &mut draft, p)?;
    Ok(draft.def)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::samples::cnc_machines as cnc;

    /// A world with the CNC machines sample open.
    fn world() -> World {
        let mut w = World::new();
        w.insert_resource(ActiveDocument::new(cnc::document().unwrap()));
        w.insert_resource(crate::side_panel::SidePanel::default());
        w
    }

    fn undo_len(w: &World) -> usize {
        w.resource::<ActiveDocument>().history.undo_len()
    }

    fn names(v: &Value) -> Vec<&str> {
        v["axes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect()
    }

    fn name(assembly: &str) -> tools::AssemblyName {
        tools::AssemblyName {
            assembly: Some(assembly.into()),
        }
    }

    #[test]
    fn get_machine_describes_the_samples_mills() {
        let mut w = world();
        let v = get_machine(&mut w, &name("5-axis mill")).unwrap();
        assert_eq!(names(&v), ["Z", "B", "Y", "X", "C"]);
        assert_eq!(
            (v["set_up"].clone(), v["is_machine"].clone()),
            (json!(true), json!(true))
        );
        let b = &v["axes"][1];
        assert_eq!(
            (b["kind"].as_str(), b["side"].as_str()),
            (Some("rotary"), Some("tool"))
        );
        assert_eq!(
            (b["min"].as_f64(), b["max"].as_f64()),
            (Some(-100.0), Some(100.0))
        );
        assert_eq!(v["tool_mount"]["instance"], json!("Spindle <1>"));
        assert_eq!(v["controller"]["tcp"], json!(true));
        // The spindle's nose is straight over the platter, looking down.
        assert_eq!(v["tool_in_table"]["z"], json!([0.0, 0.0, -1.0]));
        let v = get_machine(&mut w, &name("4-axis mill")).unwrap();
        assert_eq!(v["actuators"][0]["name"], json!("Magazine"));
        assert_eq!(v["actuators"][0]["magazine"], json!(true));
        assert_eq!(v["tool_change"]["pockets"].as_array().unwrap().len(), 8);
        // Not an assembly, and no such tab.
        assert!(get_machine(&mut w, &name("Machine parts")).is_err());
        assert!(get_machine(&mut w, &name("Lathe")).is_err());
        assert_eq!(undo_len(&w), 0);
    }

    fn frame(instance: &str, origin: [f64; 3], z: [f64; 3]) -> tools::MateFrame {
        tools::MateFrame {
            instance: instance.into(),
            origin,
            z: Some(z),
            x: None,
        }
    }

    #[test]
    fn set_machine_detects_then_overrides_in_one_step() {
        let mut w = world();
        // Cleared, the assembly is an ordinary one again (one step).
        let cleared = set_machine(
            &mut w,
            &tools::SetMachine {
                assembly: Some("3-axis mill".into()),
                clear: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cleared["removed"], json!(true));
        assert_eq!(undo_len(&w), 1);
        let v = get_machine(&mut w, &name("3-axis mill")).unwrap();
        assert_eq!(v["set_up"], json!(false));
        // Without both mounts there is nothing to detect.
        let err = set_machine(
            &mut w,
            &tools::SetMachine {
                assembly: Some("3-axis mill".into()),
                tool_mount: Some(frame("Spindle", [0.0, 0.0, 400.0], [0.0, 0.0, -1.0])),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("both mounts"), "{err}");
        assert_eq!(undo_len(&w), 1);
        // The mounts by part name, an axis renamed and its travel typed, a controller.
        let p = tools::SetMachine {
            assembly: Some("3-axis mill".into()),
            tool_mount: Some(frame("Spindle", [0.0, 0.0, 400.0], [0.0, 0.0, -1.0])),
            table_mount: Some(frame("Table <1>", [0.0; 3], [0.0, 0.0, 1.0])),
            axes: Some(vec![tools::MachineAxisParam {
                mate: "X".into(),
                name: Some("U".into()),
                min: Some(-200.0),
                max: Some(180.0),
                home: Some(10.0),
                ..Default::default()
            }]),
            controller: Some(tools::MachineControllerParam {
                dialect: Some("linuxcnc".into()),
                tool_changer: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let v = set_machine(&mut w, &p).unwrap();
        assert_eq!(undo_len(&w), 2, "one step for the whole definition");
        assert_eq!(names(&v), ["Z", "Y", "U"]);
        let u = &v["axes"][2];
        assert_eq!(
            (
                u["min"].as_f64(),
                u["max"].as_f64(),
                u["home"].as_f64(),
                u["mate"].as_str()
            ),
            (Some(-200.0), Some(180.0), Some(10.0), Some("X"))
        );
        assert_eq!(u["limits_from_mate"], json!(false));
        assert_eq!(v["controller"]["dialect"], json!("LinuxCNC"));
        assert_eq!(v["controller"]["tool_changer"], json!(false));
        assert_eq!(v["is_machine"], json!(true));
        // The same call again changes nothing: no step.
        set_machine(&mut w, &p).unwrap();
        assert_eq!(undo_len(&w), 2);
        // What it refuses, with the names to choose from.
        let bad = |axes: tools::MachineAxisParam| {
            preview_set_machine(
                w.resource::<ActiveDocument>(),
                cnc::MILL_3,
                &tools::SetMachine {
                    axes: Some(vec![axes]),
                    ..Default::default()
                },
            )
            .unwrap_err()
        };
        let e = bad(tools::MachineAxisParam {
            mate: "Q".into(),
            ..Default::default()
        });
        assert!(e.contains("\"Q\"") && e.contains("X"), "{e}");
        let e = bad(tools::MachineAxisParam {
            mate: "Y".into(),
            name: Some("Z".into()),
            ..Default::default()
        });
        assert!(e.contains("another axis"), "{e}");
        let e = bad(tools::MachineAxisParam {
            mate: "Y".into(),
            continuous: Some(true),
            ..Default::default()
        });
        assert!(e.contains("slide"), "{e}");
        let e = set_machine(
            &mut w,
            &tools::SetMachine {
                assembly: Some("3-axis mill".into()),
                tool_mount: Some(frame("Chuck", [0.0; 3], [0.0, 0.0, 1.0])),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(
            e.contains("no instance named") && e.contains("Spindle <1>"),
            "{e}"
        );
    }

    #[test]
    fn jog_machine_previews_or_commits() {
        let mut w = world();
        let before = w.resource::<ActiveDocument>().doc.clone();
        let jog = |axes: &[(&str, f64)], commit: bool| tools::JogMachine {
            assembly: Some("5-axis mill".into()),
            axes: axes.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
            actuators: None,
            home: false,
            commit,
        };
        // A preview: the builder opens with the jog; the document is as it was.
        let v = jog_machine(&mut w, &jog(&[("B", 30.0), ("C", 45.0)], false)).unwrap();
        assert_eq!(
            (v["axes"]["B"].as_f64(), v["axes"]["C"].as_f64()),
            (Some(30.0), Some(45.0))
        );
        assert_eq!(v["committed"], json!(false));
        assert_eq!(w.resource::<ActiveDocument>().doc.elements, before.elements);
        assert_eq!(undo_len(&w), 0);
        let b = w.resource::<MachineBuilder>();
        assert_eq!(b.draft.value(b.draft.axis_named("B").unwrap()), 30.0);
        assert_eq!(
            *w.resource::<crate::side_panel::SidePanel>(),
            crate::side_panel::SidePanel::MachineBuilder
        );
        assert_eq!(w.resource::<ActiveDocument>().active, Some(cnc::MILL_5));
        // The tool leans by B as the table sees it (C turns the table under it).
        let z = v["tool_in_table"]["z"].as_array().unwrap();
        assert!(
            (z[2].as_f64().unwrap() + 30f64.to_radians().cos()).abs() < 1e-3,
            "{z:?}"
        );
        // The next jog goes on from there; beyond its travel an axis is a limit hit.
        let v = jog_machine(&mut w, &jog(&[("B", 130.0)], false)).unwrap();
        assert_eq!(v["axes"]["C"].as_f64(), Some(45.0));
        assert_eq!(v["limit_hits"][0]["axis"], json!("B"));
        assert_eq!(v["limit_hits"][0]["max"].as_f64(), Some(100.0));
        // Committed: one MoveInstances step, and get_machine reads the axes there.
        let v = jog_machine(&mut w, &jog(&[("B", 20.0), ("C", 0.0)], true)).unwrap();
        assert_eq!(v["committed"], json!(true));
        assert!(v["moved"].as_u64().unwrap() >= 1);
        assert_eq!(undo_len(&w), 1);
        w.remove_resource::<MachineBuilder>();
        let now = get_machine(&mut w, &name("5-axis mill")).unwrap();
        assert_eq!(now["axes"][1]["value"].as_f64(), Some(20.0));
        // Home, and what it refuses.
        let mut home = jog(&[], false);
        home.home = true;
        let v = jog_machine(&mut w, &home).unwrap();
        assert_eq!(v["axes"]["B"].as_f64(), Some(0.0));
        let e = jog_machine(&mut w, &jog(&[("Q", 1.0)], false)).unwrap_err();
        assert!(
            e.contains("no axis named") && e.contains("Z, B, Y, X, C"),
            "{e}"
        );
        let mut named = jog(&[], false);
        named.assembly = Some("4-axis mill".into());
        named.actuators = Some(
            [(
                "Magazine".to_string(),
                tools::ActuatorTarget::Position("Pocket 3".into()),
            )]
            .into_iter()
            .collect(),
        );
        let v = jog_machine(&mut w, &named).unwrap();
        assert_eq!(v["actuators"]["Magazine"].as_f64(), Some(90.0));
    }

    #[test]
    fn the_cam_machine_comes_from_the_catalogue_or_an_assembly() {
        let mut w = world();
        let cam = |c: &str| tools::GetCam {
            cam: Some(c.into()),
        };
        let v = get_cam(&mut w, &cam("CAM 5-axis")).unwrap();
        let job = &v["jobs"][0];
        assert_eq!(job["machine"]["from"]["assembly"], json!("5-axis mill"));
        assert_eq!(job["machine"]["axes"], json!("Z B · Y X C"));
        assert_eq!(job["machine"]["rotary_axes"], json!(2));
        let ops = job["setups"][0]["operations"].as_array().unwrap();
        assert!(
            ops.iter()
                .any(|o| o["kind"] == json!("toolpath") && o["pattern"].is_string())
        );
        assert!(
            ops.iter()
                .any(|o| o["kind"] == json!("tool") && o["number"] == json!(1))
        );
        assert!(job["setups"][0]["mating"].is_object() || job["setups"][0]["mating"].is_string());
        // With no CAM Studio named and several in the document, it asks which.
        assert!(get_cam(&mut w, &tools::GetCam { cam: None }).is_err());
        // To a catalogue machine: the source let go, then the machine set.
        let set = |catalogue: Option<&str>, assembly: Option<&str>| tools::SetCamMachine {
            cam: Some("CAM 5-axis".into()),
            job: None,
            catalogue: catalogue.map(str::to_string),
            assembly: assembly.map(str::to_string),
        };
        let first = catalog().remove(0);
        let v = set_cam_machine(&mut w, &set(Some(&first.model), None)).unwrap();
        assert_eq!(v["jobs"][0]["machine"]["from"], json!("catalogue"));
        assert_eq!(v["jobs"][0]["machine"]["name"], json!(first.title()));
        let steps = undo_len(&w);
        assert_eq!(steps, 2);
        // To another assembly's machine: one step.
        let v = set_cam_machine(&mut w, &set(None, Some("4-axis mill"))).unwrap();
        assert_eq!(
            v["jobs"][0]["machine"]["from"]["assembly"],
            json!("4-axis mill")
        );
        assert_eq!(v["jobs"][0]["machine"]["axes"], json!("Z · Y X A"));
        assert_eq!(undo_len(&w), steps + 1);
        // What it refuses.
        assert!(set_cam_machine(&mut w, &set(None, None)).is_err());
        assert!(set_cam_machine(&mut w, &set(Some("x"), Some("y"))).is_err());
        let e = set_cam_machine(&mut w, &set(Some("Lathe 9000"), None)).unwrap_err();
        assert!(e.contains("no catalogue machine"), "{e}");
        let e = set_cam_machine(&mut w, &set(None, Some("Test piece"))).unwrap_err();
        assert!(e.contains("not an Assembly"), "{e}");
        // An assembly that is no machine is refused with why, and nothing changes.
        set_machine(
            &mut w,
            &tools::SetMachine {
                assembly: Some("3-axis mill".into()),
                clear: true,
                ..Default::default()
            },
        )
        .unwrap();
        let steps = undo_len(&w);
        let e = set_cam_machine(&mut w, &set(None, Some("3-axis mill"))).unwrap_err();
        assert!(e.contains("not set up as a machine"), "{e}");
        assert_eq!(undo_len(&w), steps);
    }
}
