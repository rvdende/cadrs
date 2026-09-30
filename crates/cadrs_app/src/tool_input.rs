//! One input abstraction for tools (the plan's `SketchInput`).
//!
//! Tools must not read winit cursor events or the window directly. They read [`ToolInput`],
//! which is built from `bevy_picking` pointers and `ButtonInput<KeyCode>`. The scenario harness
//! drives a custom picking pointer and writes `KeyboardInput` messages, so real and scripted
//! input reach tools through exactly the same path. M0 only provides the stub; sketch tools use
//! it from M4 on.

use bevy::ecs::system::SystemParam;
use bevy::picking::pointer::{PointerButton, PointerId, PointerLocation, PointerPress};
use bevy::prelude::*;

pub struct ToolInputPlugin;

impl Plugin for ToolInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActivePointer>()
            .add_systems(PreUpdate, track_active_pointer);
    }
}

/// The pointer that moved most recently (the mouse, or the harness's scripted pointer).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct ActivePointer(pub Option<PointerId>);

fn track_active_pointer(
    mut active: ResMut<ActivePointer>,
    q: Query<(&PointerId, &PointerLocation), Changed<PointerLocation>>,
) {
    for (id, loc) in &q {
        if loc.location.is_some() {
            active.0 = Some(*id);
        }
    }
}

/// Pointer and keyboard state for tools.
#[derive(SystemParam)]
pub struct ToolInput<'w, 's> {
    active: Res<'w, ActivePointer>,
    pointers: Query<
        'w,
        's,
        (
            &'static PointerId,
            &'static PointerLocation,
            &'static PointerPress,
        ),
    >,
    keys: Res<'w, ButtonInput<KeyCode>>,
}

impl ToolInput<'_, '_> {
    fn active(&self) -> Option<(&PointerLocation, &PointerPress)> {
        let id = self.active.0?;
        self.pointers
            .iter()
            .find(|(p, _, _)| **p == id)
            .map(|(_, l, p)| (l, p))
    }

    /// Cursor position in logical pixels of the render surface.
    pub fn cursor(&self) -> Option<Vec2> {
        self.active()
            .and_then(|(l, _)| l.location.as_ref())
            .map(|l| l.position)
    }

    pub fn pressed(&self, button: PointerButton) -> bool {
        self.active().is_some_and(|(_, p)| match button {
            PointerButton::Primary => p.is_primary_pressed(),
            PointerButton::Secondary => p.is_secondary_pressed(),
            PointerButton::Middle => p.is_middle_pressed(),
        })
    }

    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.keys.pressed(key)
    }

    pub fn key_just_pressed(&self, key: KeyCode) -> bool {
        self.keys.just_pressed(key)
    }

    /// Shift disables inference while drawing, as in Onshape.
    pub fn shift(&self) -> bool {
        self.keys
            .any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    }
}
