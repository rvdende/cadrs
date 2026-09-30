//! Modal dialogs, modeled on gpui-component's `Dialog` (`dialog.rs`) and Onshape's "New document"
//! dialog: a dimmed backdrop, a white panel with a title and ✕, a body, and a footer with
//! right-aligned buttons.
//!
//! The ✕ button is named `<name>-close`. Clicking it or pressing Escape triggers
//! [`DialogClose`] on the dialog root and then despawns the dialog. Clicking the backdrop does
//! nothing (the dialog is modal).

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;

use crate::anim::FadeIn;
use crate::button::IconButton;
use crate::menu::MenuDismissLayer;
use crate::theme::Theme;
use crate::z;

pub struct DialogPlugin;

impl Plugin for DialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_close_button)
            .add_observer(on_dialog_close)
            .add_systems(Update, (close_on_escape, dismiss_toasts_on_open));
    }
}

/// Asks a dialog to close. Trigger it on the dialog root; the dialog despawns after observers
/// have run.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct DialogClose {
    pub entity: Entity,
}

/// The root (backdrop) entity of a dialog.
#[derive(Component, Debug, Clone, Copy)]
pub struct DialogRoot;

/// The ✕ button of a dialog; points at its root.
#[derive(Component, Debug, Clone, Copy)]
struct DialogCloseButton(Entity);

type SpawnFn = Box<dyn FnOnce(&mut ChildSpawner) + Send + Sync>;

/// Builder for a modal dialog.
pub struct Dialog {
    name: Cow<'static, str>,
    title: String,
    width: f32,
    body: Option<SpawnFn>,
    footer: Option<SpawnFn>,
    header: Option<SpawnFn>,
    /// Title size and weight (default: the theme's large bold).
    title_font: Option<(f32, FontWeight)>,
    header_height: f32,
}

impl Dialog {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            title: String::new(),
            width: 550.0,
            body: None,
            footer: None,
            header: None,
            title_font: None,
            header_height: 43.0,
        }
    }

    /// The title's size and weight (Onshape's keyboard shortcuts dialog: 20 px regular).
    pub fn title_font(mut self, size: f32, weight: FontWeight) -> Self {
        self.title_font = Some((size, weight));
        self
    }

    /// The header row's height (default 43 px).
    pub fn header_height(mut self, h: f32) -> Self {
        self.header_height = h;
        self
    }

    /// Spawns extra header content between the title and the ✕ (such as a search box).
    pub fn header(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.header = Some(Box::new(f));
        self
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// Spawns the body content.
    pub fn body(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.body = Some(Box::new(f));
        self
    }

    /// Spawns the footer content (usually buttons, laid out right-aligned in a row).
    pub fn footer(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.footer = Some(Box::new(f));
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let theme = theme.clone();
        let Dialog {
            name,
            title,
            width,
            body,
            footer,
            header,
            title_font,
            header_height,
        } = self;
        let (title_size, title_weight) = title_font.unwrap_or((theme.font_lg, FontWeight::BOLD));
        let close_name = format!("{name}-close");
        let overlay = theme.overlay;
        (
            Name::new(name.into_owned()),
            DialogRoot,
            bevy::input_focus::tab_navigation::TabGroup::modal(),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                ..default()
            },
            BackgroundColor(overlay.with_alpha(0.0)),
            FadeIn {
                target: overlay,
                elapsed: 0.0,
                duration: theme.fade_duration.as_secs_f32(),
            },
            GlobalZIndex(z::DIALOG),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let root = p.target_entity();
                p.spawn((
                    Name::new("dialog-panel"),
                    Node {
                        width: Val::Px(width),
                        margin: UiRect::top(Val::Px(24.0)),
                        flex_direction: FlexDirection::Column,
                        border_radius: BorderRadius::all(Val::Px(theme.radius_lg)),
                        ..default()
                    },
                    BackgroundColor(theme.background),
                    BoxShadow::new(
                        Color::srgba(0.0, 0.0, 0.0, 0.25),
                        Val::Px(0.0),
                        Val::Px(4.0),
                        Val::Px(0.0),
                        Val::Px(16.0),
                    ),
                    Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                        // Header: title and ✕.
                        p.spawn((
                            Node {
                                height: Val::Px(header_height),
                                padding: UiRect::new(
                                    Val::Px(theme.space[4]),
                                    Val::Px(theme.space[3]),
                                    Val::Px(0.0),
                                    Val::Px(0.0),
                                ),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceBetween,
                                border: UiRect::bottom(Val::Px(1.0)),
                                ..default()
                            },
                            BorderColor::all(theme.separator),
                        ))
                        .with_children(|h| {
                            h.spawn((
                                theme.text(title, title_size, title_weight, theme.foreground),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            if let Some(f) = header {
                                f(h);
                            }
                            h.spawn((
                                IconButton::new(close_name, "close")
                                    .tooltip("Close")
                                    .build(&theme),
                                DialogCloseButton(root),
                            ));
                        });
                        // Body.
                        p.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(theme.space[4])),
                            row_gap: Val::Px(theme.space[4]),
                            ..default()
                        })
                        .with_children(|b| {
                            if let Some(f) = body {
                                f(b);
                            }
                        });
                        // Footer.
                        if let Some(f) = footer {
                            p.spawn((
                                Node {
                                    height: Val::Px(52.0),
                                    padding: UiRect::horizontal(Val::Px(theme.space[4])),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::FlexEnd,
                                    column_gap: Val::Px(theme.space[4]),
                                    border: UiRect::top(Val::Px(1.0)),
                                    ..default()
                                },
                                BorderColor::all(theme.separator),
                            ))
                            .with_children(|b| f(b));
                        }
                    })),
                ));
            })),
        )
    }
}

fn on_close_button(ev: On<Activate>, q: Query<&DialogCloseButton>, mut commands: Commands) {
    if let Ok(b) = q.get(ev.entity) {
        commands.trigger(DialogClose { entity: b.0 });
    }
}

fn on_dialog_close(ev: On<DialogClose>, q: Query<(), With<DialogRoot>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.entity(ev.entity).try_despawn();
    }
}

fn close_on_escape(
    mut keys: MessageReader<KeyboardInput>,
    q_dialogs: Query<(Entity, &GlobalZIndex), With<DialogRoot>>,
    q_menus: Query<(), With<MenuDismissLayer>>,
    mut commands: Commands,
) {
    for k in keys.read() {
        if k.state != ButtonState::Pressed || k.key_code != KeyCode::Escape || !q_menus.is_empty() {
            continue;
        }
        // Close the topmost dialog.
        if let Some((e, _)) = q_dialogs.iter().max_by_key(|(e, z)| (z.0, *e)) {
            commands.trigger(DialogClose { entity: e });
        }
    }
}

/// Opening a modal dialog dismisses transient toasts (ones that hide by themselves), so an
/// earlier "Renamed …" toast does not linger under the dialog.
fn dismiss_toasts_on_open(
    q_new: Query<(), Added<DialogRoot>>,
    q_toasts: Query<(Entity, &crate::toast::Toast)>,
    mut commands: Commands,
) {
    if q_new.is_empty() {
        return;
    }
    for (e, t) in &q_toasts {
        if t.autohide {
            commands.entity(e).try_despawn();
        }
    }
}
