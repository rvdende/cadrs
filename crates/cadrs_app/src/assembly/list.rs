//! The Instances list (A1.5, A1.6, A3.7, A4.1, A4.4, A16.2, A16.3, A17, A18; `ex1-step9.png`,
//! `ex3-step4.png`…`ex3-step12.png`, `ex3-drawing.png`): under the root row and the Origin, one
//! row per instance, `Motor Mount <1>`, with the part icon and, after the name, the instance's
//! state: a **fixed** icon (hatched ground lines) or a small **triad** (it has degrees of
//! freedom). The **eye** at the right end shows on hover (always, crossed out, on a hidden
//! instance, whose row is greyed) and hides or shows it. A row selects its instance (the
//! selection the view shares); right-click opens the instance menu ([`super::menu`]). The
//! header counts the instances.
//!
//! P3B.4:
//! - A **subassembly** row has the assembly icon (bracketed when rigid) and a ▸ that opens it:
//!   its own instances follow, indented (drag one out to the top level, A17.4), then its own
//!   Items and Mate Features groups, read-only. Its **lock** (shown on hover, and always, open,
//!   when flexible) makes it rigid or flexible (A16.2).
//! - **Folders** (A18, the P3.9 folder model): a folder row "Hardware (6)" with a ▸, the folder
//!   icon and an eye, at its first instance; open, its instances follow, indented; an empty
//!   folder is last. See [`super::folders`] for its menu, the New folder button, Add selection to
//!   folder… and dragging rows.
//! - The **root row**'s icon after the name shows how the assembly is held (A16.3): when an
//!   instance is fixed, or fastened to the Origin by a mate (P3B.7), with a tooltip saying which.
//! - P3B.5: a **standard content** instance has the standard content icon (A19.8).
//! - **Suppressed** instances are greyed and struck through.
//! - The **filter** field (A1.5) takes the Part Studio feature list's query language
//!   ([`cadrs_core::feature_list::Filter`]): the rows that match, and their folders.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use cadrs_core::FeatureId;
use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::commands::SetInstancesHidden;
use cadrs_core::assembly::folders::{FolderList, item};
use cadrs_core::assembly::structure::derive;
use cadrs_ui::prelude::*;
use cadrs_ui::{StateColors, Visuals};

use super::folders::{AsmFolderRow, InAsmFolder, ListUi};
use crate::parts::PartCache;
use crate::viewport::{Pick, PickRow};
use crate::{ActiveDocument, AppState};

pub struct InstanceListPlugin;

impl Plugin for InstanceListPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (rebuild_instance_rows, sync_eyes, sync_root_state)
                .chain()
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .init_resource::<InstanceConnectorHover>()
        .add_systems(Update, (sync_connector_hover, list_width).run_if(in_state(AppState::Document)))
        .add_observer(on_row_menu)
        .add_observer(on_replicate_menu)
        .add_observer(on_eye_click)
        .add_observer(on_lock_click)
        .add_observer(on_sub_toggle);
    }
}

/// The container of the instance rows (spawned by the document shell's instance list).
#[derive(Component)]
pub struct InstanceRows;

/// The "Instances (n)" header text.
#[derive(Component)]
pub struct InstanceCount;

/// The top-level assembly row.
#[derive(Component)]
pub struct RootRow;

/// The "Insert parts to start (I)" hint, shown while the assembly is empty.
#[derive(Component)]
pub struct EmptyHint;

/// An instance row.
#[derive(Component, Debug, Clone, Copy)]
pub struct InstanceRow(pub InstanceId);

/// A row of an open subassembly: its instance `id` (in the subassembly's tab) under `sub`.
#[derive(Component, Debug, Clone, Copy)]
pub struct ChildRow {
    pub sub: InstanceId,
    pub id: InstanceId,
}

/// An instance row's eye (or a folder's: every instance in it).
#[derive(Component, Debug, Clone)]
struct RowEye {
    row: Entity,
    instances: Vec<InstanceId>,
    hidden: bool,
}

/// A subassembly row's lock (rigid / flexible, A16.2).
#[derive(Component, Debug, Clone, Copy)]
struct RowLock {
    row: Entity,
    instance: InstanceId,
    flexible: bool,
}

/// What a row shows.
#[derive(Debug, Clone, PartialEq)]
enum RowSpec {
    Instance {
        id: InstanceId,
        name: String,
        hidden: bool,
        fixed: bool,
        dof: u32,
        suppressed: bool,
        /// A subassembly: open, flexible.
        sub: Option<(bool, bool)>,
        folder: Option<FeatureId>,
        /// A standard content instance (A19.8).
        standard: bool,
        /// A rigid Part Studio instance (P3B.8, A2.4): open.
        studio: Option<bool>,
        /// A part instance whose part carries Part Studio mate connectors (P3B.7 judge): open.
        connectors: Option<bool>,
        /// Listed under a Replicate row (P3B.8).
        replicated: bool,
        /// The Named position of its tab a subassembly follows (A16.2).
        following: Option<String>,
        /// P3G.1 (ER1.9, DV1.7): a linked instance's icon tooltip, and its state (P3G.2: out of
        /// date, pinned, unreachable).
        linked: Option<(String, crate::linked::LinkIcon)>,
    },
    /// A part of an open rigid Part Studio instance.
    StudioPart {
        top: InstanceId,
        part: cadrs_core::PartId,
        name: String,
        /// Unticked in the open Edit dialog (P3B.8 judge): muted.
        out: bool,
    },
    /// A Part Studio mate connector of an open instance.
    Connector {
        instance: InstanceId,
        feature: FeatureId,
        name: String,
    },
    /// A Replicate feature's copies (P3B.8, X16; "Replicate 1" in `ex4-step11.png`).
    Replicate {
        id: cadrs_core::assembly::mate::MateId,
        name: String,
        count: usize,
        open: bool,
        folder: Option<FeatureId>,
    },
    Child {
        sub: InstanceId,
        id: InstanceId,
        name: String,
        fixed: bool,
        sub_child: bool,
    },
    /// An open subassembly's Items or Mate Features group (read-only).
    SubGroup {
        sub: InstanceId,
        label: String,
    },
    Folder {
        id: FeatureId,
        name: String,
        count: usize,
        open: bool,
        hidden: bool,
        instances: Vec<InstanceId>,
    },
}

/// What the rows were built from.
type RowsKey = (Vec<RowSpec>, usize);

/// Grey of a hidden instance's name and icons.
const HIDDEN_FG: Color = Color::srgb(0.65, 0.65, 0.65);

/// The rows of the list, in order (with the filter applied).
fn rows(doc: &ActiveDocument, cache: &PartCache, dofs: &super::mates_list::InstanceDofs, ui: &ListUi, editing: Option<&(InstanceId, Vec<cadrs_core::PartId>)>, status: &crate::linked::LinkStatus) -> Option<Vec<RowSpec>> {
    let model = doc.active_element()?.assembly_model()?;
    let occ = cadrs_core::assembly::structure::occurrences(&doc.doc, model);
    let name_of = |i: &cadrs_core::assembly::Instance| -> String {
        if i.source.is_composite() {
            let base = cadrs_core::assembly::source_part_name(&doc.doc, &i.source, None);
            return i.name(&base);
        }
        cache
            .part_name(i.id.part_id())
            .map(str::to_string)
            .or_else(|| {
                // Suppressed, or not in the view yet (still rebuilding): the source's name as the
                // document has it (waiting for the rebuild here froze the app; the rows are made
                // again when the parts come).
                Some(i.name(&cadrs_core::assembly::source_part_name(&doc.doc, &i.source, None)))
            })
            .unwrap_or_else(|| format!("Missing part <{}>", i.index))
    };
    let dof_of = |i: &cadrs_core::assembly::Instance| -> u32 {
        if i.fixed {
            return 0;
        }
        if i.source.is_composite() {
            // A rigid subassembly moves as its first part does.
            return occ.iter().find(|o| o.top == i.id).and_then(|o| dofs.dofs.get(&o.id).copied()).unwrap_or(if i.flexible { 0 } else { 6 });
        }
        dofs.dofs.get(&i.id).copied().unwrap_or(6)
    };
    let filter = ui.filter.as_ref();
    let folder_name = |f: Option<&cadrs_core::document::FeatureFolder>| f.map(|f| f.name.clone());
    let matches = |i: &cadrs_core::assembly::Instance, name: &str| -> bool {
        let Some(f) = filter else { return true };
        let folder = folder_name(cadrs_core::assembly::folders::folder_of(model, FolderList::Instances, item(i.id)));
        let facts = cadrs_core::feature_list::FeatureFacts {
            name,
            type_label: if i.source.is_assembly() {
                "Assembly"
            } else if i.source.is_studio() {
                "Part Studio"
            } else {
                "Part"
            },
            folder: folder.as_deref(),
            ..Default::default()
        };
        f.matches(&facts)
    };
    // The Part Studio mate connectors a part instance carries: (feature, name).
    let connectors_of = |i: &cadrs_core::assembly::Instance| -> Vec<(FeatureId, String)> {
        let Some(p) = cache.part(i.id.part_id()) else { return Vec::new() };
        let el = doc.doc.element(i.source.element());
        p.solid
            .connectors
            .iter()
            .map(|c| (c.feature, el.and_then(|e| e.feature(c.feature)).map(|f| f.name.clone()).unwrap_or_else(|| "Mate connector".into())))
            .collect()
    };
    let mut out = Vec::new();
    let mut folders_done: Vec<FeatureId> = Vec::new();
    let mut replicates_done: Vec<cadrs_core::assembly::mate::MateId> = Vec::new();
    let spec = |i: &cadrs_core::assembly::Instance, folder: Option<FeatureId>| RowSpec::Instance {
        id: i.id,
        name: name_of(i),
        hidden: i.hidden,
        fixed: i.fixed,
        dof: dof_of(i),
        suppressed: i.suppressed,
        sub: i.source.is_assembly().then(|| (ui.expanded.contains(&i.id), i.flexible)),
        folder,
        standard: cadrs_core::assembly::standard::standard_of(&doc.doc, &i.source).is_some(),
        studio: i.source.is_studio().then(|| ui.expanded.contains(&i.id)),
        connectors: (!i.source.is_composite() && !connectors_of(i).is_empty()).then(|| ui.expanded.contains(&i.id)),
        replicated: i.replicate.is_some_and(|r| model.mate(r).is_some()),
        following: i
            .follow
            .filter(|_| !i.flexible)
            .and_then(|f| doc.doc.element(i.source.element())?.assembly_model()?.named_position(f).map(|p| p.name.clone())),
        linked: i.link.map(|r| crate::linked::instance_badge(&doc.doc, status, &r, doc.active_element().map(|e| e.id).unwrap_or_default(), i)),
    };
    let push_children = |out: &mut Vec<RowSpec>, i: &cadrs_core::assembly::Instance| {
        if !ui.expanded.contains(&i.id) || filter.is_some() {
            return;
        }
        // P3B.8: a rigid Part Studio instance's parts.
        if i.source.is_studio() {
            for part in &i.parts {
                // The source part's name (also while the Edit dialog hides it from the view).
                let el = i.source.element();
                let name = cadrs_core::assembly::source_part_name(&doc.doc, &cadrs_core::assembly::InstanceSource::Part { element: el, part: *part }, None);
                let out_now = editing.is_some_and(|(id, ticked)| *id == i.id && !ticked.contains(part));
                out.push(RowSpec::StudioPart { top: i.id, part: *part, name, out: out_now });
            }
            return;
        }
        // P3B.7 judge: a part's Part Studio mate connectors.
        if !i.source.is_assembly() {
            for (feature, name) in connectors_of(i) {
                out.push(RowSpec::Connector { instance: i.id, feature, name });
            }
            return;
        }
        let Some(child) = doc.doc.element(i.source.element()).and_then(|e| e.assembly_model()) else { return };
        for c in &child.instances {
            let name = if c.source.is_assembly() {
                c.name(&cadrs_core::assembly::source_part_name(&doc.doc, &c.source, None))
            } else {
                cache.part_name(super::occurrence_part(derive(i.id, c.id))).map(str::to_string).unwrap_or_else(|| format!("Part <{}>", c.index))
            };
            out.push(RowSpec::Child { sub: i.id, id: c.id, name, fixed: c.fixed, sub_child: c.source.is_assembly() });
        }
        // Its own Items and Mate Features, closed and read-only (`ex3-step11.png`).
        out.push(RowSpec::SubGroup { sub: i.id, label: format!("Items ({})", child.items.len()) });
        out.push(RowSpec::SubGroup { sub: i.id, label: format!("Mate Features ({})", child.mates.len()) });
    };
    for inst in &model.instances {
        let folder = cadrs_core::assembly::folders::folder_of(model, FolderList::Instances, item(inst.id)).cloned();
        match folder {
            Some(f) => {
                if folders_done.contains(&f.id) {
                    continue;
                }
                folders_done.push(f.id);
                let members: Vec<&cadrs_core::assembly::Instance> = f.features.iter().filter_map(|x| model.instance(InstanceId(x.0))).collect();
                // A folder whose name matches shows all it holds.
                let folder_hit = filter.is_some_and(|q| q.matches(&cadrs_core::feature_list::FeatureFacts { name: &f.name, type_label: "Folder", ..Default::default() }));
                let shown: Vec<&&cadrs_core::assembly::Instance> = members.iter().filter(|m| folder_hit || matches(m, &name_of(m))).collect();
                if filter.is_some() && shown.is_empty() {
                    continue;
                }
                out.push(RowSpec::Folder {
                    id: f.id,
                    name: f.name.clone(),
                    count: f.features.len(),
                    open: f.open || filter.is_some(),
                    hidden: !members.is_empty() && members.iter().all(|m| m.hidden),
                    instances: members.iter().map(|m| m.id).collect(),
                });
                if f.open || filter.is_some() {
                    for m in shown {
                        out.push(spec(m, Some(f.id)));
                        push_children(&mut out, m);
                    }
                }
            }
            None => {
                // P3B.8: a Replicate's copies under its row, at the first copy.
                if let Some(r) = inst.replicate.filter(|r| model.mate(*r).is_some()) {
                    if replicates_done.contains(&r) {
                        continue;
                    }
                    replicates_done.push(r);
                    let copies: Vec<&cadrs_core::assembly::Instance> = model.instances.iter().filter(|i| i.replicate == Some(r)).collect();
                    let shown: Vec<&&cadrs_core::assembly::Instance> = copies.iter().filter(|c| matches(c, &name_of(c))).collect();
                    let name = model.mate(r).map(|f| f.name.clone()).unwrap_or_default();
                    if filter.is_some() && shown.is_empty() {
                        continue;
                    }
                    let key = InstanceId(r.0);
                    let open = ui.expanded.contains(&key) || filter.is_some();
                    out.push(RowSpec::Replicate { id: r, name, count: copies.len(), open, folder: None });
                    if open {
                        for c in shown {
                            out.push(spec(c, None));
                        }
                    }
                    continue;
                }
                if matches(inst, &name_of(inst)) {
                    out.push(spec(inst, None));
                    push_children(&mut out, inst);
                }
            }
        }
    }
    // Empty folders, last.
    if filter.is_none() {
        for f in model.folders.iter().filter(|f| f.features.is_empty()) {
            out.push(RowSpec::Folder { id: f.id, name: f.name.clone(), count: 0, open: f.open, hidden: false, instances: Vec::new() });
        }
    }
    Some(out)
}

#[allow(clippy::too_many_arguments)]
fn rebuild_instance_rows(
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    q_rows: Query<(Entity, Ref<InstanceRows>)>,
    mut q_count: Query<&mut Text, With<InstanceCount>>,
    mut q_hint: Query<&mut Node, With<EmptyHint>>,
    theme: Res<Theme>,
    dofs: Res<super::mates_list::InstanceDofs>,
    ui: Res<ListUi>,
    asm_parts: Res<super::AssemblyParts>,
    status: Res<crate::linked::LinkStatus>,
    mut last: Local<Option<RowsKey>>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else {
        return;
    };
    let Some((container, added)) = q_rows.iter().next().map(|(e, r)| (e, r.is_added())) else {
        return;
    };
    let Some(specs) = rows(&doc, &cache, &dofs, &ui, asm_parts.studio_preview.as_ref(), &status) else { return };
    let n = model.instances.len();
    let count = format!("Instances ({n})");
    for mut t in &mut q_count {
        if t.0 != count {
            t.0 = count.clone();
        }
    }
    for mut node in &mut q_hint {
        let want = if n == 0 && model.folders.is_empty() { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    let key: RowsKey = (specs, n);
    if !added && last.as_ref() == Some(&key) {
        return;
    }
    let t = theme.clone();
    commands.entity(container).despawn_children();
    let mut k = 0;
    commands.entity(container).with_children(|c| {
        for r in &key.0 {
            match r {
                RowSpec::Folder { id, name, count, open, hidden, instances } => {
                    spawn_folder_row(c, &t, *id, name, *count, *open, *hidden, instances);
                }
                RowSpec::Instance { id, name, hidden, fixed, dof, suppressed, sub, folder, standard, studio, connectors, replicated, following, linked } => {
                    k += 1;
                    let row = InstanceRowSpec {
                        k,
                        id: *id,
                        name,
                        hidden: *hidden,
                        fixed: *fixed,
                        dof: *dof,
                        suppressed: *suppressed,
                        sub: *sub,
                        folder: *folder,
                        standard: *standard,
                        studio: *studio,
                        connectors: *connectors,
                        replicated: *replicated,
                        following: following.as_deref(),
                        linked: linked.as_ref().map(|(tip, icon)| (tip.as_str(), *icon)),
                    };
                    spawn_instance_row(c, &t, &row);
                }
                RowSpec::StudioPart { top, part, name, out } => {
                    spawn_studio_part_row(c, &t, *top, *part, name, *out);
                }
                RowSpec::Connector { instance, feature, name } => {
                    spawn_connector_row(c, &t, *instance, *feature, name);
                }
                RowSpec::Replicate { id, name, count, open, folder } => {
                    spawn_replicate_row(c, &t, *id, name, *count, *open, *folder);
                }
                RowSpec::Child { sub, id, name, fixed, sub_child } => {
                    spawn_child_row(c, &t, *sub, *id, name, *fixed, *sub_child);
                }
                RowSpec::SubGroup { sub, label } => {
                    let slug = super::insert::slug(label.split(" (").next().unwrap_or(label));
                    c.spawn((
                        TreeItem::new(format!("instance-sub-{slug}-{}", sub.0.as_u128() as u32), label.clone()).disclosure(Some(false)).left(20.0).build(&t),
                        Tooltip::new("Open the subassembly's tab to edit it"),
                    ))
                    .insert(row_visuals(&t, t.foreground))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.height = Val::Px(22.0);
                        n.margin = UiRect::new(Val::Px(2.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0));
                    });
                }
            }
        }
    });
    *last = Some(key);
}

fn row_visuals(t: &Theme, fg: Color) -> Visuals {
    Visuals {
        background: StateColors::new(Color::NONE, t.list_hover, t.list_active, Color::NONE).with_selected(t.list_selected),
        border: StateColors::all(Color::NONE),
        foreground: StateColors::all(fg),
        focus_ring: t.focus_ring,
    }
}

fn eye_bundle(name: String, row: Entity, instances: Vec<InstanceId>, hidden: bool) -> impl Bundle {
    (
        Name::new(name),
        RowEye { row, instances, hidden },
        Node {
            width: Val::Px(20.0),
            height: Val::Px(20.0),
            // Over the row's right end (shown on hover): it takes no room from the name.
            position_type: PositionType::Absolute,
            right: Val::Px(2.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        },
        Visibility::Hidden,
        Tooltip::new(if hidden { "Show" } else { "Hide" }),
        children![(cadrs_ui::icon::icon(if hidden { "hidden" } else { "visible" }, 16.0, Color::srgb_u8(0x55, 0x55, 0x55)), Pickable::IGNORE,)],
    )
}

#[allow(clippy::too_many_arguments)]
fn spawn_folder_row(c: &mut ChildSpawnerCommands, t: &Theme, id: FeatureId, name: &str, count: usize, open: bool, hidden: bool, instances: &[InstanceId]) {
    let slug = super::insert::slug(name);
    let row_name = format!("instance-folder-{slug}");
    let fg = if hidden { HIDDEN_FG } else { t.foreground };
    let mut row = c.spawn((
        TreeItem::new(row_name.clone(), format!("{name} ({count})"))
            .disclosure(Some(open))
            .icon("folder", 16.0)
            .icon_color(if hidden { HIDDEN_FG } else { t.muted_foreground })
            .left(4.0)
            .editable()
            .build(t),
        AsmFolderRow { list: FolderList::Instances, id },
        ContextMenuTarget,
        cadrs_ui::DoubleClickable,
    ));
    row.insert(row_visuals(t, fg)).entry::<Node>().and_modify(|mut n| {
        n.height = Val::Px(22.0);
        n.margin = UiRect::new(Val::Px(2.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0));
    });
    let row_e = row.id();
    row.with_children(|r| {
        r.spawn(eye_bundle(format!("{row_name}-eye"), row_e, instances.to_vec(), hidden));
    });
}

/// What an instance row shows.
struct InstanceRowSpec<'a> {
    k: usize,
    id: InstanceId,
    name: &'a str,
    hidden: bool,
    fixed: bool,
    dof: u32,
    suppressed: bool,
    sub: Option<(bool, bool)>,
    folder: Option<FeatureId>,
    standard: bool,
    studio: Option<bool>,
    connectors: Option<bool>,
    replicated: bool,
    following: Option<&'a str>,
    linked: Option<(&'a str, crate::linked::LinkIcon)>,
}

fn spawn_instance_row(c: &mut ChildSpawnerCommands, t: &Theme, spec: &InstanceRowSpec) {
    let InstanceRowSpec { k, id, name, hidden, fixed, dof, suppressed, sub, folder, standard, studio, connectors, replicated, following, linked } = *spec;
    let fg = if hidden || suppressed { HIDDEN_FG } else { t.foreground };
    let row_name = format!("instance-row-{k}");
    // The part name shrinks ("Structural R…"), the number stays (P3B.1 judge).
    let (part, number) = match name.rsplit_once(" <") {
        Some((p, n)) => (p.to_string(), format!("<{n}")),
        None => (name.to_string(), String::new()),
    };
    let left = if folder.is_some() || replicated { 38.0 } else { 22.0 };
    // Rows with a ▸: a subassembly, a rigid Part Studio, a part carrying mate connectors.
    let open = sub.map(|(o, _)| o).or(studio).or(connectors);
    let mut item = TreeItem::new(row_name.clone(), part)
        // A rigid subassembly has the bracketed assembly icon (`ex3-step11.png`).
        .icon(
            match sub {
                Some((_, false)) => "assembly-rigid",
                Some((_, true)) => "assembly",
                None if studio.is_some() => "part-studio",
                None if standard => "standard-content",
                None => "part",
            },
            16.0,
        )
        .icon_color(if hidden || suppressed { HIDDEN_FG } else { t.muted_foreground })
        .strikethrough(suppressed)
        .left(if open.is_some() { left - 16.0 } else { left });
    if open.is_some() {
        item = item.disclosure(open);
    }
    let tip = if studio.is_some() { format!("{name}\nRigid Part Studio instance") } else { name.to_string() };
    let mut row = c.spawn((item.build(t), PickRow(Pick::Part(id.part_id())), InstanceRow(id), ContextMenuTarget, Tooltip::new(tip)));
    if let Some(f) = folder {
        row.insert(InAsmFolder(f));
    }
    let is_linked = linked.is_some();
    row.insert(row_visuals(t, fg)).entry::<Node>().and_modify(move |mut n| {
        n.height = Val::Px(22.0);
        n.margin = UiRect::new(Val::Px(2.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0));
        // P3G.4 (P3G.3 carried, a name cut beside the link icon): a linked row's gaps are
        // tighter and its eye's slot narrower, so the name keeps that room.
        n.column_gap = Val::Px(if is_linked { 3.0 } else { 5.0 });
        // The eye's slot is kept free, so the name and its <n> never run under it (P3B.4
        // judge); a subassembly's lock shows beside it on hover, over the row's own icons, so the
        // name keeps that room (P3B.8 judge: "Step Stool… <1>" with room to spare).
        n.padding.right = Val::Px(if is_linked { 20.0 } else { 22.0 });
        let _ = sub;
    });
    let row_e = row.id();
    // Fixed (hatched ground), free (a small triad: it has degrees of freedom, A16.1), or
    // nothing when the mates hold it.
    let state = if fixed {
        Some(("constraint-fix", "fixed", "Fixed".to_string()))
    } else if dof > 0 && !suppressed {
        let tip = if dof == 1 { "This instance has 1 degree of freedom".to_string() } else { format!("This instance has {dof} degrees of freedom") };
        Some(("instance-dof", "dof", tip))
    } else {
        None
    };
    row.with_children(|r| {
        if !number.is_empty() {
            r.spawn((
                Name::new(format!("{row_name}-number")),
                t.text(number.clone(), t.font_sm, bevy::text::FontWeight::NORMAL, fg),
                Node { flex_shrink: 0.0, ..default() },
                cadrs_ui::InheritFg,
                Pickable::IGNORE,
            ));
        }
        // A16.2: it follows a Named position of its tab.
        if let Some(pos) = following {
            r.spawn((
                Name::new(format!("{row_name}-following")),
                cadrs_ui::icon::icon_in("named-positions", 14.0, Color::srgb_u8(0x3c, 0x3c, 0x3c), Node { flex_shrink: 0.0, ..default() }),
                Tooltip::new(format!("Follows the named position \"{pos}\"")),
            ));
        }
        if let Some((state_icon, suffix, tip)) = state {
            r.spawn((
                Name::new(format!("{row_name}-{suffix}")),
                cadrs_ui::icon::icon_in(state_icon, 15.0, if hidden { HIDDEN_FG } else { Color::srgb_u8(0x3c, 0x3c, 0x3c) }, Node { flex_shrink: 0.0, ..default() }),
                Tooltip::new(tip),
            ));
        }
        // P3G.1 (ER1.9, `ex1-step5.png`): a linked instance's version icon, right-aligned at the
        // row's end before the eye's slot (red while its source can't be reached, DV1.7; P3G.2:
        // the blue badge, the transitive arrow, the pin; a click opens the Reference manager).
        if let Some((tip, icon)) = linked {
            crate::linked::spawn_link_icon(r, format!("{row_name}-linked"), icon, tip, crate::linked::LinkTarget::Instance(id));
        }
        // A16.2: the lock of a subassembly: rigid (locked) or flexible.
        if let Some((_, flexible)) = sub {
            r.spawn((
                Name::new(format!("{row_name}-lock")),
                RowLock { row: row_e, instance: id, flexible },
                Node {
                    width: Val::Px(18.0),
                    height: Val::Px(20.0),
                    position_type: PositionType::Absolute,
                    // Left of a linked subassembly's version icon (P3G.2), else over the icons.
                    right: Val::Px(if linked.is_some() { 38.0 } else { 22.0 }),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                // Over the row's own icons while hovered: on the hover colour.
                BackgroundColor(t.list_hover),
                Visibility::Hidden,
                Tooltip::new(if flexible { "Flexible: its mates act here. Click to make it rigid" } else { "Rigid. Click to make it flexible" }),
                children![(
                    cadrs_ui::icon::icon(if flexible { "lock-open" } else { "lock-filled" }, 14.0, Color::srgb_u8(0x55, 0x55, 0x55)),
                    Pickable::IGNORE,
                )],
            ));
        }
        r.spawn(eye_bundle(format!("{row_name}-eye"), row_e, vec![id], hidden));
    });
}

/// A part of an open rigid Part Studio instance (read-only; picks the part).
fn spawn_studio_part_row(c: &mut ChildSpawnerCommands, t: &Theme, top: InstanceId, part: cadrs_core::PartId, name: &str, out: bool) {
    let (label, _) = name.rsplit_once(" <").unwrap_or((name, ""));
    let slug = super::insert::slug(label);
    let o = derive(top, cadrs_core::assembly::structure::studio_part_key(part));
    let fg = if out { HIDDEN_FG } else { t.foreground };
    let mut row = c.spawn((
        TreeItem::new(format!("instance-studio-part-{slug}"), label.to_string())
            .icon("part", 15.0)
            .icon_color(if out { HIDDEN_FG } else { t.muted_foreground })
            .muted(out)
            .left(36.0)
            .build(t),
        PickRow(Pick::Part(super::occurrence_part(o))),
        Tooltip::new(if out { format!("{label}: unticked in Edit (taken out on ✓)") } else { format!("{label} (in the rigid Part Studio instance)") }),
    ));
    row.insert(row_visuals(t, fg)).entry::<Node>().and_modify(|mut n| {
        n.height = Val::Px(22.0);
        n.margin = UiRect::new(Val::Px(2.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0));
    });
}

/// A Part Studio mate connector of an open part instance (`ex4-step11.png`): hovering it
/// highlights it in the view.
fn spawn_connector_row(c: &mut ChildSpawnerCommands, t: &Theme, instance: InstanceId, feature: FeatureId, name: &str) {
    let slug = super::insert::slug(name);
    let mut row = c.spawn((
        TreeItem::new(format!("instance-connector-{slug}"), name.to_string())
            .icon("mate-connector", 14.0)
            .icon_color(Color::srgb_u8(0x8a, 0x8f, 0x94))
            .left(36.0)
            .muted(true)
            .build(t),
        InstanceConnectorRow { instance, feature },
        Tooltip::new(name.to_string()),
    ));
    row.insert(row_visuals(t, Color::srgb_u8(0x70, 0x75, 0x7a))).entry::<Node>().and_modify(|mut n| {
        n.height = Val::Px(22.0);
        n.margin = UiRect::new(Val::Px(2.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0));
    });
}

/// A Replicate feature's row in the Instances list: ▸, the replicate icon, "Replicate 1".
fn spawn_replicate_row(c: &mut ChildSpawnerCommands, t: &Theme, id: cadrs_core::assembly::mate::MateId, name: &str, count: usize, open: bool, folder: Option<FeatureId>) {
    let slug = super::insert::slug(name);
    let mut row = c.spawn((
        TreeItem::new(format!("instance-replicate-{slug}"), name.to_string())
            .disclosure(Some(open))
            .icon("replicate", 16.0)
            .icon_color(t.muted_foreground)
            .left(if folder.is_some() { 22.0 } else { 6.0 })
            .build(t),
        ReplicateRow(id),
        ContextMenuTarget,
        Tooltip::new(format!("{name}: {count} instances")),
    ));
    row.insert(row_visuals(t, t.foreground)).entry::<Node>().and_modify(|mut n| {
        n.height = Val::Px(22.0);
        n.margin = UiRect::new(Val::Px(2.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0));
    });
}

/// The list panel of an assembly is wider than a Part Studio's feature list, so instance names
/// ("Base Frame Bar <1>") fit beside their state icons (P3B.8 judge).
fn list_width(kind: Res<crate::viewport::ActiveKind>, mut q: Query<(&Name, &mut cadrs_ui::dock::DockPanelState)>) {
    let want = match *kind {
        crate::viewport::ActiveKind::Assembly => ASSEMBLY_LIST_W,
        // P3H.3: the PCB Studio's Boards/Components panel.
        crate::viewport::ActiveKind::PcbStudio => crate::pcb::PANEL_W,
        crate::viewport::ActiveKind::Render => crate::render_ui::PANEL_W,
        _ => 190.0,
    };
    for (n, mut st) in &mut q {
        if n.as_str() == "feature-panel" && st.width() != want {
            st.set_width(want);
        }
    }
}

/// The list panel's width in an assembly (px).
pub const ASSEMBLY_LIST_W: f32 = 240.0;

/// A Part Studio mate connector's row under its instance.
#[derive(Component, Debug, Clone, Copy)]
pub struct InstanceConnectorRow {
    pub instance: InstanceId,
    pub feature: FeatureId,
}

/// A Replicate feature's row in the Instances list.
#[derive(Component, Debug, Clone, Copy)]
pub struct ReplicateRow(pub cadrs_core::assembly::mate::MateId);

/// The Part Studio connector row under the pointer (drawn highlighted, P3B.7 judge).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct InstanceConnectorHover(pub Option<(InstanceId, FeatureId)>);

fn sync_connector_hover(q: Query<(&InstanceConnectorRow, &Hovered)>, mut hover: ResMut<InstanceConnectorHover>) {
    let over = q.iter().find(|(_, h)| h.get()).map(|(r, _)| (r.instance, r.feature));
    if hover.0 != over {
        hover.0 = over;
    }
}

fn spawn_child_row(c: &mut ChildSpawnerCommands, t: &Theme, sub: InstanceId, id: InstanceId, name: &str, fixed: bool, sub_child: bool) {
    let (part, number) = match name.rsplit_once(" <") {
        Some((p, n)) => (p.to_string(), format!("<{n}")),
        None => (name.to_string(), String::new()),
    };
    let slug = super::insert::slug(&part);
    let row_name = format!("instance-child-{slug}-{}", number.trim_matches(|c| c == '<' || c == '>'));
    let part_pick = super::occurrence_part(derive(sub, id));
    let mut row = c.spawn((
        TreeItem::new(row_name.clone(), part).icon(if sub_child { "assembly" } else { "part" }, 15.0).icon_color(t.muted_foreground).left(36.0).build(t),
        PickRow(Pick::Part(part_pick)),
        ChildRow { sub, id },
        Tooltip::new(name.to_string()),
    ));
    // The standard row height (P3B.4 judge).
    row.insert(row_visuals(t, t.foreground)).entry::<Node>().and_modify(|mut n| {
        n.height = Val::Px(22.0);
        n.margin = UiRect::new(Val::Px(2.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0));
        n.column_gap = Val::Px(5.0);
    });
    row.with_children(|r| {
        if !number.is_empty() {
            r.spawn((
                Name::new(format!("{row_name}-number")),
                t.text(number.clone(), t.font_sm, bevy::text::FontWeight::NORMAL, t.foreground),
                Node { flex_shrink: 0.0, ..default() },
                Pickable::IGNORE,
            ));
        }
        if fixed {
            r.spawn(cadrs_ui::icon::icon_in("constraint-fix", 14.0, Color::srgb_u8(0x3c, 0x3c, 0x3c), Node { flex_shrink: 0.0, ..default() }));
        }
    });
}

/// The eye shows while its row is hovered, and always (crossed out) on a hidden instance; a
/// subassembly's lock while hovered, and always when flexible.
fn sync_eyes(mut q: Query<(&RowEye, &mut Visibility), Without<RowLock>>, mut q_lock: Query<(&RowLock, &mut Visibility), Without<RowEye>>, q_row: Query<&Hovered>) {
    for (eye, mut vis) in &mut q {
        let show = eye.hidden || q_row.get(eye.row).is_ok_and(|h| h.get());
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (lock, mut vis) in &mut q_lock {
        let show = lock.flexible || q_row.get(lock.row).is_ok_and(|h| h.get());
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// A16.3: the root row shows how the assembly is held: fastened to the origin when an instance
/// is fixed.
fn sync_root_state(
    doc: Option<Res<ActiveDocument>>,
    q_root: Query<Entity, With<RootRow>>,
    q_state: Query<Entity, With<RootState>>,
    mut last: Local<Option<(bool, bool)>>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    let fixed = cadrs_core::assembly::structure::root_fixed(model);
    // P3B.7: an instance fastened to the Origin by a mate.
    let fastened = !model.fastened_to_origin().is_empty();
    let state = (fixed, fastened);
    let want = fixed || fastened;
    let have = q_state.iter().next().is_some();
    if *last == Some(state) && have == want {
        return;
    }
    *last = Some(state);
    for e in &q_state {
        commands.entity(e).try_despawn();
    }
    if !want {
        return;
    }
    let Some(root) = q_root.iter().next() else { return };
    let (name, tip) = if fastened {
        ("instance-root-fastened", "Fastened to the origin by a mate")
    } else {
        ("instance-root-fixed", "Fastened to the origin: an instance of this assembly is fixed (not carried into a parent assembly)")
    };
    let icon = commands
        .spawn((
            Name::new(name),
            RootState,
            // How the base is held (`ex3-drawing.png`'s, `ex4-step9.png`'s root row).
            cadrs_ui::icon::icon_in("origin-fastened", 15.0, Color::srgb_u8(0x3c, 0x3c, 0x3c), Node { flex_shrink: 0.0, margin: UiRect::left(Val::Px(4.0)), ..default() }),
            Tooltip::new(tip),
        ))
        .id();
    commands.entity(root).add_child(icon);
}

/// The root row's state icon.
#[derive(Component)]
struct RootState;

fn on_eye_click(mut click: On<Pointer<Click>>, q: Query<&RowEye>, mut commands: Commands) {
    let Ok(eye) = q.get(click.entity) else { return };
    click.propagate(false);
    let (instances, hidden) = (eye.instances.clone(), !eye.hidden);
    if instances.is_empty() {
        return;
    }
    commands.queue(move |world: &mut World| {
        let Some(element) = world.get_resource::<ActiveDocument>().and_then(super::active_assembly) else {
            return;
        };
        super::run(world, &SetInstancesHidden { element, instances, hidden });
    });
}

fn on_lock_click(mut click: On<Pointer<Click>>, q: Query<&RowLock>, mut commands: Commands) {
    let Ok(lock) = q.get(click.entity).copied() else { return };
    click.propagate(false);
    commands.queue(move |world: &mut World| {
        let Some(element) = world.get_resource::<ActiveDocument>().and_then(super::active_assembly) else { return };
        super::run(world, &cadrs_core::assembly::structure::SetSubassemblyFlexible { element, instances: vec![lock.instance], flexible: !lock.flexible });
    });
}

/// A subassembly row's ▸ opens it (its instances, indented).
fn on_sub_toggle(ev: On<cadrs_ui::TreeToggle>, q: Query<&InstanceRow>, q_rep: Query<&ReplicateRow>, mut ui: ResMut<ListUi>) {
    let key = q.get(ev.entity).map(|r| r.0).ok().or_else(|| q_rep.get(ev.entity).ok().map(|r| InstanceId(r.0.0)));
    if let Some(k) = key
        && !ui.expanded.remove(&k)
    {
        ui.expanded.insert(k);
    }
}

/// A Replicate row's menu: the feature's own (Edit…, Delete, …).
fn on_replicate_menu(ev: On<ContextMenuRequested>, q: Query<&ReplicateRow>, mut commands: Commands) {
    let Ok(row) = q.get(ev.entity).copied() else { return };
    let (at, e) = (ev.position, ev.entity);
    commands.queue(move |world: &mut World| {
        let at = super::mates_list::beside_row(world, e, at);
        super::mates_list::open_mate_menu(world, at, row.0)
    });
}

fn on_row_menu(ev: On<ContextMenuRequested>, q: Query<&InstanceRow>, mut commands: Commands) {
    let Ok(row) = q.get(ev.entity) else { return };
    let (instance, at) = (row.0, ev.position);
    commands.queue(move |world: &mut World| {
        // Right-click selects the row's instance (unless it is in the selection already), as
        // Onshape does (`ex1-step9.png`): the triad and the readout of the last pick go.
        let row = Pick::Part(instance.part_id());
        if !super::selected_instances(world.resource::<crate::viewport::Selection>()).contains(&instance) {
            world.resource_mut::<crate::viewport::Selection>().0 = vec![row];
        }
        super::menu::open_instance_menu(world, at, instance, super::triad::TriadHandle::None);
    });
}
