//! Radio groups, modeled on gpui-component's `Radio` / `RadioGroup` (`radio.rs`) and the
//! Representation options of Onshape's PCB Component pane: a 13 px ring that shows a blue ring
//! with a blue dot when chosen, followed by a 12 px label, one option per row.
//!
//! Clicking an option (its ring or label) chooses it and triggers [`RadioChange`] on the group
//! with its index; clicking the chosen one again does nothing. The group's [`RadioGroupState`]
//! is the current choice: the owner may set it from code (a controlled value, as in
//! gpui-component), and the rings follow. An option can carry a small trailing icon button
//! ([`RadioGroup::option_icon`]) that sits after the label, outside the clickable part, so
//! clicking it only activates the button.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::InteractionDisabled;

use crate::button::IconButton;
use crate::theme::Theme;

pub struct RadioPlugin;

impl Plugin for RadioPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_click).add_systems(PostUpdate, sync_rings.before(bevy::ui::UiSystems::Prepare));
    }
}

/// The chosen option of a radio group (`None`: nothing chosen).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RadioGroupState {
    pub selected: Option<usize>,
}

/// An option was chosen by the user. Targets the group.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct RadioChange {
    pub entity: Entity,
    pub index: usize,
}

/// The clickable part of an option (ring and label).
#[derive(Component, Debug, Clone, Copy)]
#[require(Hovered)]
pub struct RadioOption {
    pub group: Entity,
    pub index: usize,
}

/// An option's ring; its child dot shows when chosen.
#[derive(Component, Debug, Clone, Copy)]
struct RadioRing {
    group: Entity,
    index: usize,
}

#[derive(Component, Debug, Clone, Copy)]
struct RadioDot {
    group: Entity,
    index: usize,
}

struct OptionSpec {
    name: Cow<'static, str>,
    label: String,
    disabled: bool,
    icon: Option<(Cow<'static, str>, Cow<'static, str>, String)>,
}

/// Builder for a vertical radio group.
pub struct RadioGroup {
    name: Cow<'static, str>,
    options: Vec<OptionSpec>,
    selected: Option<usize>,
    row_height: f32,
}

impl RadioGroup {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self { name: name.into(), options: Vec::new(), selected: None, row_height: 24.0 }
    }

    /// Adds an option; `name` names its clickable row.
    pub fn option(mut self, name: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        self.options.push(OptionSpec { name: name.into(), label: label.into(), disabled: false, icon: None });
        self
    }

    /// Gives the last option a small icon button after its label (named `name`).
    pub fn option_icon(mut self, icon: impl Into<Cow<'static, str>>, name: impl Into<Cow<'static, str>>, tooltip: impl Into<String>) -> Self {
        if let Some(o) = self.options.last_mut() {
            o.icon = Some((icon.into(), name.into(), tooltip.into()));
        }
        self
    }

    /// Greys the last option out.
    pub fn option_disabled(mut self, d: bool) -> Self {
        if let Some(o) = self.options.last_mut() {
            o.disabled = d;
        }
        self
    }

    pub fn selected(mut self, i: Option<usize>) -> Self {
        self.selected = i;
        self
    }

    pub fn row_height(mut self, h: f32) -> Self {
        self.row_height = h;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let RadioGroup { name, options, selected, row_height } = self;
        (
            Name::new(name.into_owned()),
            RadioGroupState { selected },
            Node { flex_direction: FlexDirection::Column, flex_shrink: 0.0, ..default() },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let group = p.target_entity();
                for (index, o) in options.into_iter().enumerate() {
                    let on = selected == Some(index);
                    let fg = if o.disabled { t.tool_disabled_foreground } else { t.foreground };
                    let tt = t.clone();
                    p.spawn(Node { height: Val::Px(row_height), align_items: AlignItems::Center, column_gap: Val::Px(2.0), flex_shrink: 0.0, ..default() }).with_children(|row| {
                        let mut opt = row.spawn((
                            Name::new(o.name.into_owned()),
                            RadioOption { group, index },
                            Node { height: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() },
                        ));
                        if o.disabled {
                            opt.insert(InteractionDisabled);
                        }
                        opt.with_children(|c| {
                            c.spawn((
                                RadioRing { group, index },
                                Node {
                                    width: Val::Px(13.0),
                                    height: Val::Px(13.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::MAX,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BackgroundColor(tt.background),
                                BorderColor::all(ring_color(&tt, on, o.disabled)),
                                Pickable::IGNORE,
                            ))
                            .with_child((
                                RadioDot { group, index },
                                Node { width: Val::Px(7.0), height: Val::Px(7.0), border_radius: BorderRadius::MAX, ..default() },
                                BackgroundColor(ring_color(&tt, true, o.disabled)),
                                if on { Visibility::Inherited } else { Visibility::Hidden },
                                Pickable::IGNORE,
                            ));
                            c.spawn((tt.text(o.label, tt.font_sm, FontWeight::MEDIUM, fg), Pickable::IGNORE));
                        });
                        if let Some((icon, name, tip)) = o.icon {
                            row.spawn(IconButton::new(name, icon).icon_size(13.0).tooltip(tip).build(&tt)).entry::<Node>().and_modify(|mut n| {
                                n.width = Val::Px(20.0);
                                n.min_width = Val::Px(20.0);
                                n.height = Val::Px(20.0);
                            });
                        }
                    });
                }
            })),
        )
    }
}

fn ring_color(t: &Theme, on: bool, disabled: bool) -> Color {
    if disabled {
        t.disabled_foreground
    } else if on {
        t.checkbox_checked
    } else {
        t.checkbox_border
    }
}

fn on_click(mut click: On<Pointer<Click>>, q: Query<(&RadioOption, Has<InteractionDisabled>)>, mut q_group: Query<&mut RadioGroupState>, mut commands: Commands) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok((o, disabled)) = q.get(click.entity) else { return };
    click.propagate(false);
    if disabled {
        return;
    }
    let Ok(mut state) = q_group.get_mut(o.group) else { return };
    if state.selected == Some(o.index) {
        return;
    }
    state.selected = Some(o.index);
    commands.trigger(RadioChange { entity: o.group, index: o.index });
}

fn sync_rings(
    theme: Res<Theme>,
    q_groups: Query<&RadioGroupState, Changed<RadioGroupState>>,
    q_opts: Query<(&RadioOption, Has<InteractionDisabled>)>,
    mut q_ring: Query<(&RadioRing, &mut BorderColor)>,
    mut q_dot: Query<(&RadioDot, &mut Visibility)>,
) {
    if q_groups.is_empty() {
        return;
    }
    let disabled = |g: Entity, i: usize| q_opts.iter().any(|(o, d)| o.group == g && o.index == i && d);
    for (r, mut b) in &mut q_ring {
        if let Ok(s) = q_groups.get(r.group) {
            let c = BorderColor::all(ring_color(&theme, s.selected == Some(r.index), disabled(r.group, r.index)));
            b.set_if_neq(c);
        }
    }
    for (d, mut v) in &mut q_dot {
        if let Ok(s) = q_groups.get(d.group) {
            v.set_if_neq(if s.selected == Some(d.index) { Visibility::Inherited } else { Visibility::Hidden });
        }
    }
}
