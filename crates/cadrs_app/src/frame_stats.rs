//! Frame rate: the real time between frames over the last couple of seconds ([`FrameStats`]),
//! shown at the bottom left of the 3D view with where the frame's time goes
//! ([`crate::frame_profile`]): the main thread's schedules as one bar, the slowest systems and
//! the slowest GPU passes as bars of the frame. Preferences › Interface › *Frame stats* picks
//! nothing (the default), the "60 fps · 16.7 ms" line alone, or the whole panel (#69;
//! [`cadrs_core::preferences::FrameStatsMode`]). The MCP tool `get_frame_stats` ([`crate::mcp`])
//! returns the same whatever is shown, so a slow view can be measured at any time.

use std::collections::VecDeque;

use bevy::prelude::*;
use cadrs_core::preferences::FrameStatsMode;
use cadrs_ui::Theme;
use serde_json::{Value, json};

use crate::AppState;
use crate::frame_profile::{CpuProfiling, FrameProfile, Row, short_name};
use crate::preferences_ui::LocalPreferences;

/// How far back the stats look.
const WINDOW_SECS: f64 = 2.0;
/// How often the panel changes (a number that changes every frame can't be read).
const LABEL_EVERY_SECS: f32 = 0.5;
/// The panel's bars at a whole frame (px).
const BAR_PX: f32 = 260.0;
/// The system and GPU rows shown.
const SYSTEM_ROWS: usize = 8;
const GPU_ROWS: usize = 4;

pub struct FrameStatsPlugin;

impl Plugin for FrameStatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameStats>()
            .add_systems(First, record_frame)
            .add_systems(Update, sync_panel.run_if(in_state(AppState::Document)));
    }
}

/// The last [`WINDOW_SECS`] of frames: when each ended (s since start) and how long it took (ms).
#[derive(Resource, Debug, Default)]
pub struct FrameStats {
    frames: VecDeque<(f64, f32)>,
}

impl FrameStats {
    pub fn push(&mut self, now: f64, ms: f32) {
        self.frames.push_back((now, ms));
        while self
            .frames
            .front()
            .is_some_and(|(t, _)| now - t > WINDOW_SECS)
        {
            self.frames.pop_front();
        }
    }

    /// Frames per second over the window (frames / the time they took).
    pub fn fps(&self) -> f32 {
        let total: f32 = self.frames.iter().map(|(_, ms)| ms).sum();
        if total > 0.0 {
            self.frames.len() as f32 * 1000.0 / total
        } else {
            0.0
        }
    }

    /// Frame times in ms over the window: (mean, median, 95th percentile, max).
    pub fn frame_ms(&self) -> (f32, f32, f32, f32) {
        let mut ms: Vec<f32> = self.frames.iter().map(|(_, ms)| *ms).collect();
        if ms.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        ms.sort_by(f32::total_cmp);
        let at = |q: f32| ms[((ms.len() - 1) as f32 * q).round() as usize];
        (
            ms.iter().sum::<f32>() / ms.len() as f32,
            at(0.5),
            at(0.95),
            ms[ms.len() - 1],
        )
    }

    /// What `get_frame_stats` returns.
    pub fn describe(&self) -> Value {
        let (mean, median, p95, max) = self.frame_ms();
        let r = |x: f32| (x as f64 * 10.0).round() / 10.0;
        json!({
            "fps": r(self.fps()),
            "frames": self.frames.len(),
            "window_s": WINDOW_SECS,
            "frame_ms": { "mean": r(mean), "median": r(median), "p95": r(p95), "max": r(max) },
        })
    }

    fn label(&self) -> String {
        let (mean, _, _, max) = self.frame_ms();
        format!("{:.0} fps · {mean:.1} ms (max {max:.0})", self.fps())
    }
}

fn record_frame(time: Res<Time<Real>>, mut stats: ResMut<FrameStats>) {
    let ms = time.delta_secs() * 1000.0;
    if ms > 0.0 {
        stats.push(time.elapsed_secs_f64(), ms);
    }
}

#[derive(Component)]
struct FrameStatsPanel;

#[derive(Component)]
struct FrameStatsText;

/// The bars under the frame rate, rebuilt as they change.
#[derive(Component)]
struct FrameProfileRows;

/// Spawns the panel into the viewport area (shown by [`sync_panel`]).
pub fn spawn_frame_stats(p: &mut ChildSpawnerCommands, t: &Theme) {
    p.spawn((
        Name::new("frame-stats"),
        FrameStatsPanel,
        // A small glass chip at the view's bottom left, like the view tools at its right.
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(0.0),
            left: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            padding: UiRect::axes(Val::Px(t.space[3]), Val::Px(t.space[2])),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius_panel)),
            max_width: Val::Px(BAR_PX + 2.0 * t.space[3]),
            // Long system names are cut at the panel's edge (#69).
            overflow: Overflow::clip(),
            display: Display::None,
            ..default()
        },
        cadrs_ui::glass::panel(t),
        Pickable::IGNORE,
    ))
    .with_children(|panel| {
        panel
            .spawn((
                Node {
                    column_gap: Val::Px(t.space[2]),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_child((
                FrameStatsText,
                t.text("", t.font_sm, FontWeight::NORMAL, t.foreground),
                Pickable::IGNORE,
            ));
        panel.spawn((
            Name::new("frame-profile"),
            FrameProfileRows,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.0),
                ..default()
            },
            Pickable::IGNORE,
        ));
    });
}

/// Colours for the main loop's schedules, in turn.
const SCHEDULE_COLORS: [(u8, u8, u8); 6] = [
    (0x4e, 0x79, 0xa7),
    (0xf2, 0x8e, 0x2b),
    (0x59, 0xa1, 0x4f),
    (0xe1, 0x57, 0x59),
    (0x76, 0xb7, 0xb2),
    (0xb0, 0x7a, 0xa1),
];

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_panel(
    stats: Res<FrameStats>,
    prefs: Res<LocalPreferences>,
    profile: Res<FrameProfile>,
    time: Res<Time<Real>>,
    theme: Res<Theme>,
    mut last: Local<(f32, Option<FrameStatsMode>)>,
    mut commands: Commands,
    mut q_panel: Query<&mut Node, With<FrameStatsPanel>>,
    mut q_text: Query<&mut Text, With<FrameStatsText>>,
    q_rows: Query<Entity, With<FrameProfileRows>>,
    cpu: Res<CpuProfiling>,
    q_over: Query<
        (),
        Or<(
            With<cadrs_ui::menu::MenuDismissLayer>,
            With<cadrs_ui::name_popup::NamePopupRoot>,
            With<crate::tab_preview::PreviewCard>,
            With<crate::tab_folders::OverflowPanel>,
        )>,
    >,
) {
    // Out of the way while a menu, a name popup or a tab's card is up over it (a tab folder's
    // list left slivers of it showing either side).
    let mode = prefs.0.frame_stats;
    let display = if mode == FrameStatsMode::Off || !q_over.is_empty() {
        Display::None
    } else {
        Display::Flex
    };
    for mut node in &mut q_panel {
        if node.display != display {
            node.display = display;
        }
    }
    // Not on `stats` changing: the frame's time is written to it every frame.
    let now = time.elapsed_secs();
    if mode == FrameStatsMode::Off || (last.1 == Some(mode) && now - last.0 < LABEL_EVERY_SECS) {
        return;
    }
    *last = (now, Some(mode));
    for mut text in &mut q_text {
        text.0 = stats.label();
    }
    let t = &*theme;
    for rows in &q_rows {
        commands.entity(rows).despawn_children();
        if mode != FrameStatsMode::Details {
            continue;
        }
        let s = profile.summary();
        let frame = s.frame_ms.max(1e-3);
        let px = |ms: f32| (ms / frame * BAR_PX).clamp(1.0, BAR_PX);
        let small = |text: String, color: Color| {
            (
                t.text(text, t.font_sm - 1.0, FontWeight::NORMAL, color),
                Pickable::IGNORE,
            )
        };
        commands.entity(rows).with_children(|r| {
            // Without the CPU times (off unless wanted: `CpuProfiling`), why not.
            if let Some(why) = cpu.why_not() {
                r.spawn((
                    t.text(
                        format!("CPU times: {why}."),
                        t.font_sm - 1.0,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Node {
                        max_width: Val::Px(BAR_PX),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                // Wrapped at the bars' width (the theme's text doesn't wrap).
                .insert(TextLayout::default());
            } else {
                // The main thread: one bar of the loop's schedules, then their names.
                r.spawn(small("CPU · main loop".into(), t.muted_foreground));
                r.spawn((
                    Node {
                        width: Val::Px(BAR_PX),
                        height: Val::Px(8.0),
                        ..default()
                    },
                    BackgroundColor(t.secondary),
                    Pickable::IGNORE,
                ))
                .with_children(|bar| {
                    for (i, row) in s.main.iter().enumerate() {
                        let (cr, cg, cb) = SCHEDULE_COLORS[i % SCHEDULE_COLORS.len()];
                        bar.spawn((
                            Node {
                                width: Val::Px(px(row.ms)),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(Color::srgb_u8(cr, cg, cb)),
                            Pickable::IGNORE,
                        ));
                    }
                });
                let mut by_time: Vec<(usize, &Row)> = s.main.iter().enumerate().collect();
                by_time.sort_by(|a, b| b.1.ms.total_cmp(&a.1.ms));
                r.spawn(Node {
                    column_gap: Val::Px(6.0),
                    flex_wrap: FlexWrap::Wrap,
                    max_width: Val::Px(BAR_PX),
                    ..default()
                })
                .insert(Pickable::IGNORE)
                .with_children(|legend| {
                    for (i, row) in by_time.into_iter().take(4) {
                        let (cr, cg, cb) = SCHEDULE_COLORS[i % SCHEDULE_COLORS.len()];
                        legend.spawn(small(
                            format!("{} {:.1}", row.name, row.ms),
                            Color::srgb_u8(cr, cg, cb),
                        ));
                    }
                });
                // The render thread, on the same scale.
                let render_ms: f32 = s.render.iter().map(|r| r.ms).sum();
                r.spawn((
                    Node {
                        column_gap: Val::Px(6.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|line| {
                    line.spawn((
                        Node {
                            width: Val::Px(BAR_PX * 0.7),
                            height: Val::Px(6.0),
                            ..default()
                        },
                        BackgroundColor(t.secondary),
                        Pickable::IGNORE,
                    ))
                    .with_child((
                        Node {
                            width: Val::Px(px(render_ms) * 0.7),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgb_u8(0x9c, 0x75, 0x5f)),
                        Pickable::IGNORE,
                    ));
                    line.spawn(small(
                        format!("render thread {render_ms:.1}"),
                        t.muted_foreground,
                    ));
                });
            }
            let bar_row = |r: &mut ChildSpawnerCommands, row: &Row, name: String, color: Color| {
                r.spawn((
                    Node {
                        column_gap: Val::Px(6.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|line| {
                    line.spawn((
                        Node {
                            width: Val::Px(BAR_PX / 4.0),
                            height: Val::Px(6.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(t.secondary),
                        Pickable::IGNORE,
                    ))
                    .with_child((
                        Node {
                            width: Val::Px(px(row.ms) / 4.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(color),
                        Pickable::IGNORE,
                    ));
                    line.spawn(small(format!("{:>6.1} {name}", row.ms), t.foreground));
                });
            };
            if !s.systems.is_empty() {
                r.spawn(small(
                    "CPU · systems (ms a frame)".into(),
                    t.muted_foreground,
                ));
                for row in s.systems.iter().take(SYSTEM_ROWS) {
                    bar_row(r, row, short_name(&row.name), t.primary);
                }
            }
            let verts: f32 = s.gizmos.iter().map(|g| g.ms).sum();
            if let Some(top) = s.gizmos.first() {
                r.spawn(small(
                    format!(
                        "gizmos {:.0}k vertices, most {} {:.0}k",
                        verts / 1e3,
                        short_name(&top.name)
                            .rsplit("::")
                            .next()
                            .unwrap_or_default(),
                        top.ms / 1e3
                    ),
                    t.muted_foreground,
                ));
            }
            if s.gpu.is_empty() {
                r.spawn(small("GPU: not measured".into(), t.muted_foreground));
            } else {
                r.spawn(small(
                    format!(
                        "GPU · {:.1} ms",
                        s.gpu
                            .iter()
                            .filter(|g| !g.name.contains('/'))
                            .map(|g| g.ms)
                            .sum::<f32>()
                    ),
                    t.muted_foreground,
                ));
                for row in s.gpu.iter().take(GPU_ROWS) {
                    bar_row(r, row, row.name.clone(), Color::srgb_u8(0x59, 0xa1, 0x4f));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_cover_the_last_two_seconds() {
        let mut s = FrameStats::default();
        // Three seconds at 10 ms a frame, then one 100 ms frame.
        for i in 1..=300 {
            s.push(i as f64 * 0.01, 10.0);
        }
        s.push(3.105, 100.0);
        // The window is [1.105, 3.105]: 190 frames of 10 ms and the 100 ms one.
        assert_eq!(s.frames.len(), 191);
        let (mean, median, p95, max) = s.frame_ms();
        assert_eq!((median, p95, max), (10.0, 10.0, 100.0));
        assert!((mean - 2000.0 / 191.0).abs() < 1e-3);
        assert!((s.fps() - 191.0 / 2.0).abs() < 1e-3);
        assert_eq!(s.describe()["frames"], 191);
    }
}
