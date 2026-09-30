//! Toast notifications, modeled on gpui-component's `Notification` (`notification.rs`) and
//! Onshape's in-app messages (`reference/onshape/screens/07`, `sketch_dialog/restore.png`): a
//! light-blue pill with an ⓘ icon, the message, optional action links and a ✕, at the top
//! center of the [`ToastHost`] (the 3D viewport in a document) or of the window.
//!
//! - [`Notification::info`] builds one; [`show_notification`] shows it, replacing any other.
//! - Auto-hiding toasts ([`show_toast`]) disappear after a few seconds or at the next click
//!   anywhere outside them. A prompt such as "Select a sketch plane" sets
//!   [`Notification::autohide`] to false and stays until its ✕ is clicked or the app closes it
//!   with [`close_toasts`].
//! - Clicking the ✕ (named `toast-close`) closes the toast.

use std::borrow::Cow;

use bevy::picking::hover::Hovered;
use bevy::picking::pointer::{PointerAction, PointerInput};
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::{UiTransform, Val2};
use bevy::ui_widgets::{Activate, observe};

use crate::button::{Button, visuals_for};
use crate::ellipsis::Ellipsis;
use crate::icon::icon;
use crate::style::{StateColors, Visuals};
use crate::theme::Theme;
use crate::z;

pub struct ToastPlugin;

impl Plugin for ToastPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (expire_toasts, dismiss_on_click));
    }
}

/// A toast on screen. Auto-hiding toasts are despawned when `remaining` reaches zero.
#[derive(Component, Debug, Clone, Copy)]
#[require(Hovered)]
pub struct Toast {
    pub remaining: f32,
    pub autohide: bool,
}

/// Toasts are shown at the top center of this node (the viewport area of a document).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ToastHost;

/// Where a toast's action links and ✕ go.
#[derive(Component, Debug, Clone, Copy)]
struct ToastActions;

/// Builder for a toast notification.
#[derive(Debug, Clone)]
pub struct Notification {
    message: String,
    warning: bool,
    /// An error (P3G.2): pale red with a red icon, shown until closed.
    error: bool,
    seconds: f32,
    autohide: bool,
    closable: bool,
    name: Cow<'static, str>,
    /// Where the top-left corner goes in the host (px); `None` centers it at the top.
    at: Option<Vec2>,
    /// The widest it gets (px); a longer message is cut with "…" (the whole of it in a tooltip).
    max_width: Option<f32>,
}

impl Notification {
    /// An informational message (the only kind Onshape shows in this position).
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            warning: false,
            error: false,
            seconds: 3.0,
            autohide: true,
            closable: true,
            name: "toast".into(),
            at: None,
            max_width: None,
        }
    }

    /// A warning banner (`screens/15`): pale yellow with a yellow ⚠, shown until closed.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            warning: true,
            autohide: false,
            ..Self::info(message)
        }
    }

    /// An error message (a refused action, such as a circular reference): pale red with a red
    /// icon, shown until closed.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            error: true,
            autohide: false,
            ..Self::info(message)
        }
    }

    /// The toast's `Name` (default `toast`).
    pub fn name(mut self, n: impl Into<Cow<'static, str>>) -> Self {
        self.name = n.into();
        self
    }

    /// How long an auto-hiding toast stays (default 3 s).
    pub fn seconds(mut self, s: f32) -> Self {
        self.seconds = s;
        self
    }

    /// False keeps the toast until it is closed (default true).
    pub fn autohide(mut self, a: bool) -> Self {
        self.autohide = a;
        self
    }

    /// Puts the top-left corner at `left`, `top` (px) in the host instead of centering it at
    /// the top (the sketch conflict banner sits next to the sketch dialog, `screens/15`).
    pub fn at(mut self, left: f32, top: f32) -> Self {
        self.at = Some(Vec2::new(left, top));
        self
    }

    /// Caps the width (px): a longer message is cut with "…" and shows whole in a tooltip;
    /// the icon and the ✕ always stay.
    pub fn max_width(mut self, w: f32) -> Self {
        self.max_width = Some(w);
        self
    }

    /// Shows the ✕ (default true).
    pub fn closable(mut self, c: bool) -> Self {
        self.closable = c;
        self
    }
}

/// A toast's width without an explicit cap: longer messages wrap (P3G.4).
pub const DEFAULT_MAX_WIDTH: f32 = 720.0;

/// Shows `text` in a toast for 3 seconds, replacing any toast already shown.
pub fn show_toast(commands: &mut Commands, theme: &Theme, text: impl Into<String>) -> Entity {
    show_notification(commands, theme, Notification::info(text))
}

/// Shows `text` in a toast for `seconds`. Returns the toast entity; add actions with
/// [`toast_action`].
pub fn show_toast_for(
    commands: &mut Commands,
    theme: &Theme,
    text: impl Into<String>,
    seconds: f32,
) -> Entity {
    show_notification(commands, theme, Notification::info(text).seconds(seconds))
}

/// Removes every toast.
pub fn close_toasts(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<Toast>>();
    let old: Vec<Entity> = q.iter(world).collect();
    for e in old {
        world.entity_mut(e).despawn();
    }
}

/// Removes the toasts that hide by themselves ("Tab deleted. Undo", "Renamed …"): they name an
/// action, so undoing or redoing makes them stale. Prompts that stay (autohide off) are kept.
pub fn close_transient_toasts(world: &mut World) {
    let mut q = world.query::<(Entity, &Toast)>();
    let old: Vec<Entity> = q.iter(world).filter(|(_, t)| t.autohide).map(|(e, _)| e).collect();
    for e in old {
        world.entity_mut(e).despawn();
    }
}

/// Shows a notification, replacing any toast already shown. Returns the toast entity.
pub fn show_notification(commands: &mut Commands, theme: &Theme, n: Notification) -> Entity {
    commands.queue(close_toasts);
    let t = theme.clone();
    let closable = n.closable;
    let warning = n.warning;
    let at = n.at;
    let max_width = n.max_width;
    let error = n.error;
    let (background, border, icon_name, icon_color, foreground) = if error {
        (
            Color::srgb_u8(0xfd, 0xec, 0xec),
            Color::srgb_u8(0xef, 0xb8, 0xb8),
            "error-filled",
            theme.danger,
            Color::srgb_u8(0x5c, 0x12, 0x12),
        )
    } else if warning {
        (
            theme.warning_banner_background,
            theme.warning_banner_border,
            "warning-filled",
            theme.warning_icon,
            theme.foreground,
        )
    } else {
        (
            theme.toast_background,
            theme.toast_border,
            "info-filled",
            theme.toast_icon,
            theme.toast_foreground,
        )
    };
    let toast = commands
        .spawn((
            Name::new(n.name.into_owned()),
            Toast {
                remaining: n.seconds,
                autohide: n.autohide,
            },
            Node {
                position_type: PositionType::Absolute,
                left: at.map_or(Val::Percent(50.0), |p| Val::Px(p.x)),
                top: Val::Px(at.map_or(4.0, |p| p.y)),
                // The warning banner is lower (`screens/15`: 26 px). P3G.4 (P3G.3 carried): a
                // long message wraps onto a second line inside a capped width, so the toast
                // never runs past the graphics area or over the view cube, and its action and ✕
                // stay visible.
                height: if max_width.is_some() { Val::Px(if warning { 26.0 } else { 31.0 }) } else { Val::Auto },
                min_height: Val::Px(if warning { 26.0 } else { 31.0 }),
                max_width: Val::Px(max_width.unwrap_or(DEFAULT_MAX_WIDTH)),
                padding: UiRect::new(Val::Px(15.0), Val::Px(10.0), Val::Px(if max_width.is_some() { 0.0 } else { 4.0 }), Val::Px(if max_width.is_some() { 0.0 } else { 4.0 })),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
                ..default()
            },
            UiTransform::from_translation(Val2::new(
                if at.is_some() { Val::Px(0.0) } else { Val::Percent(-50.0) },
                Val::Px(0.0),
            )),
            BackgroundColor(background),
            BorderColor::all(border),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.12),
                Val::Px(0.0),
                Val::Px(2.0),
                Val::Px(0.0),
                Val::Px(5.0),
            ),
            GlobalZIndex(z::TOOLTIP),
        ))
        .with_children(|p| {
            p.spawn((icon(icon_name, 16.0, icon_color), Pickable::IGNORE))
                .entry::<Node>()
                .and_modify(|mut n| n.flex_shrink = 0.0);
            let mut text = p.spawn((
                Name::new("toast-message"),
                t.text(
                    n.message,
                    t.font_base,
                    if warning { FontWeight::NORMAL } else { FontWeight::MEDIUM },
                    foreground,
                ),
                Pickable::IGNORE,
            ));
            if max_width.is_some() {
                text.insert((Ellipsis::default().with_tooltip(), Ellipsis::node()));
            } else {
                text.insert((
                    Node { flex_shrink: 1.0, ..default() },
                    TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary),
                ));
            }
            p.spawn((
                ToastActions,
                Node {
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    margin: UiRect::left(Val::Px(4.0)),
                    ..default()
                },
            ))
            .with_children(|a| {
                if closable {
                    let mut ghost = visuals_for(&t, crate::ButtonVariant::Ghost);
                    ghost.foreground = StateColors::new(
                        t.toast_foreground,
                        t.foreground,
                        t.foreground,
                        t.disabled_foreground,
                    );
                    ghost.background = StateColors::all(Color::NONE);
                    a.spawn((
                        Button::new("toast-close")
                            .icon("x-bold")
                            .icon_size(14.0)
                            .ghost()
                            .build(&t),
                        observe(|a: On<Activate>, q: Query<&ChildOf>, qt: Query<(), With<Toast>>, mut commands: Commands| {
                            // The ✕ sits in the actions row of the toast.
                            let mut e = a.entity;
                            while let Ok(parent) = q.get(e) {
                                e = parent.parent();
                                if qt.contains(e) {
                                    commands.entity(e).try_despawn();
                                    break;
                                }
                            }
                        }),
                    ))
                    .insert((
                        ghost,
                        Node {
                            width: Val::Px(18.0),
                            height: Val::Px(18.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ));
                }
            });
        })
        .id();
    commands.queue(move |world: &mut World| {
        let mut q = world.query_filtered::<Entity, With<ToastHost>>();
        if let Some(host) = q.iter(world).next() {
            world.entity_mut(toast).insert(ChildOf(host));
        } else if let Ok(mut e) = world.get_entity_mut(toast)
            && let Some(mut node) = e.get_mut::<Node>()
        {
            // No host: below the window's top bar.
            node.top = Val::Px(44.0);
        }
    });
    toast
}

/// Adds an action link (such as "Undo" or "Restore") to a toast, before its ✕. Returns the
/// link button; observe its `Activate`.
pub fn toast_action(
    commands: &mut Commands,
    theme: &Theme,
    toast: Entity,
    name: impl Into<Cow<'static, str>>,
    label: impl Into<String>,
) -> Entity {
    let mut link: Visuals = visuals_for(theme, crate::ButtonVariant::Link);
    link.foreground = StateColors::new(
        theme.toast_foreground,
        theme.link,
        theme.link,
        theme.disabled_foreground,
    );
    // Underlined like Onshape's action links: a 1 px rule about 4 px under the text.
    link.border = link.foreground;
    let button = commands
        .spawn(Button::new(name).label(label).link().small().build(theme))
        .insert(link)
        .id();
    commands.entity(button).entry::<Node>().and_modify(|mut n| {
        n.height = Val::Auto;
        n.padding = UiRect::new(n.padding.left, n.padding.right, Val::ZERO, Val::Px(1.0));
        n.border = UiRect::bottom(Val::Px(1.0));
        n.border_radius = BorderRadius::ZERO;
    });
    commands.queue(move |world: &mut World| {
        let actions = world
            .get::<Children>(toast)
            .and_then(|c| c.iter().find(|e| world.get::<ToastActions>(*e).is_some()));
        if let Some(actions) = actions {
            world.entity_mut(actions).insert_children(0, &[button]);
        }
        // Action labels are bold in Onshape.
        let mut q = world.query::<(&ChildOf, &mut TextFont)>();
        for (parent, mut font) in q.iter_mut(world) {
            if parent.parent() == button {
                font.weight = FontWeight::BOLD;
            }
        }
    });
    button
}

fn expire_toasts(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Toast)>) {
    for (e, mut t) in &mut q {
        if !t.autohide {
            continue;
        }
        t.remaining -= time.delta_secs();
        if t.remaining <= 0.0 {
            commands.entity(e).try_despawn();
        }
    }
}

/// Auto-hiding toasts also go away at the next click outside them.
fn dismiss_on_click(
    mut inputs: MessageReader<PointerInput>,
    q: Query<(Entity, &Toast, &Hovered)>,
    mut commands: Commands,
) {
    let pressed = inputs
        .read()
        .any(|i| matches!(i.action, PointerAction::Press(_)));
    if !pressed {
        return;
    }
    for (e, t, hovered) in &q {
        if t.autohide && !hovered.get() {
            commands.entity(e).try_despawn();
        }
    }
}
