//! Walk mode (issue #97): moving through a model from inside it, a building's rooms, as in a
//! game. W and S go forward and back along the floor, A and D to the sides, Q and E down and up;
//! the rotate drag looks round from the eye. While walking the view is in perspective and keeps
//! the camera up; Esc, or the view menu's "Walk mode" again, leaves it and puts the projection
//! back. A sketch being edited, or another kind of tab, leaves it too (the sketch tools have
//! those keys). The keys are read as typed (the letters, on any keyboard layout, as every
//! shortcut is: issue #32); Shift stays free for the Part Studio's Shift+W, Shift+E and Shift+S.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_ui::ShortcutKey;

use crate::camera::ViewState;
use crate::viewport::{ActiveKind, ViewportView};

pub struct WalkPlugin;

impl Plugin for WalkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Walking>()
            .init_resource::<WalkKeys>()
            .add_systems(Update, (keep_up, walk_keys, walk).chain());
    }
}

/// Walk mode is on (`.0`), and whether the view was in perspective before it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Walking(pub bool, bool);

/// The walk keys held: forward, back, left, right, down, up.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WalkKeys([bool; 6]);

/// How far a second's walk goes, in eye distances (the eye's distance in front of the focus:
/// walking in a room seen up close is slower than over a site seen from afar).
pub const WALK_PER_SECOND: f32 = 0.25;

/// Turns walk mode on or off (the view menu's "Walk mode"): on, the view goes to perspective;
/// off, back to the projection it had.
pub fn set_walking(world: &mut World, on: bool) {
    let was = *world.resource::<Walking>();
    if was.0 == on {
        return;
    }
    let perspective = world.resource::<ViewportView>().view.perspective;
    world.insert_resource(Walking(on, if on { perspective } else { false }));
    world.insert_resource(WalkKeys::default());
    match (on, perspective, was.1) {
        (true, false, _) => crate::view_options::set_perspective(world, true),
        (false, true, false) => crate::view_options::set_perspective(world, false),
        _ => {}
    }
}

/// The walk keys' index in [`WalkKeys`].
fn walk_key(k: KeyCode) -> Option<usize> {
    Some(match k {
        KeyCode::KeyW => 0,
        KeyCode::KeyS => 1,
        KeyCode::KeyA => 2,
        KeyCode::KeyD => 3,
        KeyCode::KeyQ => 4,
        KeyCode::KeyE => 5,
        _ => return None,
    })
}

/// The view keeps the camera up when the preference says so, and while walking.
fn keep_up(
    prefs: Option<Res<crate::preferences_ui::LocalPreferences>>,
    walking: Res<Walking>,
    mut view: ResMut<ViewportView>,
) {
    let want = walking.0 || prefs.is_some_and(|p| p.0.keep_camera_up);
    if view.keep_up != want {
        view.keep_up = want;
    }
}

/// The walk keys pressed and let go, and Esc leaving walk mode; walk mode left in a sketch or
/// another kind of tab.
fn walk_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    walking: Res<Walking>,
    mut held: ResMut<WalkKeys>,
    kind: Res<ActiveKind>,
    sketch: Option<Res<crate::sketch::SketchSession>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    if !walking.0 {
        keys_in.clear();
        return;
    }
    if sketch.is_some() || !matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly) {
        commands.queue(|world: &mut World| set_walking(world, false));
        return;
    }
    let modified = keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    for k in keys_in.read() {
        let code = k.shortcut_code();
        if code == KeyCode::Escape && k.state == ButtonState::Pressed {
            commands.queue(|world: &mut World| set_walking(world, false));
            continue;
        }
        if let Some(i) = walk_key(code) {
            // (A chord is a shortcut, not a step: Shift+W revolves.)
            held.0[i] = k.state == ButtonState::Pressed && !modified;
        }
    }
}

/// A frame's walk: the view moved by [`walk_step`].
fn walk(
    walking: Res<Walking>,
    held: Res<WalkKeys>,
    time: Res<Time>,
    mut view: ResMut<ViewportView>,
) {
    if !walking.0 || held.0 == [false; 6] {
        return;
    }
    let d = walk_step(&view.view, held.0, time.delta_secs());
    if d != Vec3::ZERO {
        view.animation = None;
        view.view.focus += d;
    }
}

/// How far the view walks in `dt` seconds with the walk keys `held` ([`WalkKeys`]): forward
/// and to the sides along the floor (the view's directions turned level, so looking down
/// doesn't walk into the ground), down and up along world Z, at [`WALK_PER_SECOND`].
pub fn walk_step(v: &ViewState, held: [bool; 6], dt: f32) -> Vec3 {
    let axis =
        |pos: usize, neg: usize| f32::from(u8::from(held[pos])) - f32::from(u8::from(held[neg]));
    let (ahead, side, up) = (axis(0, 1), axis(3, 2), axis(5, 4));
    // (Unit vectors turned level; none for one within rounding of straight down or up.)
    let level = |d: Vec3| {
        let h = Vec3::new(d.x, d.y, 0.0);
        (h.length() > 1e-4).then(|| h.normalize())
    };
    // Looking straight down, forward is where the top of the screen points (straight up, where
    // its bottom does: the top points behind).
    let forward = level(-v.back())
        .or_else(|| level(v.up() * v.back().z.signum()))
        .unwrap_or(Vec3::Y);
    let right = level(v.right()).unwrap_or_else(|| forward.cross(Vec3::Z));
    let dir = (forward * ahead + right * side + Vec3::Z * up).normalize_or_zero();
    dir * WALK_PER_SECOND * v.eye_distance() * dt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::StandardView;

    fn held(keys: &[usize]) -> [bool; 6] {
        let mut h = [false; 6];
        for &k in keys {
            h[k] = true;
        }
        h
    }

    /// W walks level, where the view looks, even looking down at the floor; S back; A and D
    /// to the sides; E up and Q down; nothing held, nowhere.
    #[test]
    fn walking_goes_level_where_the_view_looks() {
        let mut v = ViewState::standard(StandardView::Default);
        v.perspective = true;
        v.scale = 2.0;
        let step = WALK_PER_SECOND * v.eye_distance();
        let w = walk_step(&v, held(&[0]), 1.0);
        assert!(w.z.abs() < 1e-4 && (w.length() - step).abs() < 1e-2, "{w}");
        assert!(w.dot(-v.back()) > 0.0);
        assert!((walk_step(&v, held(&[1]), 1.0) + w).length() < 1e-3);
        let d = walk_step(&v, held(&[3]), 1.0);
        assert!(d.dot(v.right()) > 0.0 && d.z.abs() < 1e-4);
        assert!(d.normalize().dot(w.normalize()).abs() < 1e-4, "{d} {w}");
        assert!((walk_step(&v, held(&[2]), 1.0) + d).length() < 1e-3);
        let e = walk_step(&v, held(&[5]), 1.0);
        assert!((e - Vec3::Z * step).length() < 1e-2, "{e}");
        assert!((walk_step(&v, held(&[4]), 1.0) + e).length() < 1e-3);
        assert_eq!(walk_step(&v, held(&[]), 1.0), Vec3::ZERO);
        // From the top, straight down: forward is up the screen.
        let mut top = ViewState::standard(StandardView::Top);
        top.perspective = true;
        let f = walk_step(&top, held(&[0]), 1.0);
        assert!(
            f.z.abs() < 1e-4 && f.normalize().dot(top.up()) > 0.99,
            "{f}"
        );
    }
}

#[cfg(test)]
mod keep_up_tests {
    use crate::viewport::ViewportView;

    /// Issue #97: keeping the camera up, the arrow keys' and the view cube's turns go about
    /// world Z and tilt, leaving no roll.
    #[test]
    fn turns_keep_the_camera_up() {
        let mut v = ViewportView {
            keep_up: true,
            ..Default::default()
        };
        v.view.rotate_screen(20.0, 50.0);
        v.turn_by(35.0, 25.0);
        let to = v.target();
        assert_eq!(to.roll, 0.0);
        assert!(to.right().z.abs() < 1e-5, "{}", to.right());
        // Without it the same turn is about the screen, and rolls a tilted view.
        let mut free = ViewportView::default();
        free.view.rotate_screen(20.0, 50.0);
        free.turn_by(35.0, 25.0);
        assert!(free.target().right().z.abs() > 1e-3);
    }
}
