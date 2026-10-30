//! The CAM Studio's toolbar: Insert, the create buttons (enabled as the studio allows),
//! Regenerate, Preview and Post.

use super::*;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Needs {
    Part,
    Job,
    Machine,
    Setup,
    Tool,
    Toolpath,
    Calculated,
}

/// The toolbar of a CAM Studio tab.
pub fn toolbar(tb: &mut ChildSpawnerCommands, t: &Theme) {
    let tool = |name: &'static str, icon: &'static str, tip: &'static str| {
        ToolButton::new(name, icon)
            .icon_size(20.0)
            .tooltip(tip)
            .build(t)
    };
    tb.spawn((
        ToolButton::new("cam-tb-insert", "cam-insert")
            .icon_size(20.0)
            .dropdown(true)
            .tooltip("Insert part, workholding and stock")
            .build(t),
        observe(
            |ev: On<Activate>,
             q: Query<(&UiGlobalTransform, &ComputedNode)>,
             theme: Res<Theme>,
             mut commands: Commands| {
                let at = q
                    .get(ev.entity)
                    .map(|(tf, n)| {
                        let s = n.inverse_scale_factor();
                        Vec2::new(
                            (tf.translation.x - n.size().x / 2.0) * s,
                            (tf.translation.y + n.size().y / 2.0) * s + 2.0,
                        )
                    })
                    .unwrap_or(Vec2::new(60.0, 80.0));
                let menu = Menu::new("cam-insert-menu")
                    .min_width(190.0)
                    .item(MenuItem::new("cam-insert-part", "Insert part").icon("part"))
                    .item(
                        MenuItem::new("cam-insert-workholding", "Insert workholding")
                            .icon("cam-workholding"),
                    )
                    .item(MenuItem::new("cam-insert-stock", "Insert stock").icon("cam-stock"));
                let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
                commands
                    .entity(anchor)
                    .insert((InsertMenu, DespawnOnExit(AppState::Document)));
            },
        ),
    ));
    let button = |tb: &mut ChildSpawnerCommands,
                  name: &'static str,
                  icon: &'static str,
                  tip: &'static str,
                  needs: Needs,
                  run: fn(&mut World)| {
        tb.spawn((
            tool(name, icon, tip),
            needs,
            observe(move |_: On<Activate>, mut commands: Commands| {
                commands.queue(run);
            }),
        ));
    };
    button(
        tb,
        "cam-tb-job",
        "cam-job",
        "Create job",
        Needs::Part,
        dialogs::create_job,
    );
    button(
        tb,
        "cam-tb-machine",
        "cam-machine",
        "Create machine",
        Needs::Job,
        dialogs::create_machine,
    );
    button(
        tb,
        "cam-tb-setup",
        "cam-setup",
        "Create setup",
        Needs::Machine,
        dialogs::create_setup,
    );
    button(
        tb,
        "cam-tb-tool",
        "cam-tool",
        "Create tool",
        Needs::Setup,
        dialogs::create_tool,
    );
    button(
        tb,
        "cam-tb-toolpath",
        "cam-toolpath",
        "Create toolpath",
        Needs::Tool,
        dialogs::create_toolpath,
    );
    button(
        tb,
        "cam-tb-workplane",
        "cam-work-plane",
        "Create work plane",
        Needs::Setup,
        dialogs::create_work_plane,
    );
    tb.spawn(toolbar_separator(t));
    button(
        tb,
        "cam-tb-regenerate",
        "cam-regenerate",
        "Regenerate the selected toolpaths (all out-of-date ones of the active job when none is selected)",
        Needs::Toolpath,
        regenerate_selection,
    );
    button(
        tb,
        "cam-tb-preview",
        "cam-preview",
        "Preview operation: back plot, verify and simulate",
        Needs::Calculated,
        preview::open_for_selection,
    );
    button(
        tb,
        "cam-tb-post",
        "cam-post",
        "Post the setup to G-code",
        Needs::Calculated,
        export::open_for_selection,
    );
    tb.spawn(toolbar_separator(t));
    tb.spawn((
        tool(
            "cam-tb-stock",
            "cam-stock",
            "Show or hide the active job's stock",
        ),
        observe(|_: On<Activate>, mut ui: ResMut<CamUi>| {
            ui.show_stock = !ui.show_stock;
        }),
    ));
}

#[derive(Component)]
pub(super) struct InsertMenu;

pub(super) fn on_insert_menu_action(
    ev: On<MenuAction>,
    q: Query<(), (With<InsertMenu>, With<ContextMenuAnchor>)>,
    mut commands: Commands,
) {
    if !q.contains(ev.entity) {
        return;
    }
    let role = match ev.item.as_str() {
        "cam-insert-part" => cadrs_core::cam::Role::Part,
        "cam-insert-workholding" => cadrs_core::cam::Role::Workholding,
        "cam-insert-stock" => cadrs_core::cam::Role::Stock,
        _ => return,
    };
    commands.queue(move |w: &mut World| dialogs::insert_component(w, role));
}

/// Greys the toolbar's buttons until their step is possible.
pub(super) fn sync_toolbar(
    doc: Option<Res<ActiveDocument>>,
    computed: Res<Computed>,
    ui: Res<CamUi>,
    q: Query<(Entity, &Needs, Has<bevy::ui::InteractionDisabled>)>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    let Some((_, cam)) = active_cam(&doc) else {
        return;
    };
    let job = cam.active();
    let target = target_setup(cam, &ui);
    let setup = target.and_then(|(s, _)| cam.setup(s));
    let has_tool = setup.is_some_and(|(_, s)| s.operations.iter().any(|o| o.tool().is_some()));
    let has_toolpath = job.is_some_and(|j| {
        j.setups
            .iter()
            .any(|s| s.operations.iter().any(|o| o.toolpath().is_some()))
    });
    let calculated = job.is_some_and(|j| {
        j.setups.iter().any(|s| {
            s.operations
                .iter()
                .any(|o| computed.current(o.id).is_some())
        })
    });
    for (e, needs, disabled) in &q {
        let on = match needs {
            Needs::Part => cam
                .components
                .iter()
                .any(|c| c.role == cadrs_core::cam::Role::Part),
            Needs::Job => job.is_some(),
            Needs::Machine => job.is_some_and(|j| j.machine.is_some()),
            Needs::Setup => setup.is_some(),
            Needs::Tool => has_tool,
            Needs::Toolpath => has_toolpath,
            Needs::Calculated => calculated,
        };
        if on && disabled {
            commands
                .entity(e)
                .try_remove::<bevy::ui::InteractionDisabled>();
        } else if !on && !disabled {
            commands.entity(e).try_insert(bevy::ui::InteractionDisabled);
        }
    }
}

/// Regenerate: the selected toolpaths (a setup or job: all of theirs), else every toolpath of
/// the active job that isn't up to date.
pub fn regenerate_selection(world: &mut World) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let sel = world.resource::<CamUi>().selected.clone();
    let mut ops = Vec::new();
    for id in &sel {
        match cam.find(*id) {
            Some(NodeRef::Operation(..)) => {
                if cam
                    .operation(*id)
                    .is_some_and(|(_, _, o)| o.toolpath().is_some())
                {
                    ops.push(*id);
                }
            }
            Some(NodeRef::Setup(j, s)) => ops.extend(toolpaths_of(&cam.jobs[j].setups[s])),
            Some(NodeRef::Job(j)) => {
                for s in &cam.jobs[j].setups {
                    ops.extend(toolpaths_of(s));
                }
            }
            _ => {}
        }
    }
    let c = world.resource::<Computed>();
    if ops.is_empty()
        && let Some(job) = cam.active()
    {
        for s in &job.setups {
            ops.extend(
                toolpaths_of(s)
                    .into_iter()
                    .filter(|o| !matches!(c.status(*o), Status::Done(_))),
            );
        }
    }
    world.resource_mut::<Computed>().request(ops);
}

pub(super) fn toolpaths_of(s: &cadrs_core::cam::Setup) -> Vec<CamId> {
    s.operations
        .iter()
        .filter(|o| !o.suppressed && o.toolpath().is_some())
        .map(|o| o.id)
        .collect()
}
