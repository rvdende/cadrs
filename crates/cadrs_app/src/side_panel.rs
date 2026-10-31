//! The side panel docked on the viewport's right (PS2.10, P3.6): one at a time, toggled by its
//! button (an assembly's on the viewport's right strip, the Simulation and Sheet metal ones in
//! the tab bar's tools), which shows pressed while it is open.

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_ui::prelude::*;

use crate::AppState;

pub struct SidePanelPlugin;

impl Plugin for SidePanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SidePanel>()
            .add_systems(
                Update,
                sync_panel_buttons.run_if(in_state(AppState::Document)),
            )
            .add_observer(on_panel_button);
    }
}

/// The open side panel: in an assembly the Bill of Materials (P3B.6,
/// [`crate::assembly::bom_panel`]), the Named positions or the Exploded views; the Simulation
/// panel; in a Part Studio with a sheet metal model the Sheet metal table and flat view.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SidePanel {
    #[default]
    None,
    Bom,
    /// P3B.8: an assembly's Named positions ([`crate::assembly::named_positions`]).
    NamedPositions,
    /// P3B.8: an assembly's Exploded views ([`crate::assembly::explode`]).
    ExplodedViews,
    /// P3F.5: the Simulation panel ([`crate::simulation_ui`]).
    Simulation,
    /// P3I.3: the Sheet metal table and flat view ([`crate::sheetmetal_table`]).
    SheetMetal,
}

fn on_panel_button(a: On<Activate>, q: Query<&Name>, mut open: ResMut<SidePanel>) {
    let Ok(n) = q.get(a.entity) else {
        return;
    };
    let toggle =
        |open: &mut SidePanel, p: SidePanel| *open = if *open == p { SidePanel::None } else { p };
    match n.as_str() {
        "panel-bom" | "assembly-bom" => toggle(&mut open, SidePanel::Bom),
        "panel-named-positions" => toggle(&mut open, SidePanel::NamedPositions),
        "panel-exploded-views" => toggle(&mut open, SidePanel::ExplodedViews),
        "panel-sheet-metal" => toggle(&mut open, SidePanel::SheetMetal),
        "bom-panel-close" | "named-positions-panel-close" | "exploded-views-panel-close" => {
            *open = SidePanel::None
        }
        _ => {}
    }
}

/// The side panels' frame: a floating glass panel, full height on the viewport's right,
/// beside it rather than over it, so the graphics area (and the view ball in its corner)
/// shrinks to make room (P3.6).
pub(crate) fn side_panel_node(t: &Theme) -> impl Bundle {
    (
        Node {
            width: Val::Px(240.0),
            flex_shrink: 0.0,
            margin: UiRect::left(Val::Px(crate::document::PANEL_GAP)),
            overflow: Overflow::clip(),
            ..cadrs_ui::glass::panel_node(t)
        },
        cadrs_ui::glass::panel(t),
    )
}

/// The button of the open panel shows pressed (P3.6).
fn sync_panel_buttons(
    open: Res<SidePanel>,
    q: Query<(Entity, &Name, Has<cadrs_ui::style::Selected>)>,
    mut commands: Commands,
) {
    if !open.is_changed() {
        return;
    }
    for (e, n, selected) in &q {
        let want = match n.as_str() {
            "panel-bom" | "assembly-bom" => *open == SidePanel::Bom,
            "panel-named-positions" => *open == SidePanel::NamedPositions,
            "panel-exploded-views" => *open == SidePanel::ExplodedViews,
            "panel-simulation" => *open == SidePanel::Simulation,
            "panel-sheet-metal" => *open == SidePanel::SheetMetal,
            _ => continue,
        };
        if want && !selected {
            commands.entity(e).try_insert(cadrs_ui::style::Selected);
        } else if !want && selected {
            commands.entity(e).try_remove::<cadrs_ui::style::Selected>();
        }
    }
}

pub(crate) fn side_panel_header(
    p: &mut ChildSpawnerCommands,
    t: &Theme,
    title: &str,
    close: &'static str,
) {
    p.spawn(Node {
        height: Val::Px(32.0),
        padding: UiRect::new(Val::Px(10.0), Val::Px(4.0), Val::ZERO, Val::ZERO),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        border: UiRect::bottom(Val::Px(1.0)),
        ..default()
    })
    .insert(BorderColor::all(t.panel_border))
    .with_children(|h| {
        h.spawn(t.text(title.to_string(), 13.0, FontWeight::BOLD, t.foreground));
        h.spawn(
            IconButton::new(close, "close")
                .tooltip(cadrs_ui::i18n::tr("common-close"))
                .build(t),
        );
    });
}

/// Puts a side panel right after the viewport area in its row.
pub(crate) fn dock_beside_viewport(
    commands: &mut Commands,
    area: (Entity, &ChildOf),
    q_children: &Query<&Children>,
    panel: Entity,
) {
    let (area, parent) = (area.0, area.1.parent());
    let at = q_children
        .get(parent)
        .ok()
        .and_then(|c| c.iter().position(|e| e == area))
        .map_or(0, |i| i + 1);
    commands.entity(parent).insert_children(at, &[panel]);
}
