//! Long lists that build only the rows near what their scroll area shows, with a spacer above and
//! below standing in for the rest. Bevy lays out, clips and draws every UI node every frame, so
//! a Part Studio of 1,600 features with every row built cost more than the frame. Lists shorter
//! than [`MIN_ROWS`] are built whole, as before.
//!
//! A list's rows container carries [`VirtualRows`]. [`move_windows`] moves its window as the list
//! scrolls (half a page ahead each way, so it moves once every quarter page); the list's owner rebuilds
//! the rows when it changes, through [`Slots`]: every row asks whether it is in the window, and
//! the ones left out add their height to the spacers.

use std::ops::Range;

use bevy::prelude::*;
use bevy::ui::{OverflowAxis, ScrollPosition, UiGlobalTransform};

/// Lists with fewer rows than this are built whole.
pub const MIN_ROWS: usize = 200;
/// The rows built when a long list is first shown, before its window is placed.
const FIRST_ROWS: usize = 100;

pub struct VirtualRowsPlugin;

impl Plugin for VirtualRowsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, move_windows.after(bevy::ui::UiSystems::Layout));
    }
}

/// On a list's rows container: the rows to build.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct VirtualRows {
    /// The rows (by index) to build; `None`: all of them.
    pub window: Option<Range<usize>>,
    /// How many rows the list has, as its owner last built it.
    pub total: usize,
    /// A row's height with its margins (logical px).
    pub pitch: f32,
}

impl VirtualRows {
    pub fn new(pitch: f32) -> Self {
        Self {
            window: None,
            total: 0,
            pitch,
        }
    }

    /// The window to build `total` rows through: the one placed, or the first rows of a long list
    /// not placed yet.
    pub fn window_for(&self, total: usize) -> Option<Range<usize>> {
        if total < MIN_ROWS {
            return None;
        }
        Some(match &self.window {
            Some(w) => w.start.min(total)..w.end.min(total),
            None => 0..FIRST_ROWS.min(total),
        })
    }
}

/// Builds a list's rows through its window.
pub struct Slots {
    window: Option<Range<usize>>,
    next: usize,
    above: f32,
    below: f32,
}

impl Slots {
    pub fn new(window: Option<Range<usize>>) -> Self {
        Self {
            window,
            next: 0,
            above: 0.0,
            below: 0.0,
        }
    }

    /// Whether to build the next row, `height` tall with its margins; one left out adds its
    /// height to a spacer.
    pub fn take(&mut self, height: f32) -> bool {
        let i = self.next;
        self.next += 1;
        match &self.window {
            Some(w) if i < w.start => {
                self.above += height;
                false
            }
            Some(w) if i >= w.end => {
                self.below += height;
                false
            }
            _ => true,
        }
    }

    /// The index the next row will have.
    pub fn next_index(&self) -> usize {
        self.next
    }

    /// How many rows were counted.
    pub fn count(&self) -> usize {
        self.next
    }

    /// The spacers' heights: above the built rows and below them.
    pub fn spacers(&self) -> (f32, f32) {
        (self.above, self.below)
    }
}

/// A spacer standing in for rows not built.
pub fn spacer(name: &'static str, height: f32) -> impl Bundle {
    (
        Name::new(name),
        Node {
            height: Val::Px(height),
            flex_shrink: 0.0,
            ..default()
        },
    )
}

/// The rows of a container `top` (logical px, on screen) that a scroll area from `view_top`
/// `view_h` tall shows, one `pitch` each, of `total`.
fn shown_rows(top: f32, view_top: f32, view_h: f32, pitch: f32, total: usize) -> Range<usize> {
    let from = ((view_top - top) / pitch).floor().max(0.0) as usize;
    let to = ((view_top + view_h - top) / pitch).ceil().max(0.0) as usize;
    from.min(total)..to.min(total)
}

/// The window for `shown`: kept while it holds a quarter page either side, else half a page
/// either side (rows built off screen still cost the frame their text).
fn place_window(
    current: Option<&Range<usize>>,
    shown: Range<usize>,
    page: usize,
    total: usize,
) -> Range<usize> {
    let (half, quarter) = (page.div_ceil(2), page.div_ceil(4));
    let need = shown.start.saturating_sub(quarter)..(shown.end + quarter).min(total);
    match current {
        Some(w) if w.start <= need.start && need.end <= w.end => w.clone(),
        _ => shown.start.saturating_sub(half)..(shown.end + half).min(total),
    }
}

/// Moves each long list's window to keep what its scroll area shows built (from the layout just
/// computed: the owner rebuilds the rows next frame).
pub fn move_windows(
    mut q: Query<(
        Entity,
        &mut VirtualRows,
        &Node,
        &ComputedNode,
        &UiGlobalTransform,
        &ScrollPosition,
    )>,
    parents: Query<&ChildOf>,
    areas: Query<(&Node, &ComputedNode, &UiGlobalTransform)>,
) {
    let rect = |n: &ComputedNode, at: &UiGlobalTransform| {
        let s = n.inverse_scale_factor();
        let h = n.size().y * s;
        (at.translation.y * s - h / 2.0, h)
    };
    for (e, mut rows, own, node, at, scroll) in &mut q {
        if rows.total < MIN_ROWS {
            if rows.window.is_some() {
                rows.window = None;
            }
            continue;
        }
        // Where the rows start on screen, and what shows them: the container itself when it
        // scrolls them, else the nearest ancestor that scrolls (every node has a
        // `ScrollPosition`; that one's overflow scrolls).
        let (own_top, own_h) = rect(node, at);
        let (top, (view_top, view_h)) = if own.overflow.y == OverflowAxis::Scroll {
            (own_top - scroll.y, (own_top, own_h))
        } else {
            let Some((_, area, area_at)) = parents
                .iter_ancestors(e)
                .filter_map(|a| areas.get(a).ok())
                .find(|(n, ..)| n.overflow.y == OverflowAxis::Scroll)
            else {
                continue;
            };
            (own_top, rect(area, area_at))
        };
        if view_h <= 0.0 {
            continue;
        }
        let shown = shown_rows(top, view_top, view_h, rows.pitch, rows.total);
        let page = (view_h / rows.pitch).ceil() as usize;
        let window = place_window(rows.window.as_ref(), shown, page, rows.total);
        if rows.window.as_ref() != Some(&window) {
            rows.window = Some(window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_lists_are_built_whole() {
        let rows = VirtualRows::new(24.0);
        assert_eq!(rows.window_for(MIN_ROWS - 1), None);
        assert_eq!(rows.window_for(MIN_ROWS), Some(0..FIRST_ROWS));
    }

    #[test]
    fn rows_left_out_become_spacers() {
        let mut slots = Slots::new(Some(2..4));
        let built: Vec<bool> = (0..6).map(|_| slots.take(24.0)).collect();
        assert_eq!(built, [false, false, true, true, false, false]);
        assert_eq!(slots.spacers(), (48.0, 48.0));
        assert_eq!(slots.count(), 6);
    }

    #[test]
    fn the_window_moves_only_near_its_edges() {
        // 40 rows shown from row 100, of 1,000: half a page (20 rows) either side.
        let w = place_window(None, 100..140, 40, 1000);
        assert_eq!(w, 80..160);
        // Scrolled 10 rows: still a quarter page either side.
        assert_eq!(place_window(Some(&w), 110..150, 40, 1000), w);
        // Scrolled 15 rows: moved.
        assert_eq!(place_window(Some(&w), 115..155, 40, 1000), 95..175);
    }

    #[test]
    fn shown_rows_follow_the_scroll() {
        // A container whose top is 480 px above the view: rows 20 to 30 in a 240 px view.
        assert_eq!(shown_rows(-480.0, 0.0, 240.0, 24.0, 1000), 20..30);
        // Past the end.
        assert_eq!(shown_rows(-480.0, 0.0, 240.0, 24.0, 25), 20..25);
    }
}
