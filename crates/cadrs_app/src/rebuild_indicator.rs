//! The "Rebuilding… 12 / 45" pill at the top of the 3D view: shown while a Part Studio rebuild
//! runs on the rebuild thread for longer than a moment (opening a big document, an edit early
//! in a long feature list), so the view isn't just empty or stale without a word. The count is
//! the part features done of those in the rebuild ([`cadrs_core::rebuild::progress`]).

use bevy::prelude::*;
use cadrs_ui::Theme;
use cadrs_ui::spinner::Spinner;

use crate::AppState;
use crate::parts::PartCache;

/// How long a rebuild runs before the pill shows (quick ones never flash it).
const SHOW_AFTER_SECS: f32 = 0.4;

pub struct RebuildIndicatorPlugin;

impl Plugin for RebuildIndicatorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_indicator.run_if(in_state(AppState::Document)));
    }
}

#[derive(Component)]
struct RebuildIndicator;

#[derive(Component)]
struct RebuildIndicatorText;

/// Spawns the (hidden) pill into the viewport area.
pub fn spawn_rebuild_indicator(p: &mut ChildSpawnerCommands, t: &Theme) {
    p.spawn((
        Name::new("rebuild-indicator"),
        RebuildIndicator,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(t.space[3]),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            display: Display::None,
            ..default()
        },
        Pickable::IGNORE,
    ))
    .with_children(|row| {
        row.spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(t.space[2]),
                padding: UiRect::axes(Val::Px(t.space[3]), Val::Px(t.space[1])),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius_lg)),
                ..default()
            },
            BackgroundColor(t.popover),
            BorderColor::all(t.border),
            Pickable::IGNORE,
        ))
        .with_children(|pill| {
            pill.spawn(Spinner::new("rebuild-indicator-spinner").size(14.0).thickness(2.0).build(t));
            pill.spawn((
                Name::new("rebuild-indicator-text"),
                RebuildIndicatorText,
                t.text("Rebuilding…", t.font_sm, FontWeight::NORMAL, t.foreground),
                Pickable::IGNORE,
            ));
        });
    });
}

fn sync_indicator(
    cache: Res<PartCache>,
    time: Res<Time<Real>>,
    mut since: Local<Option<f32>>,
    mut q: Query<&mut Node, With<RebuildIndicator>>,
    mut q_text: Query<&mut Text, With<RebuildIndicatorText>>,
) {
    let now = time.elapsed_secs();
    let show = if cache.rebuilding {
        now - *since.get_or_insert(now) >= SHOW_AFTER_SECS
    } else {
        *since = None;
        false
    };
    let display = if show { Display::Flex } else { Display::None };
    for mut n in &mut q {
        if n.display != display {
            n.display = display;
        }
    }
    if show {
        let (done, total) = cadrs_core::rebuild::progress();
        let label = if total > 0 {
            format!("Rebuilding… {} / {total}", done.min(total))
        } else {
            "Rebuilding…".to_string()
        };
        for mut text in &mut q_text {
            if text.0 != label {
                text.0 = label.clone();
            }
        }
    }
}
