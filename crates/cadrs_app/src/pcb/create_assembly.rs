//! **Create an assembly from this ECAD data** (P3H.6; PCB7.1, X7;
//! `ex2-step5-create-assembly-dialog.png`): the toolbar's `create-assembly` button opens
//! "Create assembly from '<board>'" for the board shown (the course's screenshot titles it with
//! another board's name; ours always names the one it builds): "Select features to include in the
//! assembly:" with **Board** ✓, **Components** ✓ and **Keep-In and Keep-Out Areas** ☐, a note
//! that building can take a while and you can keep working, OK / Cancel.
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
use cadrs_pcb::create_assembly::{CreateOptions, generate};
use cadrs_ui::{Button, Checkbox, CheckboxState, Dialog, DialogClose, Notification, Spinner, Theme, show_notification};

use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub fn register(app: &mut App) {
    app.add_systems(Update, finish_create.run_if(in_state(AppState::Document)));
}

#[derive(Component)]
struct CreateDialog {
    element: ElementId,
    board: BoardId,
}

/// A generation on the kernel thread.
#[derive(Component)]
struct RunningCreate {
    board_name: String,
    pending: PendingJob<Result<CreatePcbAssembly, String>>,
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
                b.spawn(Checkbox::new("pcb-create-board").label("Board").checked(true).height(24.0).build(t));
                b.spawn(Checkbox::new("pcb-create-components").label("Components").checked(true).height(24.0).build(t));
                b.spawn(Checkbox::new("pcb-create-keeps").label("Keep-In and Keep-Out Areas").height(24.0).build(t));
                b.spawn((
                    Name::new("pcb-create-note"),
                    t.text(
                        "Building the assembly can take several minutes. You can keep working in cadrs while it is built.",
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.foreground,
                    ),
                    Node { margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(14.0), Val::Px(6.0)), width: Val::Px(420.0), ..default() },
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

/// Starts building the tabs for `board` of the PCB Studio `element` (the dialog's OK).
pub fn start(world: &mut World, element: ElementId, board: BoardId, opts: CreateOptions) {
    let Some(doc) = world.get_resource::<ActiveDocument>().map(|d| d.doc.clone()) else { return };
    let Some(name) = doc.element(element).and_then(|e| e.pcb()).and_then(|s| s.board(board)).map(|b| b.name().to_string()) else { return };
    let pending = cadrs_core::rebuild::run_on_worker(move |r| {
        let cmd = generate(&doc, element, board, &opts).map_err(|e| e.to_string())?;
        // Rebuild the new Part Studios once, so their parts are cached when the tabs are shown.
        for e in &cmd.elements {
            if matches!(e.kind, cadrs_core::document::ElementKind::PartStudio { .. }) {
                r.rebuild(&e.active_features());
            }
        }
        Ok(cmd)
    });
    world.spawn((Name::new("pcb-create-running"), RunningCreate { board_name: name.clone(), pending }, DespawnOnExit(AppState::Document)));
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

fn finish_create(q: Query<(Entity, &RunningCreate)>, mut commands: Commands) {
    for (e, run) in &q {
        let Some(result) = run.pending.poll() else { continue };
        let result = result.unwrap_or_else(|| Err("The kernel thread stopped".into()));
        let name = run.board_name.clone();
        commands.entity(e).despawn();
        commands.queue(move |world: &mut World| finish(world, &name, result));
    }
}

fn toast(world: &mut World, n: Notification) -> Entity {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let e = show_notification(&mut commands, &theme, n);
    world.flush();
    e
}

fn finish(world: &mut World, board: &str, result: Result<CreatePcbAssembly, String>) {
    let mut q = world.query_filtered::<Entity, With<CreateProgress>>();
    let cards: Vec<Entity> = q.iter(world).collect();
    for c in cards {
        world.entity_mut(c).despawn();
    }
    let cmd = match result {
        Ok(c) => c,
        Err(e) => {
            toast(world, Notification::warning(format!("Couldn't create the assembly from {board}: {e}")).name("pcb-create-toast").max_width(640.0));
            return;
        }
    };
    let assembly = cmd.generated.assembly;
    if let Err(e) = world.resource_mut::<ActiveDocument>().execute(&cmd) {
        toast(world, Notification::warning(format!("Couldn't create the assembly from {board}: {e}")).name("pcb-create-toast").max_width(640.0));
        return;
    }
    let t = toast(world, Notification::info(format!("Created the Part Studio and Assembly \"{board}\"")).seconds(8.0).name("pcb-create-toast").max_width(640.0));
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
