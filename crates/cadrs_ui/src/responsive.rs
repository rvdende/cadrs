//! Nodes shown only on a window at least so wide: a top bar's buttons fold into a menu on a
//! phone's screen. Bevy's UI has no media queries, so [`HideBelow`] compares the width of the
//! node's render target, before each layout.

use bevy::prelude::*;
use bevy::ui::{ComputedUiRenderTargetInfo, UiSystems};

pub struct ResponsivePlugin;

impl Plugin for ResponsivePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            hide_below
                .after(UiSystems::Propagate)
                .before(UiSystems::Layout),
        );
    }
}

/// Hides the node while its window (the UI's logical width) is narrower than this many px.
/// A node with UI children also hides while none of them is shown, so a wrapper whose button
/// hides itself (the network button where cadrs can't go online) takes no room either.
#[derive(Component, Debug, Clone, Copy)]
pub struct HideBelow(pub f32);

impl HideBelow {
    /// Whether a window this wide (logical px) hides it.
    pub fn hides(self, width: f32) -> bool {
        width < self.0
    }
}

fn hide_below(
    mut q: Query<(
        &HideBelow,
        &ComputedUiRenderTargetInfo,
        Option<&Children>,
        &mut Node,
    )>,
    q_child: Query<&Node, Without<HideBelow>>,
) {
    for (hide, target, children, mut node) in &mut q {
        // Not yet on a target: leave it as spawned.
        if target.physical_size().x == 0 {
            continue;
        }
        let mut child_nodes = children
            .into_iter()
            .flatten()
            .filter_map(|c| q_child.get(*c).ok())
            .peekable();
        let empty = child_nodes.peek().is_some() && child_nodes.all(|n| n.display == Display::None);
        let display = if hide.hides(target.logical_size().x) || empty {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
}
