//! The feature list while a rebuild runs for longer than a moment: each feature it rebuilds
//! shows where it stands, done (as it always looks), busy (a spinner at its end and a pulsing
//! accent at its left edge) or queued (veiled), from [`PartCache::rebuild_steps`]; the parts in
//! the view show how far it has got ([`PartCache`] takes them as it goes). The list scrolls to
//! keep the busy row high in it, the queued rows below in view, until it is scrolled by hand.
//!
//! The rows are spawned again whenever their key changes ([`crate::document::FeatureRowsSet`]),
//! so each one's step is an overlay of its own, added, updated and faded out in place.

use std::collections::HashMap;

use bevy::prelude::*;
use cadrs_core::FeatureId;
use cadrs_ui::Theme;
use cadrs_ui::spinner::Spinner;
use cadrs_ui::spring::MotionSettings;

use crate::AppState;
use crate::document::FeatureRow;
use crate::parts::{PartCache, RebuildStep};
use crate::rebuild_indicator::SHOW_AFTER_SECS;

/// How much a queued row is veiled (the list's background over it, 0–1).
const QUEUED_VEIL: f32 = 0.55;
/// How long the veil takes to come or go (s).
const FADE: f32 = 0.15;
/// The busy row's accent: its width (px), and how often it pulses (per second).
const ACCENT_WIDTH: f32 = 3.0;
const PULSE_HZ: f32 = 1.2;
/// Where the list keeps the busy row: this far down its height (its top edge).
const FOLLOW_AT: f32 = 0.25;
/// How fast the list eases to it (per second; the larger, the sooner).
const FOLLOW_RATE: f32 = 10.0;

pub struct RebuildRowsPlugin;

impl Plugin for RebuildRowsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                sync_steps.after(crate::document::FeatureRowsSet),
                animate_steps.after(sync_steps),
                follow_busy.after(sync_steps),
            )
                .run_if(in_state(AppState::Document)),
        );
    }
}

/// A row's rebuild step, over the row (its last child).
#[derive(Component)]
struct StepOverlay {
    /// `None`: the rebuild is over (the overlay fades out and goes).
    step: Option<RebuildStep>,
    /// The veil's opacity now (it eases to its step's).
    veil: f32,
}

/// What shows only while the row is busy: its accent and its spinner.
#[derive(Component)]
struct BusyMark;

/// The busy row's pulsing accent.
#[derive(Component)]
struct Accent;

/// A busy mark's node, its colour (the accent's) and whether it is the accent.
type BusyMarks = (
    &'static mut Node,
    Option<&'static mut BackgroundColor>,
    Has<Accent>,
);

/// Puts each row's step on it: an overlay for a row of the rebuild running (once it has run
/// as long as the "Rebuilding…" indicator waits), its step updated, and none left showing a
/// step once the rebuild is over.
fn sync_steps(
    mut commands: Commands,
    cache: Res<PartCache>,
    time: Res<Time<Real>>,
    theme: Res<Theme>,
    mut since: Local<Option<f32>>,
    rows: Query<(Entity, &FeatureRow, &Name)>,
    mut overlays: Query<(&ChildOf, &mut StepOverlay)>,
) {
    let now = time.elapsed_secs();
    let steps: HashMap<FeatureId, RebuildStep> = match cache.rebuild_steps() {
        Some(s) if now - *since.get_or_insert(now) >= SHOW_AFTER_SECS => s.into_iter().collect(),
        Some(_) => HashMap::new(),
        None => {
            *since = None;
            HashMap::new()
        }
    };
    let mut shown: HashMap<Entity, Mut<StepOverlay>> = overlays
        .iter_mut()
        .map(|(parent, o)| (parent.parent(), o))
        .collect();
    for (row, FeatureRow(id), name) in &rows {
        let step = steps.get(id).copied();
        if let Some(o) = shown.get_mut(&row) {
            if o.step != step {
                o.step = step;
            }
            continue;
        }
        let Some(step) = step else { continue };
        let busy = step == RebuildStep::Busy;
        let display = |on: bool| if on { Display::Flex } else { Display::None };
        commands.entity(row).with_children(|r| {
            r.spawn((
                Name::new(format!("{name}-step")),
                StepOverlay {
                    step: Some(step),
                    veil: 0.0,
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    ..default()
                },
                BackgroundColor(theme.sidebar.with_alpha(0.0)),
                Pickable::IGNORE,
            ))
            .with_children(|o| {
                o.spawn((
                    Name::new(format!("{name}-busy-accent")),
                    BusyMark,
                    Accent,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        width: Val::Px(ACCENT_WIDTH),
                        display: display(busy),
                        ..default()
                    },
                    BackgroundColor(theme.primary),
                    Pickable::IGNORE,
                ));
                o.spawn((
                    Name::new(format!("{name}-busy")),
                    BusyMark,
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(theme.space[2]),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        align_items: AlignItems::Center,
                        display: display(busy),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|w| {
                    w.spawn(
                        Spinner::new(format!("{name}-spinner"))
                            .size(12.0)
                            .thickness(2.0)
                            .build(&theme),
                    );
                });
            });
        });
    }
}

/// Eases each overlay's veil to its step's, pulses the busy row's accent, shows the busy marks
/// on the busy row alone, and takes an overlay away once the rebuild is over and it has faded.
fn animate_steps(
    mut commands: Commands,
    time: Res<Time<Real>>,
    theme: Res<Theme>,
    motion: Res<MotionSettings>,
    mut overlays: Query<(Entity, &mut StepOverlay, &mut BackgroundColor, &Children)>,
    mut marks: Query<BusyMarks, (With<BusyMark>, Without<StepOverlay>)>,
) {
    let dt = time.delta_secs();
    let pulse = if motion.enabled {
        0.55 + 0.45 * (std::f32::consts::TAU * PULSE_HZ * time.elapsed_secs()).cos()
    } else {
        1.0
    };
    for (e, mut o, mut bg, children) in &mut overlays {
        let target = if o.step == Some(RebuildStep::Queued) {
            QUEUED_VEIL
        } else {
            0.0
        };
        let veil = if motion.enabled {
            let step = QUEUED_VEIL * dt / FADE;
            if o.veil < target {
                (o.veil + step).min(target)
            } else {
                (o.veil - step).max(target)
            }
        } else {
            target
        };
        if veil != o.veil {
            o.veil = veil;
            bg.0 = theme.sidebar.with_alpha(veil);
        }
        if o.step.is_none() && o.veil <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let busy = o.step == Some(RebuildStep::Busy);
        for c in children {
            let Ok((mut node, color, accent)) = marks.get_mut(*c) else {
                continue;
            };
            let display = if busy { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
            if busy
                && accent
                && let Some(mut color) = color
            {
                color.0 = theme.primary.with_alpha(pulse);
            }
        }
    }
}

/// Where the list is following the busy row: the scroll it last set, until the list is scrolled
/// any other way (by hand), when it stops for the rest of the rebuild.
#[derive(Default)]
struct Follow {
    stopped: bool,
    set: Option<f32>,
}

/// Scrolls the feature list to keep the busy row [`FOLLOW_AT`] down it, eased (at once with
/// motion off), so the queued rows after it show below.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn follow_busy(
    time: Res<Time<Real>>,
    motion: Res<MotionSettings>,
    mut follow: Local<Follow>,
    overlays: Query<(&ChildOf, &StepOverlay)>,
    nodes: Query<(&Name, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    rows: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform)>,
    mut lists: Query<(&Name, &ComputedNode, &mut ScrollPosition)>,
    (cache, slots, container): (
        Res<PartCache>,
        Res<crate::document::FeatureRowSlots>,
        Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<crate::document::FeatureRows>>,
    ),
) {
    let busy = overlays
        .iter()
        .find(|(_, o)| o.step == Some(RebuildStep::Busy))
        .map(|(row, _)| row.parent());
    // While the rebuild shows: a busy row out of a long list's window isn't built, so it has no
    // overlay; it is placed by its feature's index.
    let busy_feature = (busy.is_none() && overlays.iter().next().is_some())
        .then(|| cache.rebuild_steps())
        .flatten()
        .and_then(|steps| steps.into_iter().find(|(_, s)| *s == RebuildStep::Busy))
        .map(|(id, _)| id);
    if busy.is_none() && busy_feature.is_none() {
        // The rebuild is over (or not showing yet): the next one follows again.
        *follow = Follow::default();
        return;
    }
    let Some((_, list, mut pos)) = lists
        .iter_mut()
        .find(|(n, ..)| n.as_str() == "feature-scroll")
    else {
        return;
    };
    if follow.set.is_some_and(|set| (pos.y - set).abs() > 1.5) {
        follow.stopped = true;
    }
    if follow.stopped {
        return;
    }
    // The row's top within the scrolled content (the list's logical pixels): from the content's
    // top, laid out in the same pass, so whatever the scroll was then (the view's own position
    // lags the scroll set a frame, and is rounded to the pixel).
    let Some((_, content, content_at)) = nodes.iter().find(|(n, ..)| n.as_str() == "feature-tree")
    else {
        return;
    };
    let s = list.inverse_scale_factor();
    let top = |n: &ComputedNode, at: &bevy::ui::UiGlobalTransform| {
        (at.translation.y - n.size().y / 2.0) * s
    };
    let row_top = match busy {
        Some(busy) => match rows.get(busy) {
            Ok((row, row_at)) if row.size().y > 0.0 => top(row, row_at),
            _ => return,
        },
        None => {
            let Some(row_top) =
                busy_feature
                    .zip(container.single().ok())
                    .and_then(|(id, (n, at))| {
                        crate::document::feature_row_screen_top(id, &slots, n, at)
                    })
            else {
                return;
            };
            row_top
        }
    };
    let offset = row_top - top(content, content_at);
    let max = ((list.content_size().y - list.size().y) * s).max(0.0);
    let target = (offset - FOLLOW_AT * list.size().y * s)
        .round()
        .clamp(0.0, max);
    if (target - pos.y).abs() < 1.0 {
        follow.set = Some(pos.y);
        return;
    }
    let y = if motion.enabled {
        let k = 1.0 - (-FOLLOW_RATE * time.delta_secs()).exp();
        let y = pos.y + (target - pos.y) * k;
        if (target - y).abs() < 1.0 { target } else { y }
    } else {
        target
    };
    pos.y = y;
    follow.set = Some(y);
}
