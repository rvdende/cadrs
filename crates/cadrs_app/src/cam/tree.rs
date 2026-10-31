//! The CAM Studio's tree: Jobs (their machine, setups and operations) and Components, its
//! rows' clicks, menus and their actions, and Delete.

use super::*;
use cadrs_ui::i18n::{tr, tr_with};

#[derive(Component)]
pub struct CamTree;

/// The panel's host, spawned by the document shell for a CAM Studio tab.
pub fn panel(p: &mut ChildSpawnerCommands, _t: &Theme) {
    p.spawn((
        Name::new("cam-tree"),
        CamTree,
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(6.0), Val::Px(4.0)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    ));
}

/// What a tree row stands for.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CamRow {
    Component(CamId),
    Job(CamId),
    Machine(CamId),
    Setup(CamId),
    Operation(CamId),
}

impl CamRow {
    pub fn id(self) -> CamId {
        match self {
            CamRow::Component(i)
            | CamRow::Job(i)
            | CamRow::Machine(i)
            | CamRow::Setup(i)
            | CamRow::Operation(i) => i,
        }
    }
}

/// A row as the tree shows it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RowSnap {
    row: CamRow,
    depth: u32,
    label: String,
    icon: &'static str,
    open: Option<bool>,
    bold: bool,
    muted: bool,
    struck: bool,
    selected: bool,
    trailing: Option<String>,
    error: bool,
    /// What the row's hover says: a toolpath's warnings, or why it failed.
    tip: Option<String>,
    /// The toolpath has warnings (its time shows ⚠).
    warn: bool,
    eye: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct TreeSnap {
    tree: Option<Entity>,
    rows: Vec<RowSnap>,
    components: usize,
    jobs: usize,
    /// The interface's language (the tree is rebuilt in a new one).
    language: &'static str,
}

pub(super) fn rows_of(
    cam: &CamStudio,
    ui: &CamUi,
    computed: &Computed,
) -> (Vec<RowSnap>, Vec<RowSnap>) {
    let mut jobs = Vec::new();
    let active = cam.active().map(|j| j.id);
    for job in &cam.jobs {
        let open = !ui.collapsed.contains(&job.id);
        jobs.push(RowSnap {
            row: CamRow::Job(job.id),
            depth: 0,
            label: job.name.clone(),
            icon: "cam-job",
            open: Some(open),
            bold: Some(job.id) == active,
            muted: job.suppressed,
            struck: job.suppressed,
            selected: ui.selected.contains(&job.id),
            trailing: None,
            error: false,
            tip: None,
            warn: false,
            eye: None,
        });
        if !open {
            continue;
        }
        jobs.push(RowSnap {
            row: CamRow::Machine(job.id),
            depth: 1,
            label: job
                .machine
                .as_ref()
                // (The model alone for the catalogue's generic machines: the row is narrow.)
                .map(|m| {
                    if m.builder == "Generic" {
                        m.model.clone()
                    } else {
                        m.title()
                    }
                })
                .unwrap_or_else(|| tr("cam-no-machine-create-machine")),
            icon: job
                .machine
                .as_ref()
                .map_or("cam-machine", |m| m.kind.icon()),
            open: None,
            bold: false,
            muted: job.machine.is_none(),
            struck: false,
            selected: false,
            trailing: None,
            error: false,
            tip: None,
            warn: false,
            eye: None,
        });
        for setup in &job.setups {
            let sopen = !ui.collapsed.contains(&setup.id);
            jobs.push(RowSnap {
                row: CamRow::Setup(setup.id),
                depth: 1,
                label: format!("{} (O{})", setup.name, setup.program_number),
                icon: "cam-setup",
                open: Some(sopen),
                bold: false,
                muted: setup.suppressed,
                struck: setup.suppressed,
                selected: ui.selected.contains(&setup.id),
                trailing: None,
                error: false,
                tip: None,
                warn: false,
                eye: None,
            });
            if !sopen {
                continue;
            }
            for o in &setup.operations {
                let mut tip = None;
                let mut warn = false;
                let (label, trailing, error, eye) = match &o.kind {
                    OpKind::Tool(t) => (format!("T{} {}", t.number, o.name), None, false, None),
                    OpKind::Toolpath(_) => {
                        let st = computed.status(o.id);
                        let warnings = computed
                            .current(o.id)
                            .map(|d| d.toolpath.warnings.clone())
                            .unwrap_or_default();
                        tip = st
                            .problem()
                            .map(String::from)
                            .or_else(|| (!warnings.is_empty()).then(|| warnings.join("\n")));
                        warn = !warnings.is_empty() && st.problem().is_none();
                        let short = st.short();
                        (
                            o.name.clone(),
                            Some(if warn { format!("⚠ {short}") } else { short }),
                            st.problem().is_some(),
                            Some(!o.hidden),
                        )
                    }
                    OpKind::WorkPlane(_) => (o.name.clone(), None, false, Some(!o.hidden)),
                };
                jobs.push(RowSnap {
                    row: CamRow::Operation(o.id),
                    depth: 2,
                    label,
                    icon: o.icon(),
                    open: None,
                    bold: false,
                    muted: o.suppressed,
                    struck: o.suppressed,
                    selected: ui.selected.contains(&o.id),
                    trailing,
                    error,
                    tip,
                    warn,
                    eye,
                });
            }
        }
    }
    let comps = cam
        .components
        .iter()
        .map(|c| RowSnap {
            row: CamRow::Component(c.id),
            depth: 0,
            label: c.name.clone(),
            icon: c.role.icon(),
            open: None,
            bold: false,
            muted: c.hidden,
            struck: false,
            selected: ui.selected.contains(&c.id),
            trailing: Some(if c.derived.version_name.is_empty() {
                tr("cam-component-workspace")
            } else {
                c.derived.version_name.clone()
            }),
            error: false,
            tip: None,
            warn: false,
            eye: Some(!c.hidden),
        })
        .collect();
    (jobs, comps)
}

pub(super) fn rebuild_tree(
    doc: Option<Res<ActiveDocument>>,
    ui: Res<CamUi>,
    computed: Res<Computed>,
    q_tree: Query<Entity, With<CamTree>>,
    theme: Res<Theme>,
    mut last: Local<TreeSnap>,
    mut commands: Commands,
) {
    let Ok(tree) = q_tree.single() else {
        *last = TreeSnap::default();
        return;
    };
    let Some(doc) = doc else { return };
    let Some((_, cam)) = active_cam(&doc) else {
        return;
    };
    let (jobs, comps) = rows_of(cam, &ui, &computed);
    let snap = TreeSnap {
        tree: Some(tree),
        components: comps.len(),
        jobs: cam.jobs.len(),
        rows: jobs.iter().chain(comps.iter()).cloned().collect(),
        language: cadrs_ui::i18n::language(),
    };
    if *last == snap {
        return;
    }
    let t = theme.clone();
    commands.entity(tree).despawn_children();
    commands.entity(tree).with_children(|p| {
        header(
            p,
            &t,
            "cam-jobs-header",
            &tr_with("cam-jobs-count", &[("count", &cam.jobs.len().to_string())]),
        );
        if jobs.is_empty() {
            note(
                p,
                &t,
                &tr(if cam.components.is_empty() {
                    "cam-tree-insert-part-then-job"
                } else {
                    "cam-tree-create-job-for-parts"
                }),
            );
        }
        for r in &jobs {
            spawn_row(p, &t, r);
        }
        p.spawn((
            Name::new("cam-tree-gap"),
            Node {
                height: Val::Px(10.0),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        header(
            p,
            &t,
            "cam-components-header",
            &tr_with(
                "cam-components-count",
                &[("count", &comps.len().to_string())],
            ),
        );
        if comps.is_empty() {
            note(p, &t, &tr("cam-tree-insert-components-note"));
        }
        for r in &comps {
            spawn_row(p, &t, r);
        }
    });
    *last = snap;
}

pub(super) fn header(p: &mut ChildSpawnerCommands, t: &Theme, name: &str, text: &str) {
    p.spawn((
        Name::new(name.to_string()),
        t.text(text, 12.0, bevy::text::FontWeight::BOLD, t.foreground),
        Node {
            margin: UiRect::new(Val::Px(6.0), Val::Px(4.0), Val::Px(4.0), Val::Px(4.0)),
            ..default()
        },
    ));
}

pub(super) fn note(p: &mut ChildSpawnerCommands, t: &Theme, text: &str) {
    p.spawn((
        t.text(
            text,
            11.0,
            bevy::text::FontWeight::NORMAL,
            t.muted_foreground,
        ),
        Node {
            margin: UiRect::new(Val::Px(8.0), Val::Px(6.0), Val::Px(2.0), Val::Px(4.0)),
            ..default()
        },
    ));
}

/// A row's `Name`: its kind and id (`cam-op-1f2e…`).
pub fn row_name(r: CamRow) -> String {
    let (k, id) = match r {
        CamRow::Component(i) => ("comp", i),
        CamRow::Job(i) => ("job", i),
        CamRow::Machine(i) => ("machine", i),
        CamRow::Setup(i) => ("setup", i),
        CamRow::Operation(i) => ("op", i),
    };
    format!("cam-{k}-{:x}", id.0)
}

pub(super) fn spawn_row(p: &mut ChildSpawnerCommands, t: &Theme, r: &RowSnap) {
    let mut item = TreeItem::new(row_name(r.row), r.label.clone())
        .icon(r.icon, 16.0)
        .depth(r.depth)
        .disclosure(r.open)
        .selected(r.selected)
        .muted(r.muted)
        .strikethrough(r.struck)
        .middle_ellipsis();
    if r.bold {
        item = item.weight(bevy::text::FontWeight::BOLD);
    }
    if let Some(tr) = &r.trailing {
        let c = if r.error {
            t.danger
        } else if r.warn {
            t.warning_icon
        } else {
            t.muted_foreground
        };
        item = item.trailing(Some(tr.clone()), c);
    }
    if r.error {
        item = item.foreground(t.danger);
    }
    if let Some(on) = r.eye {
        item = item
            .toggle(format!("{}-eye", row_name(r.row)), "visible", "hidden", on)
            .toggle_tooltip(tr(if on { "common-hide" } else { "common-show" }));
        if !on {
            item = item.toggle_keeps_off();
        }
    }
    let mut e = p.spawn((item.build(t), r.row));
    if let Some(tip) = &r.tip {
        e.insert(cadrs_ui::Tooltip::new(tip.clone()));
    }
}

pub(super) fn on_row_activate(ev: On<Activate>, q: Query<&CamRow>, mut commands: Commands) {
    let Ok(&row) = q.get(ev.entity) else {
        return;
    };
    commands.queue(move |w: &mut World| click_row(w, row));
}

/// A click on a row: selects it (Ctrl adds to the selection); a job becomes the active one; a
/// second click soon after edits it.
pub(super) fn click_row(world: &mut World, row: CamRow) {
    let ctrl = world
        .get_resource::<ButtonInput<KeyCode>>()
        .is_some_and(|k| k.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]));
    let id = row.id();
    let now = Instant::now();
    let double = {
        let mut ui = world.resource_mut::<CamUi>();
        let double = ui
            .last_click
            .is_some_and(|(i, t)| i == id && now.duration_since(t).as_secs_f32() < 0.4);
        ui.last_click = Some((id, now));
        if ctrl {
            if let Some(i) = ui.selected.iter().position(|x| *x == id) {
                ui.selected.remove(i);
            } else {
                ui.selected.push(id);
            }
        } else {
            ui.selected = vec![id];
        }
        double
    };
    if let CamRow::Job(j) = row
        && active_cam_owned(world).is_some_and(|(_, c)| c.active().map(|x| x.id) != Some(j))
    {
        edit(world, Edit::SetActiveJob(j));
    }
    if double {
        dialogs::edit_row(world, row);
    }
}

pub(super) fn on_row_toggle(ev: On<TreeToggle>, q: Query<&CamRow>, mut ui: ResMut<CamUi>) {
    let Ok(row) = q.get(ev.entity) else { return };
    let id = row.id();
    if !ui.collapsed.remove(&id) {
        ui.collapsed.insert(id);
    }
}

pub(super) fn on_row_eye(ev: On<TreeRowToggled>, q: Query<&CamRow>, mut commands: Commands) {
    let Ok(&row) = q.get(ev.entity) else { return };
    commands.queue(move |w: &mut World| {
        let Some((_, cam)) = active_cam_owned(w) else {
            return;
        };
        let hidden = match row {
            CamRow::Component(id) => cam.component(id).map(|c| c.hidden),
            CamRow::Operation(id) => cam.operation(id).map(|(_, _, o)| o.hidden),
            _ => None,
        };
        if let Some(h) = hidden {
            edit(w, Edit::Hide(row.id(), !h));
        }
    });
}

#[derive(Component, Clone, Copy)]
pub(super) struct RowMenuFor(CamRow);

pub(super) fn on_row_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&CamRow>,
    doc: Option<Res<ActiveDocument>>,
    computed: Res<Computed>,
    theme: Res<Theme>,
    mut ui: ResMut<CamUi>,
    mut commands: Commands,
) {
    let Ok(&row) = q.get(ev.entity) else { return };
    let Some(doc) = doc else { return };
    let Some((_, cam)) = active_cam(&doc) else {
        return;
    };
    if !ui.selected.contains(&row.id()) {
        ui.selected = vec![row.id()];
    }
    let item = |id: &str, label: &str, icon: &str| {
        let it = MenuItem::new(id.to_string(), label.to_string());
        if icon.is_empty() {
            it
        } else {
            it.icon(icon.to_string())
        }
    };
    let mut menu = Menu::new("cam-row-menu").min_width(200.0);
    match row {
        CamRow::Component(id) => {
            let Some(c) = cam.component(id) else { return };
            menu = menu.item(item("cam-m-edit", &tr("common-edit-ellipsis"), "edit"));
            menu = menu.item(item(
                "cam-m-hide",
                &tr(if c.hidden {
                    "common-show"
                } else {
                    "common-hide"
                }),
                if c.hidden { "visible" } else { "hidden" },
            ));
            if !c.derived.version_name.is_empty() {
                menu = menu.item(item(
                    "cam-m-update",
                    &tr("cam-update-to-latest-version"),
                    "link",
                ));
            }
            menu = menu
                .separator()
                .item(item("cam-m-delete", &tr("common-delete"), "delete"));
        }
        CamRow::Job(id) => {
            let Some(j) = cam.job(id) else { return };
            menu = menu
                .item(item("cam-m-edit", &tr("cam-edit-job"), "edit"))
                .item(item(
                    "cam-m-machine",
                    &tr(if j.machine.is_some() {
                        "cam-edit-machine"
                    } else {
                        "cam-create-machine-ellipsis"
                    }),
                    "cam-machine",
                ));
            if j.machine.is_some() {
                menu = menu.item(item(
                    "cam-m-setup",
                    &tr("cam-create-setup-ellipsis"),
                    "cam-setup",
                ));
            }
            menu = menu
                .item(item(
                    "cam-m-regenerate",
                    &tr("cam-regenerate-all-toolpaths"),
                    "cam-regenerate",
                ))
                .separator()
                .item(item("cam-m-duplicate", &tr("cam-duplicate"), "copy"))
                .item(item(
                    "cam-m-suppress",
                    &tr(if j.suppressed {
                        "common-unsuppress"
                    } else {
                        "common-suppress"
                    }),
                    "",
                ))
                .item(item("cam-m-delete", &tr("common-delete"), "delete"));
        }
        CamRow::Machine(_) => {
            menu = menu.item(item(
                "cam-m-machine",
                &tr("cam-edit-machine"),
                "cam-machine",
            ));
        }
        CamRow::Setup(id) => {
            let Some((_, s)) = cam.setup(id) else { return };
            menu = menu
                .item(item("cam-m-edit", &tr("cam-edit-setup"), "edit"))
                .item(item(
                    "cam-m-tool",
                    &tr("cam-create-tool-ellipsis"),
                    "cam-tool",
                ))
                .item(item(
                    "cam-m-toolpath",
                    &tr("cam-create-toolpath-ellipsis"),
                    "cam-toolpath",
                ))
                .item(item(
                    "cam-m-workplane",
                    &tr("cam-create-work-plane-ellipsis"),
                    "cam-work-plane",
                ))
                .separator()
                .item(item(
                    "cam-m-regenerate",
                    &tr("cam-regenerate-all-toolpaths"),
                    "cam-regenerate",
                ))
                .item(item(
                    "cam-m-preview",
                    &tr("cam-preview-operation"),
                    "cam-preview",
                ))
                .item(item(
                    "cam-m-post",
                    &tr("cam-post-child-operations"),
                    "cam-post",
                ))
                .separator()
                .item(item("cam-m-duplicate", &tr("cam-duplicate"), "copy"))
                .item(item(
                    "cam-m-suppress",
                    &tr(if s.suppressed {
                        "common-unsuppress"
                    } else {
                        "common-suppress"
                    }),
                    "",
                ))
                .item(item("cam-m-delete", &tr("common-delete"), "delete"));
        }
        CamRow::Operation(id) => {
            let Some((_, setup, o)) = cam.operation(id) else {
                return;
            };
            menu = menu.item(item("cam-m-edit", &tr("common-edit-ellipsis"), "edit"));
            if o.toolpath().is_some() {
                let calc = computed.current(id).is_some();
                menu = menu
                    .item(item(
                        "cam-m-regenerate",
                        &tr("cam-regenerate-operation"),
                        "cam-regenerate",
                    ))
                    .item(
                        item("cam-m-preview", &tr("cam-preview-operation"), "cam-preview")
                            .disabled(!calc),
                    )
                    .item(
                        item(
                            "cam-m-post-selected",
                            &tr("cam-post-selected-operations"),
                            "cam-post",
                        )
                        .disabled(!calc),
                    );
            }
            let i = setup
                .operations
                .iter()
                .position(|x| x.id == id)
                .unwrap_or(0);
            menu = menu
                .separator()
                .item(item("cam-m-up", &tr("common-move-up"), "arrow-up").disabled(i == 0))
                .item(
                    item("cam-m-down", &tr("common-move-down"), "arrow-down")
                        .disabled(i + 1 >= setup.operations.len()),
                )
                .item(item("cam-m-duplicate", &tr("cam-duplicate"), "copy"));
            // To the other setups: moved within its job, copied to any.
            for (jj, other_job) in cam.jobs.iter().enumerate() {
                for other in &other_job.setups {
                    if other.id == setup.id {
                        continue;
                    }
                    let place = if cam.jobs.len() > 1 {
                        format!("{} · {}", other_job.name, other.name)
                    } else {
                        other.name.clone()
                    };
                    if jj
                        == cam.find(id).map_or(usize::MAX, |n| match n {
                            NodeRef::Operation(j, _, _) => j,
                            _ => usize::MAX,
                        })
                    {
                        menu = menu.item(item(
                            &format!("cam-m-moveto-{:x}", other.id.0),
                            &tr_with("cam-move-to", &[("place", &place)]),
                            "",
                        ));
                    }
                    menu = menu.item(item(
                        &format!("cam-m-copyto-{:x}", other.id.0),
                        &tr_with("cam-copy-to", &[("place", &place)]),
                        "copy",
                    ));
                }
            }
            menu = menu.item(item(
                "cam-m-suppress",
                &tr(if o.suppressed {
                    "common-unsuppress"
                } else {
                    "common-suppress"
                }),
                "",
            ));
            if !matches!(o.kind, OpKind::Tool(_)) {
                menu = menu.item(item(
                    "cam-m-hide",
                    &tr(if o.hidden {
                        "common-show"
                    } else {
                        "common-hide"
                    }),
                    if o.hidden { "visible" } else { "hidden" },
                ));
            }
            menu = menu.item(item("cam-m-delete", &tr("common-delete"), "delete"));
        }
    }
    let at = Vec2::new(ev.position.x + 2.0, ev.position.y + 4.0);
    let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((RowMenuFor(row), DespawnOnExit(AppState::Document)));
}

pub(super) fn on_menu_action(
    ev: On<MenuAction>,
    q: Query<&RowMenuFor, With<ContextMenuAnchor>>,
    mut commands: Commands,
) {
    let Ok(&RowMenuFor(row)) = q.get(ev.entity) else {
        return;
    };
    let action = ev.item.clone();
    commands.queue(move |w: &mut World| row_action(w, row, &action));
}

/// A row menu's action.
pub fn row_action(world: &mut World, row: CamRow, action: &str) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let id = row.id();
    match action {
        "cam-m-edit" => dialogs::edit_row(world, row),
        "cam-m-machine" => dialogs::edit_machine(world, id),
        "cam-m-setup" => {
            world.resource_mut::<CamUi>().selected = vec![id];
            dialogs::create_setup(world);
        }
        "cam-m-tool" => {
            world.resource_mut::<CamUi>().selected = vec![id];
            dialogs::create_tool(world);
        }
        "cam-m-toolpath" => {
            world.resource_mut::<CamUi>().selected = vec![id];
            dialogs::create_toolpath(world);
        }
        "cam-m-workplane" => {
            world.resource_mut::<CamUi>().selected = vec![id];
            dialogs::create_work_plane(world);
        }
        "cam-m-regenerate" => {
            world.resource_mut::<CamUi>().selected = vec![id];
            regenerate_selection(world);
        }
        "cam-m-preview" => {
            world.resource_mut::<CamUi>().selected = vec![id];
            preview::open_for_selection(world);
        }
        "cam-m-post" => {
            world.resource_mut::<CamUi>().selected = vec![id];
            export::open_for_selection(world);
        }
        "cam-m-post-selected" => export::open_for_selection(world),
        "cam-m-up" | "cam-m-down" => {
            if let Some((_, setup, _)) = cam.operation(id) {
                let i = setup
                    .operations
                    .iter()
                    .position(|o| o.id == id)
                    .unwrap_or(0);
                let to = if action == "cam-m-up" {
                    i.saturating_sub(1)
                } else {
                    i + 1
                };
                edit(
                    world,
                    Edit::Move {
                        op: id,
                        setup: setup.id,
                        index: to,
                    },
                );
            }
        }
        "cam-m-duplicate" => {
            let seed = uuid::Uuid::new_v4().as_u64_pair().0;
            edit(world, Edit::Duplicate { id, seed });
        }
        "cam-m-suppress" => {
            let on = match cam.find(id) {
                Some(NodeRef::Job(j)) => !cam.jobs[j].suppressed,
                Some(NodeRef::Setup(j, s)) => !cam.jobs[j].setups[s].suppressed,
                Some(NodeRef::Operation(j, s, o)) => {
                    !cam.jobs[j].setups[s].operations[o].suppressed
                }
                _ => return,
            };
            edit(world, Edit::Suppress(id, on));
        }
        "cam-m-hide" => {
            let on = match cam.find(id) {
                Some(NodeRef::Component(i)) => !cam.components[i].hidden,
                Some(NodeRef::Operation(j, s, o)) => !cam.jobs[j].setups[s].operations[o].hidden,
                _ => return,
            };
            edit(world, Edit::Hide(id, on));
        }
        "cam-m-update" => dialogs::update_component(world, id),
        a if a.starts_with("cam-m-moveto-") || a.starts_with("cam-m-copyto-") => {
            let want = u64::from_str_radix(&a["cam-m-moveto-".len()..], 16).ok();
            let Some(to) = cam
                .jobs
                .iter()
                .flat_map(|j| &j.setups)
                .find(|s| Some(s.id.0) == want)
            else {
                return;
            };
            if a.starts_with("cam-m-moveto-") {
                edit(
                    world,
                    Edit::Move {
                        op: id,
                        setup: to.id,
                        index: to.operations.len(),
                    },
                );
            } else {
                let seed = uuid::Uuid::new_v4().as_u64_pair().0;
                edit(
                    world,
                    Edit::CopyTo {
                        op: id,
                        setup: to.id,
                        seed,
                    },
                );
            }
        }
        "cam-m-delete" => delete(world, &[id]),
        _ => {}
    }
}

/// Deletes rows (one undo step).
pub fn delete(world: &mut World, ids: &[CamId]) {
    match ids {
        [] => {}
        [id] => {
            edit(world, Edit::Delete(*id));
        }
        _ => {
            edit(
                world,
                Edit::Batch(ids.iter().map(|id| Edit::Delete(*id)).collect()),
            );
        }
    }
    world.resource_mut::<CamUi>().selected.clear();
}

/// Delete deletes the selected rows (while no text field has the keyboard).
pub(super) fn delete_key(
    keys: Res<ButtonInput<KeyCode>>,
    ui: Res<CamUi>,
    doc: Option<Res<ActiveDocument>>,
    focus: Res<bevy::input_focus::InputFocus>,
    dialog: Option<Res<dialogs::CamDialog>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::Delete)
        || ui.selected.is_empty()
        || focus.get().is_some()
        || dialog.is_some()
    {
        return;
    }
    if doc.as_deref().and_then(active_cam).is_none() {
        return;
    }
    let ids = ui.selected.clone();
    commands.queue(move |w: &mut World| delete(w, &ids));
}
