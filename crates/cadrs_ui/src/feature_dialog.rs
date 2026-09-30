//! Feature dialogs, like Onshape's ("Sketch 1", "Extrude 1"; `reference/onshape/screens/07`,
//! `08c`, `22`): a small non-modal card floating at the top left of the viewport. The header
//! has the feature name (bold, dark red while the feature is invalid), a green ✓ that accepts
//! (pale green and disabled while invalid) and a red ✕ that cancels. The body holds the
//! feature's parameters (selection fields, checkboxes, …) and an optional footer row of small
//! icon buttons on the right.
//!
//! The ✓ and ✕ are named `<name>-accept` and `<name>-cancel`; clicking them triggers
//! [`FeatureDialogAccept`] or [`FeatureDialogCancel`] on the dialog. Enter and Escape are left to
//! the app, because what Escape does depends on the active tool. Update the title and validity
//! through [`FeatureDialogState`].

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, observe};

use crate::button::{Button, visuals_for};
use crate::style::StateColors;
use crate::theme::Theme;

pub struct FeatureDialogPlugin;

impl Plugin for FeatureDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            sync_feature_dialogs.before(bevy::ui::UiSystems::Prepare),
        );
    }
}

/// The dialog's title and whether the feature can be accepted.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct FeatureDialogState {
    pub title: String,
    pub valid: bool,
    /// The feature has errors but can still be accepted (a sketch with conflicting
    /// constraints): the title is red, the ✓ stays enabled.
    pub error: bool,
}

/// The ✓ was clicked. Targets the dialog.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct FeatureDialogAccept {
    pub entity: Entity,
}

/// The ✕ was clicked. Targets the dialog.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct FeatureDialogCancel {
    pub entity: Entity,
}

#[derive(Component, Debug, Clone, Copy)]
struct DialogTitle(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct AcceptButton(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct CancelButton(Entity);

type SpawnFn = Box<dyn FnOnce(&mut ChildSpawner) + Send + Sync>;

/// A panel that isn't a feature (Mass properties): its title stays black whatever `valid` says.
#[derive(Component, Debug, Clone, Copy)]
struct PlainTitle;

/// Builder for a feature dialog.
pub struct FeatureDialog {
    name: Cow<'static, str>,
    title: String,
    valid: bool,
    width: f32,
    body_padding: UiRect,
    body: Option<SpawnFn>,
    footer: Option<SpawnFn>,
    plain_title: bool,
    no_accept: bool,
}

impl FeatureDialog {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            title: String::new(),
            valid: true,
            width: 202.0,
            body_padding: UiRect::new(Val::Px(2.0), Val::Px(3.0), Val::Px(5.0), Val::ZERO),
            body: None,
            footer: None,
            plain_title: false,
            no_accept: false,
        }
    }

    /// Only the ✕ in the header, no ✓ (a panel that acts through its own buttons, such as
    /// Create selection, `ex4-step15.png`).
    pub fn no_accept(mut self) -> Self {
        self.no_accept = true;
        self
    }

    /// The title stays black (a panel such as Mass properties, not a feature).
    pub fn plain_title(mut self) -> Self {
        self.plain_title = true;
        self
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }

    /// False shows the title in red and disables the ✓.
    pub fn valid(mut self, v: bool) -> Self {
        self.valid = v;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// The padding around the parameters (default 2 px left, 3 px right, 5 px top). Dialogs
    /// with tab rows under the header use none and pad their other rows themselves.
    pub fn body_padding(mut self, p: UiRect) -> Self {
        self.body_padding = p;
        self
    }

    /// Spawns the parameters (laid out in a column).
    pub fn body(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.body = Some(Box::new(f));
        self
    }

    /// Spawns the footer's icon buttons (right-aligned).
    pub fn footer(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.footer = Some(Box::new(f));
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let FeatureDialog {
            name,
            title,
            valid,
            width,
            body_padding,
            body,
            footer,
            plain_title,
            no_accept,
        } = self;
        let accept_name = format!("{name}-accept");
        let cancel_name = format!("{name}-cancel");
        let title_name = format!("{name}-title");
        (
            Name::new(name.into_owned()),
            FeatureDialogState {
                title: title.clone(),
                valid,
                error: false,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(2.0),
                width: Val::Px(width),
                flex_direction: FlexDirection::Column,
                border: UiRect::new(Val::ZERO, Val::Px(1.0), Val::Px(1.0), Val::Px(1.0)),
                border_radius: BorderRadius::right(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(t.background),
            BorderColor::all(Color::srgb_u8(0xe4, 0xe4, 0xe4)),
            // A soft shadow that reaches about #dbdbdb just under the dialog and fades out
            // over ~10 px (`screens/22`).
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.5),
                Val::Px(1.0),
                Val::Px(2.0),
                Val::Px(0.0),
                Val::Px(6.0),
            ),
            GlobalZIndex(crate::z::DIALOG - 10),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let dialog = p.target_entity();
                // Header: title, ✓, ✕.
                p.spawn((
                    Node {
                        height: Val::Px(26.0),
                        flex_shrink: 0.0,
                        padding: UiRect::left(Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb_u8(0xda, 0xda, 0xdc)),
                ))
                .with_children(|h| {
                    let color = if valid || plain_title { t.foreground } else { t.feature_error };
                    // A long title (a hole's callout, P3.6) ends in "…" before the ✓.
                    h.spawn(Node {
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        min_width: Val::Px(0.0),
                        overflow: Overflow::clip(),
                        ..default()
                    })
                    .with_children(|clip| {
                        let mut title_entity = clip.spawn((
                            Name::new(title_name),
                            DialogTitle(dialog),
                            t.text(title, t.font_base, FontWeight::BOLD, color),
                            crate::ellipsis::Ellipsis::node(),
                            crate::ellipsis::Ellipsis::default(),
                        ));
                        title_entity.insert(TextLayout::no_wrap());
                        if plain_title {
                            title_entity.insert(PlainTitle);
                        }
                    });
                    let accept = accept_visuals(&t);
                    if !no_accept {
                    h.spawn((
                        AcceptButton(dialog),
                        Button::new(accept_name)
                            .icon("check-bold")
                            .icon_size(16.0)
                            .tooltip("Accept (Enter)")
                            .disabled(!valid)
                            .build(&t),
                        observe(|a: On<Activate>, q: Query<&AcceptButton>, mut commands: Commands| {
                            if let Ok(b) = q.get(a.entity) {
                                commands.trigger(FeatureDialogAccept { entity: b.0 });
                            }
                        }),
                    ))
                    .insert((
                        accept,
                        Node {
                            width: Val::Px(26.0),
                            height: Val::Px(26.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ));
                    }
                    let mut cancel = visuals_for(&t, crate::ButtonVariant::Ghost);
                    cancel.foreground = StateColors::all(t.cancel);
                    h.spawn((
                        CancelButton(dialog),
                        Button::new(cancel_name)
                            .icon("x-bold")
                            .icon_size(16.0)
                            .tooltip("Cancel (Esc)")
                            .build(&t),
                        observe(|a: On<Activate>, q: Query<&CancelButton>, mut commands: Commands| {
                            if let Ok(b) = q.get(a.entity) {
                                commands.trigger(FeatureDialogCancel { entity: b.0 });
                            }
                        }),
                    ))
                    .insert((
                        cancel,
                        Node {
                            width: Val::Px(24.0),
                            height: Val::Px(26.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ));
                });
                // Body.
                p.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    padding: body_padding,
                    ..default()
                })
                .with_children(|b| {
                    if let Some(f) = body {
                        f(b);
                    }
                });
                // Footer.
                p.spawn(Node {
                    height: Val::Px(26.0),
                    flex_shrink: 0.0,
                    padding: UiRect::horizontal(Val::Px(4.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::FlexEnd,
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|f| {
                    if let Some(footer) = footer {
                        footer(f);
                    }
                });
            })),
        )
    }
}

fn accept_visuals(t: &Theme) -> crate::Visuals {
    crate::Visuals {
        background: StateColors::new(
            t.accept,
            t.accept.darker(0.04),
            t.accept.darker(0.08),
            t.accept_disabled,
        ),
        border: StateColors::all(Color::NONE),
        foreground: StateColors::all(Color::WHITE),
        focus_ring: t.focus_ring,
    }
}

#[allow(clippy::type_complexity)]
fn sync_feature_dialogs(
    theme: Res<Theme>,
    q: Query<(Entity, &FeatureDialogState), Changed<FeatureDialogState>>,
    mut q_title: Query<(
        &DialogTitle,
        &mut Text,
        &mut TextColor,
        Has<PlainTitle>,
        Option<&crate::symbols::SymbolSource>,
        Option<&crate::ellipsis::Ellipsis>,
    )>,
    q_accept: Query<(Entity, &AcceptButton, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    for (e, s) in &q {
        for (title, mut text, mut color, plain, source, cut) in &mut q_title {
            if title.0 != e {
                continue;
            }
            // A callout's title is split into spans (its symbols), and a long one is cut off
            // ("…"): compare the whole string.
            let shown = match cut {
                Some(c) if !c.full.is_empty() => &c.full,
                _ => source.map_or(&text.0, |src| &src.0),
            };
            if *shown != s.title {
                text.0 = s.title.clone();
            }
            let c = if plain || (s.valid && !s.error) {
                theme.foreground
            } else {
                theme.feature_error
            };
            color.set_if_neq(TextColor(c));
        }
        for (b, accept, disabled) in &q_accept {
            if accept.0 != e {
                continue;
            }
            if s.valid && disabled {
                commands.entity(b).try_remove::<InteractionDisabled>();
            } else if !s.valid && !disabled {
                commands.entity(b).try_insert(InteractionDisabled);
            }
        }
    }
}
