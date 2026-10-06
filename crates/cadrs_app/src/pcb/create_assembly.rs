//! **Create an assembly from this ECAD data** (P3H.6; PCB7.1, X7;
//! `ex2-step5-create-assembly-dialog.png`): the toolbar's `create-assembly` button opens
//! "Create assembly from '<board>'" for the board shown (the course's screenshot titles it with
//! another board's name; ours always names the one it builds): "Select features to include in the
//! assembly:" with **Board** ✓, **Components** ✓ and **Keep-In and Keep-Out Areas** ☐, a note
//! that building can take a while and you can keep working, OK / Cancel.
//!
//! P3H.7: the components are **component documents** in the PCB settings' component folder
//! (`cadrs_pcb::component_docs`), written to the store and versioned on the kernel thread too,
//! with their thumbnails; the toast counts them.
//!
//! OK builds the tabs on the kernel thread ([`cadrs_pcb::create_assembly::generate`] on a copy of
//! the document, then each new Part Studio rebuilt once so its parts are cached) while a
//! progress card with a spinner shows at the bottom of the view; the app stays usable. When it
//! is done the tabs go into the document as one undo step and a toast names them, with **Open
//! assembly**.
//!
//! Names: `pcb-create-dialog` (`pcb-create-board`, `pcb-create-components`, `pcb-create-keeps`,
//! `pcb-create-note`, `pcb-create-ok`, `pcb-create-cancel`), `pcb-create-progress`,
//! `pcb-create-toast` (`pcb-create-open`).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementId;
use cadrs_core::pcb::{BoardId, CreatePcbAssembly};
use cadrs_core::rebuild::PendingJob;
use cadrs_pcb::component_docs::ComponentDocuments;
use cadrs_pcb::create_assembly::{CreateOptions, generate, generate_linked};
use cadrs_ui::{Button, Checkbox, CheckboxState, Dialog, DialogClose, Notification, Spinner, Theme, show_notification};

use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub fn register(app: &mut App) {
    app.init_resource::<CreateHold>()
        .add_systems(Update, finish_create.run_if(in_state(AppState::Document)))
        .add_systems(PostUpdate, flag_creating);
}

/// Scenarios only (`pcb-create-hold <frames>`): the progress card stays at least this many
/// frames, so a headless run can photograph it (the build is often done in a frame or two).
#[derive(Resource, Default)]
pub struct CreateHold(pub u32);

/// Scripted steps wait while a generation runs, but not while a scenario's hold keeps its
/// progress card up (the scenario photographs it).
fn flag_creating(q: Query<&RunningCreate>, hold: Res<CreateHold>, mut pending: ResMut<cadrs_ui::PendingWork>, mut why: ResMut<cadrs_ui::PendingWhy>) {
    if q.iter().any(|r| r.frames >= hold.0) {
        pending.0 = true;
        why.add("PCB create assembly");
    }
}

#[derive(Component)]
struct CreateDialog {
    element: ElementId,
    board: BoardId,
}

/// What a generation made: the command for the board document, and (P3H.7) the component
/// documents it wrote to the store or reused.
pub struct Created {
    pub command: CreatePcbAssembly,
    /// New component documents, and how many existing ones were reused.
    pub created: usize,
    pub reused: usize,
    /// The component folder's name.
    pub folder: String,
}

/// A generation on the kernel thread.
#[derive(Component)]
struct RunningCreate {
    board_name: String,
    /// Frames since it started (see [`CreateHold`]).
    frames: u32,
    pending: PendingJob<Result<Created, String>>,
}

/// The progress card.
#[derive(Component)]
struct CreateProgress;

fn close_dialog(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<CreateDialog>>();
    let roots: Vec<Entity> = q.iter(world).collect();
    for e in roots {
        world.trigger(DialogClose { entity: e });
    }
}

/// Opens the dialog for the board shown (the toolbar's Create assembly).
pub fn open_create_dialog(world: &mut World) {
    let mut q = world.query_filtered::<(), With<CreateDialog>>();
    if q.iter(world).next().is_some() {
        return;
    }
    let Some((element, board, b)) = super::shown(world) else { return };
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.spawn((
        Dialog::new("pcb-create-dialog")
            .title(format!("Create assembly from '{}'", b.name()))
            .width(450.0)
            .body(move |b| {
                let t = &tb;
                b.spawn((
                    t.text("Select features to include in the assembly:", t.font_sm, FontWeight::NORMAL, t.foreground),
                    Node { margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(4.0), Val::Px(6.0)), ..default() },
                ));
                // The rows 19 px apart, as the course's dialog (the body's 8 px row gap left
                // out: they are one group).
                b.spawn((Name::new("pcb-create-options"), Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(0.0), ..default() })).with_children(|c| {
                    c.spawn(Checkbox::new("pcb-create-board").label("Board").checked(true).height(19.0).build(t));
                    c.spawn(Checkbox::new("pcb-create-components").label("Components").checked(true).height(19.0).build(t));
                    c.spawn(Checkbox::new("pcb-create-keeps").label("Keep-In and Keep-Out Areas").height(19.0).build(t));
                });
                b.spawn((
                    Name::new("pcb-create-note"),
                    t.text(
                        "Building the assembly can take several minutes. You can keep working in cadrs while it is built.",
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.foreground,
                    ),
                    Node { margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(10.0), Val::Px(6.0)), width: Val::Px(420.0), ..default() },
                ))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("pcb-create-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(accept);
                    }),
                ));
                f.spawn((
                    Button::new("pcb-create-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_dialog);
                    }),
                ));
            })
            .build(&theme),
        CreateDialog { element, board },
        DespawnOnExit(AppState::Document),
    ));
}

fn checked(world: &mut World, name: &str) -> bool {
    let mut q = world.query::<(&Name, &CheckboxState)>();
    q.iter(world).find(|(n, _)| n.as_str() == name).is_some_and(|(_, s)| s.checked)
}

fn accept(world: &mut World) {
    let (element, board) = {
        let mut q = world.query::<&CreateDialog>();
        let Some(d) = q.iter(world).next() else { return };
        (d.element, d.board)
    };
    let opts = CreateOptions { board: checked(world, "pcb-create-board"), components: checked(world, "pcb-create-components"), keep_areas: checked(world, "pcb-create-keeps") };
    close_dialog(world);
    start(world, element, board, opts);
}

/// Starts building the tabs for `board` of the PCB Studio `element` (the dialog's OK). With a
/// document store (always, in the app) the components are **component documents** (P3H.7): one
/// stored document per new package in the settings' component folder, versioned, referenced by
/// version; the store work runs on the kernel thread too.
pub fn start(world: &mut World, element: ElementId, board: BoardId, opts: CreateOptions) {
    let Some(doc) = world.get_resource::<ActiveDocument>().map(|d| d.doc.clone()) else { return };
    let Some(name) = doc.element(element).and_then(|e| e.pcb()).and_then(|s| s.board(board)).map(|b| b.name().to_string()) else { return };
    let store = world.get_resource::<crate::DocumentStore>().map(|s| s.0.clone());
    let folder = doc.element(element).and_then(|e| e.pcb()).and_then(|s| s.settings.component_folder.clone());
    let now = world.resource::<crate::AppClock>().now();
    let user = world.resource::<crate::UserProfile>().id.clone();
    let board_name = name.clone();
    let pending = cadrs_core::rebuild::run_on_worker(move |r| {
        let (cmd, docs) = match &store {
            Some(store) => {
                let mut docs = ComponentDocuments::new(store, folder.as_ref(), doc.id, &board_name, &user, now)?;
                let cmd = generate_linked(&doc, element, board, &opts, &mut docs).map_err(|e| e.to_string())?;
                (cmd, Some(docs))
            }
            None => (generate(&doc, element, board, &opts).map_err(|e| e.to_string())?, None),
        };
        // Rebuild the new Part Studios (and the component copies) once, so their parts are cached
        // when the tabs are shown.
        for e in cmd.elements.iter().chain(cmd.links.iter().map(|l| &l.element)) {
            if matches!(e.kind, cadrs_core::document::ElementKind::PartStudio { .. }) {
                r.rebuild(&e.active_features());
            }
        }
        // The new component documents' thumbnails for the documents page (their part).
        if let (Some(store), Some(docs)) = (&store, &docs) {
            for u in docs.used.iter().filter(|u| u.created) {
                let Some(l) = cmd.links.iter().find(|l| l.source.document == Some(u.document) && matches!(l.element.kind, cadrs_core::document::ElementKind::PartStudio { .. })) else { continue };
                let build = r.rebuild(&l.element.active_features());
                let list: Vec<(&cadrs_core::Solid, [u8; 3])> = build.parts.iter().map(|p| (&*p.solid, cadrs_core::appearance::part_appearance(p, l.element.part_props()).rgb)).collect();
                if !list.is_empty() {
                    let _ = store.write_thumbnail(u.document, &cadrs_core::assembly::thumb::render(&list, 96));
                }
            }
        }
        let (created, reused, folder) = match &docs {
            Some(d) => (d.created(), d.used.len() - d.created(), d.folder().name.clone()),
            None => (0, 0, String::new()),
        };
        Ok(Created { command: cmd, created, reused, folder })
    });
    world.spawn((Name::new("pcb-create-running"), RunningCreate { board_name: name.clone(), frames: 0, pending }, DespawnOnExit(AppState::Document)));
    show_progress(world, &name);
}

/// The card at the bottom of the view while the tabs are built.
fn show_progress(world: &mut World, board: &str) {
    let mut q = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = q.iter(world).next() else { return };
    let theme = world.resource::<Theme>().clone();
    let t = &theme;
    let card = world
        .spawn((
            Name::new("pcb-create-progress"),
            CreateProgress,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(12.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
                column_gap: Val::Px(10.0),
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius)),
                ..default()
            },
            BackgroundColor(t.background),
            BorderColor::all(t.panel_border),
            DespawnOnExit(AppState::Document),
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            c.spawn(Spinner::new("pcb-create-spinner").size(18.0).thickness(2.5).build(t));
            c.spawn((t.text(format!("Creating assembly from '{board}'…"), t.font_sm, FontWeight::MEDIUM, t.foreground), Pickable::IGNORE));
        })
        .id();
    world.entity_mut(area).add_child(card);
}

fn finish_create(mut q: Query<(Entity, &mut RunningCreate)>, hold: Res<CreateHold>, mut commands: Commands) {
    for (e, mut run) in &mut q {
        run.frames += 1;
        if run.frames < hold.0 {
            continue;
        }
        let Some(result) = run.pending.poll() else { continue };
        let result = result.unwrap_or_else(|| Err("The kernel thread stopped".into()));
        let name = run.board_name.clone();
        commands.entity(e).despawn();
        commands.queue(move |world: &mut World| finish(world, &name, result));
    }
}

/// The toast after a Create: "Created the Part Studio and Assembly "Vision PCB"" and, with
/// component documents (P3H.7), "Created "Vision PCB" with 8 component documents in PCB
/// Components" (and how many were reused: "… with 1 new component document in PCB Components,
/// 6 reused").
pub fn created_message(board: &str, created: usize, reused: usize, folder: &str) -> String {
    let docs = |n: usize| format!("{n} {}component document{}", if reused > 0 { "new " } else { "" }, if n == 1 { "" } else { "s" });
    match (created, reused) {
        (0, 0) => format!("Created the Part Studio and Assembly \"{board}\""),
        (c, 0) => format!("Created \"{board}\" with {} in {folder}", docs(c)),
        (0, r) => format!("Created \"{board}\" with its {r} component document{}", if r == 1 { "" } else { "s" }),
        (c, r) => format!("Created \"{board}\" with {} in {folder}, {r} reused", docs(c)),
    }
}

fn toast(world: &mut World, n: Notification) -> Entity {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let e = show_notification(&mut commands, &theme, n);
    world.flush();
    e
}

fn finish(world: &mut World, board: &str, result: Result<Created, String>) {
    let mut q = world.query_filtered::<Entity, With<CreateProgress>>();
    let cards: Vec<Entity> = q.iter(world).collect();
    for c in cards {
        world.entity_mut(c).despawn();
    }
    let made = match result {
        Ok(c) => c,
        Err(e) => {
            toast(world, Notification::warning(format!("Couldn't create the assembly from {board}: {e}")).name("pcb-create-toast").max_width(640.0));
            return;
        }
    };
    let cmd = made.command;
    let assembly = cmd.generated.assembly;
    if let Err(e) = world.resource_mut::<ActiveDocument>().execute(&cmd) {
        toast(world, Notification::warning(format!("Couldn't create the assembly from {board}: {e}")).name("pcb-create-toast").max_width(640.0));
        return;
    }
    // The linked documents' states and update badges (P3H.7).
    world.resource_mut::<crate::linked::LinkStatus>().invalidate();
    let t = toast(world, Notification::info(created_message(board, made.created, made.reused, &made.folder)).seconds(8.0).name("pcb-create-toast").max_width(760.0));
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let open = cadrs_ui::toast_action(&mut commands, &theme, t, "pcb-create-open", "Open assembly");
    commands.entity(open).insert(observe(move |_: On<Activate>, mut commands: Commands| {
        commands.queue(move |world: &mut World| {
            cadrs_ui::close_toasts(world);
            if let Some(mut d) = world.get_resource_mut::<ActiveDocument>() {
                d.set_active(assembly);
            }
        });
    }));
    world.flush();
}
