//! The feature panel's toggle tab stays clear of feature dialogs (P3D.1–P3D.2 judge,
//! `course_insp_error_states` 08): the tab hangs off the panel's right edge, halfway down,
//! where a tall dialog (a fillet with many "Missing Edge" items) would cover it; while one
//! does, the tab moves to just below the dialog, as Onshape's sits below its dialogs
//! (`inspection-and-repair/ex1-step10.png`).

use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;

use crate::AppState;

pub struct PanelTabPlugin;

impl Plugin for PanelTabPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, keep_tab_clear.run_if(in_state(AppState::Document)));
    }
}

/// The tab's height and its default offset from the panel's middle (see `cadrs_ui::dock`).
const TAB_H: f32 = 32.0;
const GAP: f32 = 8.0;

#[allow(clippy::type_complexity)]
fn keep_tab_clear(
    q_named: Query<(&Name, &ComputedNode, &UiGlobalTransform, Option<&ChildOf>), Without<cadrs_ui::FeatureDialogState>>,
    q_dialogs: Query<(&ComputedNode, &UiGlobalTransform), With<cadrs_ui::FeatureDialogState>>,
    mut q_node: Query<(&Name, &mut Node), Without<cadrs_ui::FeatureDialogState>>,
) {
    let rect = |c: &ComputedNode, t: &UiGlobalTransform| {
        let s = c.inverse_scale_factor();
        Rect::from_center_size(t.translation * s, c.size() * s)
    };
    let Some((_, tab_c, tab_t, Some(parent))) = q_named.iter().find(|(n, ..)| n.as_str() == "feature-panel-toggle") else {
        return;
    };
    let Ok((_, panel_c, panel_t, _)) = q_named.get(parent.parent()) else {
        return;
    };
    let panel = rect(panel_c, panel_t);
    let tab = rect(tab_c, tab_t);
    // Where the tab sits by default (halfway down the panel).
    let default_top = panel.center().y - TAB_H / 2.0;
    let below = q_dialogs
        .iter()
        .map(|(c, t)| rect(c, t))
        .filter(|d| d.min.x < tab.max.x && d.max.x > tab.min.x && d.min.y < default_top + TAB_H && d.max.y > default_top)
        .map(|d| d.max.y + GAP)
        .fold(None, |a: Option<f32>, y| Some(a.map_or(y, |a| a.max(y))));
    let (top, margin) = match below {
        Some(y) => (Val::Px(y - panel.min.y), Val::ZERO),
        None => (Val::Percent(50.0), Val::Px(-TAB_H / 2.0)),
    };
    for (n, mut node) in &mut q_node {
        if n.as_str() != "feature-panel-toggle" {
            continue;
        }
        if node.top != top || node.margin.top != margin {
            node.top = top;
            node.margin.top = margin;
        }
    }
}
