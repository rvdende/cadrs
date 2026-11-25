//! Configurations in drawings: the view menu's **Configuration ▸** for a view of a configured
//! Part Studio (each List's rows, ✓ on the view's, and each Checkbox). Picking one shows the
//! view, and the views projected from it, in that configuration: they reference the studio's
//! copy in it ([`cadrs_core::configuration`]), the drawing records its state the first time,
//! and the drawing keeps the configuration ([`WithConfiguration`]), in one undoable step. The
//! Insert view dialog picks the configuration of new views ([`super::view_tools`]).

use bevy::prelude::*;
use cadrs_core::ElementId;
use cadrs_core::configuration::{
    self as cfg, ConfigValues, InputKind, InputValue, InstanceConfiguration, WithConfiguration,
};
use cadrs_core::document::Element;
use cadrs_drawing::{DrawingOp, View, ViewId};
use cadrs_ui::MenuEntry;
use cadrs_ui::i18n::tr;
use cadrs_ui::prelude::*;

use super::active_drawing;
use crate::ActiveDocument;

/// The configured Part Studio view `v` shows and the configuration it shows it in (its copy's,
/// else the studio's active one).
fn shown(doc: &cadrs_core::Document, v: &View) -> Option<(Element, ConfigValues)> {
    let shown = ElementId(v.reference.element);
    let tab = cfg::tab_of(doc, shown);
    let el = doc.elements.iter().find(|e| e.id == tab)?;
    if el.configuration.is_empty() || !matches!(el.kind, cadrs_core::ElementKind::PartStudio { .. })
    {
        return None;
    }
    let values = cfg::configuration_of(doc, shown)
        .map(|c| c.values)
        .unwrap_or_else(|| el.configuration.active.clone());
    Some((el.clone(), values))
}

/// **Configuration ▸** for view `v` (`None` unless its studio is configured): each List's rows
/// (`view-config-<k>-<r>`), each Checkbox (`view-config-<k>-toggle`).
pub fn menu_item(doc: &cadrs_core::Document, v: &View) -> Option<MenuItem> {
    let (el, values) = shown(doc, v)?;
    let c = &el.configuration;
    let mut entries: Vec<MenuEntry> = Vec::new();
    for (k, (input, r)) in c.inputs.iter().zip(c.resolve(&values)).enumerate() {
        match (&input.kind, r) {
            (InputKind::List { rows, .. }, cfg::Resolved::Row(cur)) => entries.push(
                MenuItem::new(format!("view-config-{k}"), input.name.clone())
                    .submenu(
                        rows.iter()
                            .enumerate()
                            .map(|(j, row)| {
                                MenuItem::new(format!("view-config-{k}-{j}"), row.name.clone())
                                    .checked(j == cur)
                                    .into()
                            })
                            .collect(),
                    )
                    .into(),
            ),
            (InputKind::Checkbox { .. }, cfg::Resolved::Checked(on)) => entries.push(
                MenuItem::new(format!("view-config-{k}-toggle"), input.name.clone())
                    .checked(on)
                    .into(),
            ),
            _ => {}
        }
    }
    Some(
        MenuItem::new("view-config", tr("config-instance-configuration"))
            .icon("configurations")
            .submenu(entries),
    )
}

/// A `view-config-…` item picked for view `id`. Returns whether it was one.
pub fn menu_action(world: &mut World, id: ViewId, item: &str) -> bool {
    let Some(rest) = item.strip_prefix("view-config-") else {
        return false;
    };
    let Some((el, mut values, v)) = world.get_resource::<ActiveDocument>().and_then(|doc| {
        let (_, d) = active_drawing(doc)?;
        let (_, v) = d.view(id)?;
        let (el, values) = shown(&doc.doc, v)?;
        Some((el, values, v.clone()))
    }) else {
        return true;
    };
    let mut parts = rest.splitn(2, '-');
    let Some(input) = parts
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .and_then(|k| el.configuration.inputs.get(k))
    else {
        return true;
    };
    let value = match (parts.next(), &input.kind) {
        (Some("toggle"), InputKind::Checkbox { .. }) => InputValue::Checked(!matches!(
            input.resolve_value(values.get(input.id)),
            cfg::Resolved::Checked(true)
        )),
        (Some(r), InputKind::List { rows, .. }) => {
            match r.parse::<usize>().ok().and_then(|j| rows.get(j)) {
                Some(row) => InputValue::Row(row.id),
                None => return true,
            }
        }
        _ => return true,
    };
    values.set(input.id, value);
    let c = InstanceConfiguration {
        studio: el.id,
        values: el.configuration.complete(&values),
    };
    set_configuration(world, &v, c);
    true
}

/// Shows view `v` and the views projected from it (with the same reference) in configuration
/// `c`, recording the copy's state in the drawing if it has none yet.
fn set_configuration(world: &mut World, v: &View, c: InstanceConfiguration) {
    let copy = c.copy_id();
    let old = v.reference.element;
    if old == copy.0 {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some((drawing, d)) = active_drawing(doc) else {
        return;
    };
    // The view and its descendants that show the same thing.
    let all: Vec<&View> = d.sheets.iter().flat_map(|s| s.views.iter()).collect();
    let mut family = vec![v.id];
    loop {
        let more: Vec<ViewId> = all
            .iter()
            .filter(|x| {
                !family.contains(&x.id)
                    && x.reference.element == old
                    && x.parent.is_some_and(|p| family.contains(&p))
            })
            .map(|x| x.id)
            .collect();
        if more.is_empty() {
            break;
        }
        family.extend(more);
    }
    let with = cfg::with_copy(&doc.doc, &c);
    let model = with.as_ref().unwrap_or(&doc.doc);
    let mut ops = Vec::new();
    let src = match d.source(copy.0) {
        Some(s) => Some(s.clone()),
        None => {
            let s = cadrs_core::drawing_source::live_source(model, copy);
            if let Some(s) = &s {
                ops.push(DrawingOp::SetSource(s.clone()));
            }
            s
        }
    };
    let label = "Change view configuration".to_string();
    for x in all.iter().filter(|x| family.contains(&x.id)) {
        let mut nv = (*x).clone();
        nv.reference.element = copy.0;
        nv.source_hash = src.as_ref().and_then(|s| s.hash_of(nv.reference.part));
        ops.push(DrawingOp::SetView {
            view: nv,
            label: label.clone(),
        });
    }
    // A sheet whose title block reads the old reference follows when all its views moved.
    for s in &d.sheets {
        if let Some(r) = s.reference
            && r.element == old
            && s.views
                .iter()
                .all(|x| x.reference.element != old || family.contains(&x.id))
        {
            let mut props = s.props();
            props.reference = Some(cadrs_drawing::ObjectRef {
                element: copy.0,
                part: r.part,
            });
            ops.push(DrawingOp::SetSheetProps { id: s.id, props });
        }
    }
    let cmd = WithConfiguration {
        element: drawing,
        configuration: c,
        command: cadrs_core::commands::EditDrawing {
            element: drawing,
            op: DrawingOp::Batch { ops, label },
        },
    };
    if let Err(e) = world.resource_mut::<ActiveDocument>().execute(&cmd) {
        warn!("drawing edit refused: {e}");
    }
}

/// The configuration a view's reference names ("M5, Chamfered"), if it is a configured copy.
pub fn describe(doc: &cadrs_core::Document, element: uuid::Uuid) -> Option<String> {
    cfg::copy_of(doc, ElementId(element))?;
    Some(cfg::describe_element(doc, ElementId(element)))
}
