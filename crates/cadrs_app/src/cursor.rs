//! Which cursor the viewport asks for (see `cadrs_ui::cursor`): a grabbing hand while
//! orbiting, the move cursor while panning, a crosshair while a sketch tool draws, the move
//! cursor over draggable sketch geometry, dimension labels and glyphs, and a grabbing hand
//! while dragging them. Buttons and text fields get theirs from `cadrs_ui`.

use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use cadrs_sketch::SketchEntity;
use cadrs_ui::{CursorKind, CursorRequest};

use crate::AppState;
use crate::sketch::{ActiveSketchTool, SketchSession, SketchTool};
use crate::sketch_tools::{SketchDraw, SketchHover, places_points};
use crate::viewport::{ViewportArea, ViewportDrag, pointer_over_viewport};

pub struct AppCursorPlugin;

impl Plugin for AppCursorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            request_cursor.before(bevy::ui::UiSystems::Prepare),
        );
    }
}

/// The viewport cursor for the current sketch state (`None` outside a sketch).
pub fn sketch_cursor(
    tool: SketchTool,
    hover: Option<SketchEntity>,
    waiting_for_plane: bool,
) -> Option<CursorKind> {
    if waiting_for_plane {
        return None;
    }
    // Trim, Extend and Split use the sketch-tool crosshair too
    // (`edit_tools/trim-points-02.png`, `03`).
    if places_points(tool)
        || matches!(
            tool,
            SketchTool::Dimension | SketchTool::Trim | SketchTool::Extend | SketchTool::Split
        )
    {
        return Some(CursorKind::Crosshair);
    }
    match (tool, hover) {
        (
            SketchTool::Select,
            Some(
                SketchEntity::Point(_)
                | SketchEntity::Curve(_)
                | SketchEntity::Dimension(_)
                | SketchEntity::Text(_)
                | SketchEntity::Constraint(_),
            ),
        ) => Some(CursorKind::Move),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn request_cursor(
    state: Option<Res<State<AppState>>>,
    drag: Res<ViewportDrag>,
    keys: Res<ButtonInput<KeyCode>>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    session: Option<Res<SketchSession>>,
    tool: Res<ActiveSketchTool>,
    draw: Res<SketchDraw>,
    hover: Res<SketchHover>,
    mut request: ResMut<CursorRequest>,
) {
    let in_document = state.is_some_and(|s| *s.get() == AppState::Document);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let mut want = CursorRequest::default();
    if in_document {
        want.capture = if drag.orbiting(ctrl) {
            Some(CursorKind::Grabbing)
        } else if drag.panning(ctrl) {
            Some(CursorKind::Move)
        } else if session.is_some() && draw.dragging() {
            Some(CursorKind::Grabbing)
        } else {
            None
        };
        if pointer_over_viewport(&hover_map, &q_area) {
            want.viewport = session
                .as_ref()
                .and_then(|s| sketch_cursor(tool.tool, hover.0, s.waiting_for_plane));
        }
    }
    if *request != want {
        *request = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_sketch::ConstraintKind;

    #[test]
    fn sketch_cursors() {
        assert_eq!(sketch_cursor(SketchTool::Line, None, false), Some(CursorKind::Crosshair));
        assert_eq!(sketch_cursor(SketchTool::Dimension, None, false), Some(CursorKind::Crosshair));
        assert_eq!(sketch_cursor(SketchTool::Trim, None, false), Some(CursorKind::Crosshair));
        assert_eq!(sketch_cursor(SketchTool::Split, None, false), Some(CursorKind::Crosshair));
        assert_eq!(sketch_cursor(SketchTool::Line, None, true), None);
        assert_eq!(sketch_cursor(SketchTool::Select, None, false), None);
        assert_eq!(
            sketch_cursor(SketchTool::Select, Some(SketchEntity::Origin), false),
            None
        );
        let tool = SketchTool::Constrain(ConstraintKind::Parallel);
        assert_eq!(sketch_cursor(tool, None, false), None);
    }
}
