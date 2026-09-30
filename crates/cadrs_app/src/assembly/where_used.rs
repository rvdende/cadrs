//! **Where used** (P3B.9, `intro-to-assemblies.md` X15; [`cadrs_core::assembly::context::where_used`]):
//! the instance menu's **Where used…** lists the Assembly tabs of the document that use the
//! instance's part (or its Part Studio, for a rigid studio instance; or the subassembly's tab),
//! each with how many instances, directly or through a subassembly. A click on a row opens that
//! tab. P3G.2 (DV1.6): below them, the other documents of the library that reference this
//! document's tab, with the version they use (`cadrs_core::link_update::where_used`).
//!
//! Names: `where-used-dialog`, rows `where-used-row-<k>`.

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::ElementId;
use cadrs_core::assembly::context::{assembly_used_in, where_used};
use cadrs_core::assembly::{InstanceId, InstanceSource};
use cadrs_ui::prelude::*;
use cadrs_ui::FeatureDialogCancel;

use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct WhereUsedPlugin;

impl Plugin for WhereUsedPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_cancel).add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<WhereUsed>());
    }
}

#[derive(Resource, Debug, Clone)]
struct WhereUsed;

#[derive(Component)]
struct WhereUsedDialog;

#[derive(Component, Clone, Copy)]
struct UseRow(ElementId);

/// Opens the list for `instance` of the active assembly.
pub fn open(world: &mut World, instance: InstanceId) {
    close(world);
    let doc = world.resource::<ActiveDocument>();
    let Some(inst) = doc.active_element().and_then(|e| e.assembly_model()?.instance(instance).cloned()) else { return };
    let (what, rows) = match inst.source {
        InstanceSource::Part { element, part } => {
            let name = cadrs_core::assembly::source_part_name(&doc.doc, &inst.source, None);
            (format!("{name} (Part Studio {})", doc.doc.element(element).map(|e| e.name.clone()).unwrap_or_default()), where_used(&doc.doc, element, Some(part)))
        }
        InstanceSource::Studio { element } => (format!("Part Studio {}", doc.doc.element(element).map(|e| e.name.clone()).unwrap_or_default()), where_used(&doc.doc, element, None)),
        InstanceSource::Assembly { element } => (format!("Assembly {}", doc.doc.element(element).map(|e| e.name.clone()).unwrap_or_default()), assembly_used_in(&doc.doc, element)),
    };
    // P3G.2 carried: a linked instance names its source document and version.
    let what = match (inst.link, doc.doc.linked_element(inst.source.element())) {
        (Some(r), Some(l)) if r.document_or(doc.doc.id) != doc.doc.id => format!("{what}, in {} ({})", l.document_name, l.version_name),
        (Some(_), Some(l)) => format!("{what}, this document ({})", l.version_name),
        _ => what,
    };
    let rows: Vec<(ElementId, String, String)> = rows
        .into_iter()
        .filter_map(|(e, n, direct)| {
            let name = doc.doc.element(e)?.name.clone();
            let count = if n == 1 { "1 instance".to_string() } else { format!("{n} instances") };
            Some((e, name, if direct { count } else { format!("{count}, in a subassembly") }))
        })
        .collect();
    // DV1.6: the documents referencing this tab (or, for a linked instance, its source's tab),
    // at which versions: the library's, and this document's own as it is now.
    let (source_doc, tab_name) = match inst.link {
        Some(r) => (r.document_or(doc.doc.id), doc.doc.linked_element(inst.source.element()).map(|l| l.element.name.clone())),
        None => (doc.doc.id, doc.doc.element(inst.source.element()).map(|e| e.name.clone())),
    };
    let this = doc.doc.id;
    let mut usages: Vec<cadrs_core::link_update::Usage> = cadrs_core::link_update::usages_in(&doc.doc, source_doc);
    let store = world.resource::<crate::DocumentStore>().0.clone();
    usages.extend(cadrs_core::link_update::where_used(&store, source_doc).into_iter().filter(|u| u.document != this));
    let others: Vec<String> = match tab_name {
        Some(tab) => usages
            .into_iter()
            .filter(|u| u.element == tab)
            .map(|u| format!("{} › {} ({}): {}", u.document_name, u.tab, u.version, if u.count == 1 { "1 use".to_string() } else { format!("{} uses", u.count) }))
            .collect(),
        None => Vec::new(),
    };
    let t = world.resource::<Theme>().clone();
    let tb = t.clone();
    let n = rows.len();
    let bundle = (
        WhereUsedDialog,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("where-used-dialog")
            .title("Where used")
            .valid(false)
            .plain_title()
            .width(320.0)
            .body(move |b| {
                let t = &tb;
                b.spawn((t.text(what, 12.0, FontWeight::SEMIBOLD, t.foreground), Node { margin: UiRect::bottom(Val::Px(2.0)), ..default() }));
                b.spawn((
                    t.text(if n == 1 { "Used in 1 assembly of this document".to_string() } else { format!("Used in {n} assemblies of this document") }, 11.5, FontWeight::NORMAL, t.muted_foreground),
                    Node { margin: UiRect::bottom(Val::Px(6.0)), ..default() },
                ));
                for (k, (e, name, count)) in rows.into_iter().enumerate() {
                    let mut row = b.spawn((
                        TreeItem::new(format!("where-used-row-{}", k + 1), name.clone()).icon("assembly", 15.0).height(26.0).left(4.0).build(t),
                        UseRow(e),
                        Tooltip::new(format!("Open {name}")),
                    ));
                    row.with_child((t.text(count, 11.0, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE, Node { margin: UiRect::right(Val::Px(6.0)), ..default() }));
                    row.observe(|click: On<Pointer<Click>>, q: Query<&UseRow>, mut commands: Commands| {
                        if let Ok(r) = q.get(click.entity).copied() {
                            commands.queue(move |world: &mut World| {
                                close(world);
                                world.resource_mut::<ActiveDocument>().set_active(r.0);
                            });
                        }
                    });
                }
                b.spawn((
                    t.text(if others.is_empty() { "No references to it at a version".to_string() } else { "References at a version (this and other documents):".to_string() }, 11.5, FontWeight::NORMAL, t.muted_foreground),
                    Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
                ));
                for (k, o) in others.into_iter().enumerate() {
                    b.spawn(TreeItem::new(format!("where-used-other-{}", k + 1), o).icon("link", 14.0).height(24.0).left(4.0).build(t));
                }
            })
            .build(&t),
    );
    let mut qa = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = qa.iter(world).next() else { return };
    let d = world.spawn(bundle).id();
    world.entity_mut(area).add_child(d);
    world.insert_resource(WhereUsed);
}

fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<WhereUsedDialog>>();
    let es: Vec<Entity> = q.iter(world).collect();
    for e in es {
        world.entity_mut(e).despawn();
    }
    world.remove_resource::<WhereUsed>();
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<WhereUsedDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(close);
    }
}
