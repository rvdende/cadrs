//! Working at scale (P3F.3; `essential-tips.md` T1.2, T4.2, X7).
//!
//! - **Tab overflow**: the document's tab strip scrolls sideways (the mouse wheel over it), the
//!   selected tab is kept in view, and ▾ at its right end lists the tabs, to switch to one that
//!   is scrolled out of sight (`crate::tab_folders`).
//! - **Measuring**: every finished rebuild of the active studio is logged ([`RebuildLog`]); the
//!   scenario command `scale-report <label>` writes the log to `perf-<label>-rebuilds.txt` in
//!   the scenario's output folder, and `rebuild-budget <ms>` sets how long a frame waits for a
//!   rebuild (scripted runs wait until it's done; `30` is the interactive setting, so the
//!   frames of a background rebuild can be timed).

use bevy::input::mouse::MouseScrollUnit;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use crate::ActiveDocument;

pub struct ScalePlugin;

impl Plugin for ScalePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RebuildLog>()
            .add_systems(
                PostUpdate,
                (pad_strip_end, keep_active_tab_in_view, sync_tab_overflow)
                    .chain()
                    .after(bevy::ui::UiSystems::Layout),
            )
            .add_observer(on_strip_wheel)
            .add_observer(on_chevron);
    }
}

// ---------------------------------------------------------------------------------------------
// Tab overflow

fn strip_of(e: Entity, q_names: &Query<(&Name, Option<&ChildOf>)>) -> Option<Entity> {
    let mut cur = Some(e);
    for _ in 0..8 {
        let c = cur?;
        let (n, parent) = q_names.get(c).ok()?;
        if n.as_str() == "tab-strip" {
            return Some(c);
        }
        cur = parent.map(|p| p.parent());
    }
    None
}

/// The chevrons at the ends of the tab strip (shown while tabs overflow).
#[derive(Component)]
pub struct TabStripChevron;

/// The strip's tabs: document tabs and (P3E.2) folder tabs, each with the document tab it is.
type StripTabs<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static crate::document::TabButton>,
        &'static ComputedNode,
        &'static UiGlobalTransform,
    ),
    Or<(
        With<crate::document::TabButton>,
        With<crate::tab_folders::FolderTab>,
    )>,
>;

/// The tabs' left edges in the strip's content (logical px from its start) and the strip's
/// scroll range.
fn tab_edges(
    strip: (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
    q_tabs: &StripTabs,
) -> (Vec<f32>, f32) {
    let (node, at, pos) = strip;
    let s = node.inverse_scale_factor();
    let strip_left = at.translation.x * s - node.size().x * s / 2.0;
    let mut edges: Vec<f32> = q_tabs
        .iter()
        .map(|(_, n, t)| t.translation.x * s - n.size().x * s / 2.0 - strip_left + pos.x)
        .map(|x| (x - 1.0).max(0.0))
        .collect();
    edges.sort_by(f32::total_cmp);
    let max = ((node.content_size().x - node.size().x) * s).max(0.0);
    (edges, max)
}

/// The scroll position one tab on from `x` (`forward`: toward the later tabs), on a tab's edge.
pub fn step_to_edge(edges: &[f32], max: f32, x: f32, forward: bool) -> f32 {
    let target = if forward {
        edges.iter().copied().find(|e| *e > x + 1.0).unwrap_or(max)
    } else {
        edges
            .iter()
            .rev()
            .copied()
            .find(|e| *e < x - 1.0)
            .unwrap_or(0.0)
    };
    target.clamp(0.0, max)
}

/// The wheel over the tabs scrolls them sideways, a whole tab at a time (P3F.3–P3F.4 judge).
fn on_strip_wheel(
    mut ev: On<PointerScroll>,
    q_names: Query<(&Name, Option<&ChildOf>)>,
    mut q_strip: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition)>,
    q_tabs: StripTabs,
) {
    let Some(strip) = strip_of(ev.entity, &q_names) else {
        return;
    };
    let Ok((node, at, mut pos)) = q_strip.get_mut(strip) else {
        return;
    };
    ev.propagate(false);
    let d = if ev.x.abs() > ev.y.abs() { ev.x } else { ev.y };
    if d == 0.0 {
        return;
    }
    // A tab per wheel notch (a line), or per 60 px of a touchpad's scroll.
    let steps = match ev.unit {
        MouseScrollUnit::Line => d.abs().ceil(),
        MouseScrollUnit::Pixel => (d.abs() / 60.0).ceil(),
    }
    .clamp(1.0, 64.0) as usize;
    let (edges, max) = tab_edges((node, at, &pos), &q_tabs);
    let mut x = pos.x;
    for _ in 0..steps {
        x = step_to_edge(&edges, max, x, d < 0.0);
    }
    pos.x = x;
}

/// The chevrons step one tab.
fn on_chevron(
    a: On<bevy::ui_widgets::Activate>,
    q: Query<&Name, With<TabStripChevron>>,
    mut q_strip: Query<(
        &Name,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
    )>,
    q_tabs: StripTabs,
) {
    let Ok(name) = q.get(a.entity) else { return };
    // P3E.2: ▾ (`tab-overflow`) is shown with the chevrons but opens a list (`tab_folders`).
    if !matches!(name.as_str(), "tab-strip-prev" | "tab-strip-next") {
        return;
    }
    let forward = name.as_str() == "tab-strip-next";
    let Some((_, node, at, mut pos)) = q_strip.iter_mut().find(|(n, ..)| n.as_str() == "tab-strip")
    else {
        return;
    };
    let (edges, max) = tab_edges((node, at, &pos), &q_tabs);
    pos.x = step_to_edge(&edges, max, pos.x, forward);
}

/// Shows the chevrons while the tabs overflow, keeps the strip's scroll within what it can
/// scroll (a window made wider again would leave it scrolled past its end), and hides a tab cut
/// by either end of the view (no stub of a tab, its name cut through, shows at an end).
#[allow(clippy::type_complexity)]
fn sync_tab_overflow(
    mut q_strip: Query<(
        &Name,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
    )>,
    mut q_chevrons: Query<
        (Entity, &Name, Has<InteractionDisabled>, &mut Node),
        With<TabStripChevron>,
    >,
    mut commands: Commands,
    mut q_tabs: Query<
        (&ComputedNode, &UiGlobalTransform, &mut Visibility),
        Or<(
            With<crate::document::TabButton>,
            With<crate::tab_folders::FolderTab>,
        )>,
    >,
) {
    let Some((_, node, strip_at, mut pos)) =
        q_strip.iter_mut().find(|(n, ..)| n.as_str() == "tab-strip")
    else {
        return;
    };
    let s = node.inverse_scale_factor();
    let (view_l, view_r) = (
        strip_at.translation.x * s - node.size().x * s / 2.0,
        strip_at.translation.x * s + node.size().x * s / 2.0,
    );
    for (n, t, mut v) in &mut q_tabs {
        let (l, r) = (
            t.translation.x * s - n.size().x * s / 2.0,
            t.translation.x * s + n.size().x * s / 2.0,
        );
        let cut = l < view_l - 1.0 || r > view_r + 1.0;
        v.set_if_neq(if cut {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    let max = ((node.content_size().x - node.size().x) * s).max(0.0);
    if pos.x > max + 0.5 {
        pos.x = max;
    }
    // The room the tabs have without the chevrons (they and P3E.2's ▾ take 62 px when shown),
    // so showing them doesn't make the tabs fit and hide them again.
    let chevrons_shown = q_chevrons.iter().any(|(.., n)| n.display != Display::None);
    let room = node.size().x * s + if chevrons_shown { 62.0 } else { 0.0 };
    let overflow = node.content_size().x * s > room + 0.5;
    for (e, name, disabled, mut n) in &mut q_chevrons {
        let d = if overflow {
            Display::Flex
        } else {
            Display::None
        };
        if n.display != d {
            n.display = d;
        }
        // ‹ dims at the strip's start and › at its end: nothing more that way.
        let at_end = match name.as_str() {
            "tab-strip-prev" => pos.x <= 0.5,
            "tab-strip-next" => pos.x >= max - 0.5,
            _ => false,
        };
        if at_end && !disabled {
            commands.entity(e).try_insert(InteractionDisabled);
        } else if !at_end && disabled {
            commands.entity(e).try_remove::<InteractionDisabled>();
        }
    }
}

/// P3E.3a (P3E.2 carried delta: a ~150 px blank band after the left chevron): the strip gets
/// just enough room after its last tab that its scroll range ends on a tab's edge, so scrolled
/// to the end it shows whole tabs from the left (the room is left over after the last tab, as
/// on a bar that isn't full) instead of a cut tab.
#[allow(clippy::type_complexity)]
fn pad_strip_end(
    mut q_strip: Query<
        (
            &ComputedNode,
            &UiGlobalTransform,
            &ScrollPosition,
            &mut Node,
        ),
        With<crate::document::TabStrip>,
    >,
    q_tabs: StripTabs,
) {
    let Some((node, at, pos, mut n)) = q_strip.iter_mut().next() else {
        return;
    };
    let s = node.inverse_scale_factor();
    let pad = match n.padding.right {
        Val::Px(p) => p,
        _ => 0.0,
    };
    let (edges, _) = tab_edges((node, at, pos), &q_tabs);
    let bare_max = (node.content_size().x * s - pad - node.size().x * s).max(0.0);
    let want = if bare_max <= 0.5 {
        0.0
    } else {
        edges
            .iter()
            .copied()
            .find(|e| *e >= bare_max - 0.5)
            .map_or(0.0, |e| (e - bare_max).max(0.0))
    };
    if (want - pad).abs() > 0.5 {
        n.padding.right = Val::Px(want);
    }
}

/// The selected tab scrolls into view when it changes (a tab chosen from ▾ or a folder's list, a new
/// tab at the end).
#[allow(clippy::type_complexity)]
fn keep_active_tab_in_view(
    doc: Option<Res<ActiveDocument>>,
    mut last: Local<Option<(cadrs_core::ElementId, usize, i32)>>,
    q_tabs: StripTabs,
    mut q_strip: Query<(
        &Name,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
    )>,
) {
    let Some(doc) = doc else { return };
    let Some(active) = doc.active else { return };
    // P3E.2: also when the bar opens another folder (the tab's place changes).
    let Some((tab_node, tab_at)) = q_tabs
        .iter()
        .find(|(b, ..)| b.is_some_and(|b| b.0 == active))
        .map(|(_, n, t)| (*n, *t))
    else {
        return;
    };
    let Some((_, strip_node, strip_at, mut pos)) =
        q_strip.iter_mut().find(|(n, ..)| n.as_str() == "tab-strip")
    else {
        return;
    };
    // P3E.2: also when the bar opens another folder (the tab's place changes) and when the
    // strip's width changes (the chevrons appear).
    let key = (
        active,
        doc.doc.elements.len() + q_tabs.iter().count() * 1000,
        strip_node.size().x.round() as i32,
    );
    if *last == Some(key) {
        return;
    }
    let s = strip_node.inverse_scale_factor();
    let (half_strip, half_tab) = (strip_node.size().x * s / 2.0, tab_node.size().x * s / 2.0);
    // The tab's centre relative to the strip's centre, in logical pixels.
    let dx = (tab_at.translation.x - strip_at.translation.x) * s;
    let (left, right) = (dx - half_tab, dx + half_tab);
    if tab_node.size().x <= 0.0 {
        return;
    }
    *last = Some(key);
    let max = ((strip_node.content_size().x - strip_node.size().x) * s).max(0.0);
    // On a tab's edge (the strip scrolls by whole tabs): the tab's own left edge, or the first
    // edge that brings its right edge in.
    let edges: Vec<f32> = q_tabs
        .iter()
        .map(|(_, n, t)| {
            ((t.translation.x - strip_at.translation.x) * s - n.size().x * s / 2.0
                + half_strip
                + pos.x
                - 1.0)
                .max(0.0)
        })
        .collect();
    if left < -half_strip {
        pos.x = (pos.x + left + half_strip - 1.0).clamp(0.0, max);
    } else if right > half_strip {
        let need = pos.x + right - half_strip;
        let mut sorted = edges;
        sorted.sort_by(f32::total_cmp);
        pos.x = sorted
            .into_iter()
            .find(|e| *e >= need - 0.5)
            .unwrap_or(max)
            .clamp(0.0, max);
    }
}

// ---------------------------------------------------------------------------------------------
// Measuring

/// The finished rebuilds of the active studio: (features, computed, elapsed).
#[derive(Resource, Debug, Default)]
pub struct RebuildLog(pub Vec<(usize, usize, std::time::Duration)>);

/// Writes the rebuild log to `perf-<label>-rebuilds.txt` in the scenario output folder and
/// clears it.
pub fn write_report(world: &mut World, label: &str) {
    // Scenarios export to `<out>/exports`; the report goes beside the screenshots.
    let dir = world
        .get_resource::<crate::ExportDirOverride>()
        .and_then(|d| d.0.clone())
        .unwrap_or_else(|| std::path::PathBuf::from("target"));
    let dir = if dir.ends_with("exports") {
        dir.parent().map(|p| p.to_path_buf()).unwrap_or(dir)
    } else {
        dir
    };
    let log = std::mem::take(&mut world.resource_mut::<RebuildLog>().0);
    let mut s = String::new();
    for (features, computed, t) in &log {
        s.push_str(&format!(
            "rebuild of {features} features: {computed} computed in {:.1} ms\n",
            t.as_secs_f64() * 1e3
        ));
    }
    let _ = cadrs_fs::create_dir_all(&dir);
    if let Err(e) = cadrs_fs::write(dir.join(format!("perf-{label}-rebuilds.txt")), s) {
        warn!("scale-report: {e}");
    }
}
