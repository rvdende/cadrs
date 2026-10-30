//! Walk mode (issue #97): moving through a model from inside it, a building's rooms, as in a
//! game. W and S go forward and back along the floor, A and D to the sides, Q and E down and up,
//! at a walk ([`WALK_SPEED`]); the mouse looks round from the eye with no button held, its
//! pointer hidden and held at the viewport's centre under a crosshair ([`LOOK_SPEED`]); the wheel
//! changes the lens, from a fisheye-like 8 mm to a 300 mm telephoto ([`LENS_MM`]). While walking
//! the view is in perspective and keeps the camera up; Esc, or the view menu's "Walk mode" again,
//! leaves it and puts the projection, the lens and the pointer back. A sketch being edited, or
//! another kind of tab, leaves it too (the sketch tools have those keys). The keys are read as typed (the letters, on any keyboard layout, as every
//! shortcut is: issue #32); Shift stays free for the Part Studio's Shift+W, Shift+E and Shift+S.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorMoved, CursorOptions, PrimaryWindow, WindowEvent};
use cadrs_ui::ShortcutKey;

use crate::camera::{FOCAL_PX, ViewState, ZOOM_PER_LINE};
use crate::viewport::{ActiveKind, ViewportArea, ViewportRect, ViewportView};

pub struct WalkPlugin;

impl Plugin for WalkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Walking>()
            .init_resource::<WalkKeys>()
            .add_systems(
                Update,
                (
                    keep_up,
                    walk_keys,
                    walk,
                    walk_pointer,
                    lens_back,
                    walk_crosshair,
                    lens_label,
                )
                    .chain(),
            );
    }
}

/// Walk mode: whether it is on, the tab it was turned on in, and whether that tab's view was in
/// perspective before (put back when it is left in the same tab).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Walking {
    pub on: bool,
    element: Option<cadrs_core::ElementId>,
    was_perspective: bool,
}

/// The walk keys held, by [`FORWARD`], [`BACK`], [`LEFT`], [`RIGHT`], [`DOWN`] and [`UP`].
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WalkKeys([bool; 6]);

const FORWARD: usize = 0;
const BACK: usize = 1;
const LEFT: usize = 2;
const RIGHT: usize = 3;
const DOWN: usize = 4;
const UP: usize = 5;

/// How far a second's walk goes (mm): 1.5 m, a person's walk, however far the view is zoomed.
pub const WALK_SPEED: f32 = 1500.0;

/// How far a mouse move turns the view while walking, against how far the same rotate drag
/// turns it (and the other way round: the view turns the way the mouse goes, as a game's does).
pub const LOOK_SPEED: f32 = 0.25;

/// The walk lens's range, as a 35 mm camera's focal lengths (mm): from 8 mm, a fisheye's sweep
/// (about 130° across a wide viewport, a flat picture's stretch at the edges), to a 300 mm
/// telephoto (about 7°).
pub const LENS_MM: std::ops::RangeInclusive<f32> = 8.0..=300.0;

/// A 35 mm frame's diagonal (mm): a focal length against it is the field of view's, as against
/// the viewport's diagonal in px.
const FRAME_DIAGONAL_MM: f32 = 43.27;

/// The 35 mm camera's focal length (mm) of a lens of `focal` logical px across a viewport of
/// `size` logical px.
pub fn lens_mm(focal: f32, size: Vec2) -> f32 {
    focal * FRAME_DIAGONAL_MM / size.length().max(1.0)
}

/// Walk mode's wheel: the lens zoomed `lines` steps (> 0, away from the user, a longer lens: a
/// narrower view and the scene larger), within [`LENS_MM`], the eye where it was.
pub fn lens_wheel(v: &ViewState, lines: f32, size: Vec2) -> ViewState {
    let mm = lens_mm(v.focal, size) * ZOOM_PER_LINE.powf(lines);
    let mm = mm.clamp(*LENS_MM.start(), *LENS_MM.end());
    v.with_focal(mm * size.length().max(1.0) / FRAME_DIAGONAL_MM)
}

/// Turns walk mode on or off (the view menu's "Walk mode"): on, the view goes to perspective;
/// off, back to the projection it had.
pub fn set_walking(world: &mut World, on: bool) {
    let was = *world.resource::<Walking>();
    if was.on == on {
        return;
    }
    let view = world.resource::<ViewportView>();
    let (perspective, element) = (view.view.perspective, view.element);
    world.insert_resource(Walking {
        on,
        element,
        was_perspective: perspective,
    });
    world.insert_resource(WalkKeys::default());
    // (What it does and how to leave it: walk mode shows nothing else.)
    if on && world.contains_resource::<cadrs_ui::Theme>() {
        crate::linked::toast(
            world,
            "Walk mode: W A S D to walk, Q E down and up, the mouse looks round, the wheel \
             changes the lens, Esc to leave",
        );
    }
    if !on {
        normal_lens(&mut world.resource_mut::<ViewportView>());
    }
    if on && !perspective {
        crate::view_options::set_perspective(world, true);
    } else if !on && was.element == element && !was.was_perspective && perspective {
        // (Left in another tab, that tab's projection is its own.)
        crate::view_options::set_perspective(world, false);
    }
}

/// The normal lens ([`FOCAL_PX`]) back, the eye where it was; a view on the move gets it at the
/// end of the move.
fn normal_lens(view: &mut ViewportView) {
    match view.animation.as_mut() {
        Some(a) => a.to = a.to.with_focal(FOCAL_PX),
        None => view.view = view.view.with_focal(FOCAL_PX),
    }
}

/// Out of walk mode the lens is the normal one: a tab's view walked in, and left so by going to
/// another tab, gets it back when it comes back.
fn lens_back(walking: Res<Walking>, mut view: ResMut<ViewportView>) {
    let lens = view
        .animation
        .as_ref()
        .map_or(view.view.focal, |a| a.to.focal);
    if !walking.on && lens != FOCAL_PX {
        normal_lens(&mut view);
    }
}

/// The walk keys' index in [`WalkKeys`].
fn walk_key(k: KeyCode) -> Option<usize> {
    Some(match k {
        KeyCode::KeyW => FORWARD,
        KeyCode::KeyS => BACK,
        KeyCode::KeyA => LEFT,
        KeyCode::KeyD => RIGHT,
        KeyCode::KeyQ => DOWN,
        KeyCode::KeyE => UP,
        _ => return None,
    })
}

/// The view keeps the camera up when the preference says so, and while walking.
fn keep_up(
    prefs: Option<Res<crate::preferences_ui::LocalPreferences>>,
    walking: Res<Walking>,
    mut view: ResMut<ViewportView>,
) {
    let want = walking.on || prefs.is_some_and(|p| p.0.keep_camera_up);
    if view.keep_up != want {
        view.keep_up = want;
    }
}

/// The walk keys pressed and let go, and Esc leaving walk mode; walk mode left in a sketch,
/// another tab or another kind of tab. Keys typed into a field or a dialog don't walk, and the
/// window losing the keyboard lets go of them all (its key-ups never come).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn walk_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    mut focus_lost: MessageReader<bevy::input::keyboard::KeyboardFocusLost>,
    walking: Res<Walking>,
    mut held: ResMut<WalkKeys>,
    (kind, view): (Res<ActiveKind>, Res<ViewportView>),
    sketch: Option<Res<crate::sketch::SketchSession>>,
    keys: Res<ButtonInput<KeyCode>>,
    (focus, q_fields, q_dialogs): (
        Res<bevy::input_focus::InputFocus>,
        Query<(), With<cadrs_ui::KeyboardField>>,
        Query<(), With<cadrs_ui::DialogRoot>>,
    ),
    mut commands: Commands,
) {
    if !walking.on {
        keys_in.clear();
        focus_lost.clear();
        return;
    }
    if sketch.is_some()
        || view.element != walking.element
        || !matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly)
    {
        commands.queue(|world: &mut World| set_walking(world, false));
        return;
    }
    let typing = focus.get().is_some_and(|e| q_fields.contains(e)) || !q_dialogs.is_empty();
    if focus_lost.read().count() > 0 || typing {
        *held = WalkKeys::default();
    }
    if typing {
        keys_in.clear();
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
    if !walking.on || held.0 == [false; 6] {
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
/// doesn't walk into the ground), down and up along world Z, at [`WALK_SPEED`].
pub fn walk_step(v: &ViewState, held: [bool; 6], dt: f32) -> Vec3 {
    let axis =
        |pos: usize, neg: usize| f32::from(u8::from(held[pos])) - f32::from(u8::from(held[neg]));
    let (ahead, side, up) = (axis(FORWARD, BACK), axis(RIGHT, LEFT), axis(UP, DOWN));
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
    dir * WALK_SPEED * dt
}

/// While walking the mouse is the view's, as in a game: the pointer hidden and held at the
/// viewport's centre (where the crosshair is, and a click picks), and its moves look round from
/// the eye at [`LOOK_SPEED`], no button held. An open menu or dialog, or the window in the
/// background, lets it go; leaving walk mode puts it back.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn walk_pointer(
    walking: Res<Walking>,
    motion: Res<AccumulatedMouseMotion>,
    rect: Res<ViewportRect>,
    mut view: ResMut<ViewportView>,
    mut q_window: Query<(Entity, &mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    q_popups: Query<(), Or<(With<cadrs_ui::menu::MenuPopup>, With<cadrs_ui::DialogRoot>)>>,
    (mut moved, mut window_events): (MessageWriter<CursorMoved>, MessageWriter<WindowEvent>),
    mut held: Local<bool>,
) {
    let Ok((window_entity, mut window, mut cursor)) = q_window.single_mut() else {
        return;
    };
    let hold = walking.on && window.focused && q_popups.is_empty();
    if hold != *held {
        // (Taken this frame, moved from the next: the OS has the grab first.)
        *held = hold;
        cursor.grab_mode = if hold {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        cursor.visible = !hold;
        return;
    }
    if !hold {
        return;
    }
    if motion.delta != Vec2::ZERO {
        view.animation = None;
        let eye = view.view.camera_position();
        view.view
            .orbit_about(eye, |v| v.orbit_turntable(motion.delta * LOOK_SPEED));
    }
    let centre = rect.0.center();
    if window
        .cursor_position()
        .is_none_or(|p| p.distance(centre) > 0.5)
    {
        window.set_cursor_position(Some(centre));
        // (Picking's pointer too: a locked pointer reports no moves of its own.)
        let event = CursorMoved {
            window: window_entity,
            position: centre,
            delta: None,
        };
        window_events.write(WindowEvent::CursorMoved(event.clone()));
        moved.write(event);
    }
}

/// Marks walk mode's crosshair, and its lens label.
#[derive(Component)]
struct Crosshair;

/// Marks the text of walk mode's lens label.
#[derive(Component)]
struct LensLabel;

/// Walk mode's crosshair at the viewport's centre, where the pointer is held, and the lens's
/// label at its foot.
fn walk_crosshair(
    walking: Res<Walking>,
    theme: Option<Res<cadrs_ui::Theme>>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_crosshair: Query<Entity, With<Crosshair>>,
    mut commands: Commands,
) {
    if walking.on != q_crosshair.is_empty() {
        return;
    }
    if !walking.on {
        for e in &q_crosshair {
            commands.entity(e).despawn();
        }
        return;
    }
    let (Some(t), Ok(area)) = (theme, q_area.single()) else {
        return;
    };
    const ARM: f32 = 18.0;
    const WIDTH: f32 = 2.0;
    let bar = |name: &'static str, width: f32, height: f32| {
        (
            Name::new(name),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(-width / 2.0),
                top: Val::Px(-height / 2.0),
                width: Val::Px(width),
                height: Val::Px(height),
                ..default()
            },
            BackgroundColor(t.foreground.with_alpha(0.85)),
            Outline::new(Val::Px(1.0), Val::ZERO, t.background.with_alpha(0.6)),
            Pickable::IGNORE,
        )
    };
    let crosshair = commands
        .spawn((
            Name::new("walk-crosshair"),
            Crosshair,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn(bar("walk-crosshair-across", ARM, WIDTH));
            p.spawn(bar("walk-crosshair-down", WIDTH, ARM));
        })
        .id();
    let lens = commands
        .spawn((
            Name::new("walk-lens"),
            Crosshair,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(14.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                Name::new("walk-lens-pill"),
                Node {
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(t.background.with_alpha(0.85)),
                Pickable::IGNORE,
            ))
            .with_child((
                Name::new("walk-lens-text"),
                LensLabel,
                t.text("", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                Pickable::IGNORE,
            ));
        })
        .id();
    commands.entity(area).add_children(&[crosshair, lens]);
}

/// The lens label's text when the lens or the viewport changes.
fn lens_label(
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    mut q_label: Query<(&mut Text, Ref<LensLabel>)>,
) {
    for (mut text, label) in &mut q_label {
        if view.is_changed() || rect.is_changed() || label.is_added() {
            let lens = lens_text(view.view.focal, rect.0.size());
            if text.0 != lens {
                text.0 = lens;
            }
        }
    }
}

/// A lens as a 35 mm camera's focal length and the view's angle across: "50 mm · 61°".
fn lens_text(focal: f32, size: Vec2) -> String {
    let across = 2.0 * (size.x / 2.0 / focal).atan();
    format!(
        "{:.0} mm · {:.0}°",
        lens_mm(focal, size),
        across.to_degrees()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::StandardView;

    /// The wheel changes the lens, the eye and the focus staying: away from the user a longer
    /// one (the scene larger), from a fisheye-like 8 mm to a 300 mm telephoto and no further.
    #[test]
    fn the_wheel_changes_the_lens() {
        let size = Vec2::new(1374.0, 901.0);
        let mut v = ViewState::standard(StandardView::Default);
        v.perspective = true;
        let eye = v.camera_position();
        let near = |a: Vec3| (a - eye).length() < eye.length() * 1e-5;
        assert_eq!(lens_text(v.focal, size), "45 mm · 44°");
        let long = lens_wheel(&v, 3.0, size);
        let ratio = lens_mm(long.focal, size) / lens_mm(v.focal, size);
        assert!((ratio - ZOOM_PER_LINE.powi(3)).abs() < 1e-4, "{ratio}");
        assert!(long.scale < v.scale && long.focus == v.focus);
        assert!(near(long.camera_position()), "{}", long.camera_position());
        let tele = lens_wheel(&v, 100.0, size);
        assert_eq!(lens_text(tele.focal, size), "300 mm · 7°");
        let wide = lens_wheel(&v, -100.0, size);
        assert_eq!(lens_text(wide.focal, size), "8 mm · 132°");
        assert!(near(wide.camera_position()) && near(tele.camera_position()));
        assert!(lens_wheel(&wide, -1.0, size).focal == wide.focal);
    }

    fn held(keys: &[usize]) -> [bool; 6] {
        let mut h = [false; 6];
        for &k in keys {
            h[k] = true;
        }
        h
    }

    /// W walks level, where the view looks, even looking down at the floor, 1.5 m a second
    /// however far the view is zoomed; S back; A and D to the sides; E up and Q down; nothing
    /// held, nowhere.
    #[test]
    fn walking_goes_level_where_the_view_looks() {
        let mut v = ViewState::standard(StandardView::Default);
        v.perspective = true;
        v.scale = 2.0;
        let step = 1500.0;
        let w = walk_step(&v, held(&[0]), 1.0);
        assert!(w.z.abs() < 1e-4 && (w.length() - step).abs() < 1e-2, "{w}");
        let mut far = v;
        far.scale = 50.0;
        assert!((walk_step(&far, held(&[0]), 1.0) - w).length() < 1e-2);
        let tele = lens_wheel(&v, 10.0, Vec2::new(1374.0, 901.0));
        assert!((walk_step(&tele, held(&[0]), 1.0) - w).length() < 1e-2);
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

/// The view's keep-up turns (the arrow keys', the view cube's: [`ViewportView::turn_by`]),
/// kept with walk mode's, which keeps the camera up too.
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

/// Leaving walk mode puts the projection back in the tab it was turned on in, not in another;
/// the upright opposite view (Numpad 9 keeping the camera up) turns the top into the bottom.
#[cfg(test)]
mod mode_tests {
    use super::*;
    use crate::camera::StandardView;

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Walking>();
        world.init_resource::<WalkKeys>();
        world.insert_resource(ViewportView {
            element: Some(cadrs_core::ElementId::new()),
            ..Default::default()
        });
        world
    }

    #[test]
    fn leaving_walk_mode_puts_the_projection_back() {
        let mut world = world();
        set_walking(&mut world, true);
        assert!(world.resource::<Walking>().on);
        assert!(world.resource::<ViewportView>().view.perspective);
        set_walking(&mut world, false);
        assert!(!world.resource::<ViewportView>().view.perspective);
        // Already in perspective: it stays so.
        crate::view_options::set_perspective(&mut world, true);
        set_walking(&mut world, true);
        set_walking(&mut world, false);
        assert!(world.resource::<ViewportView>().view.perspective);
        // The lens the wheel set goes back to the normal one.
        set_walking(&mut world, true);
        let v = world.resource::<ViewportView>().view;
        world.resource_mut::<ViewportView>().view = lens_wheel(&v, 5.0, Vec2::new(1374.0, 901.0));
        set_walking(&mut world, false);
        assert_eq!(world.resource::<ViewportView>().view.focal, FOCAL_PX);
        // Left in another tab: that tab's projection is its own.
        crate::view_options::set_perspective(&mut world, false);
        set_walking(&mut world, true);
        world.resource_mut::<ViewportView>().element = Some(cadrs_core::ElementId::new());
        set_walking(&mut world, false);
        assert!(world.resource::<ViewportView>().view.perspective);
    }

    /// Walking, the pointer is held (locked and hidden) at the viewport's centre and the mouse
    /// looks round from the eye with no button held, a quarter of a rotate drag's turn: right
    /// turns right, up looks up. Leaving walk mode lets the pointer go.
    #[test]
    fn the_mouse_looks_round_from_the_eye() {
        use bevy::ecs::message::Messages;
        let mut world = world();
        world.init_resource::<AccumulatedMouseMotion>();
        world.init_resource::<ViewportRect>();
        world.init_resource::<Messages<CursorMoved>>();
        world.init_resource::<Messages<WindowEvent>>();
        let window = world.spawn((Window::default(), PrimaryWindow)).id();
        let system = world.register_system(walk_pointer);
        set_walking(&mut world, true);
        world.run_system(system).unwrap();
        let cursor = world.get::<CursorOptions>(window).unwrap();
        assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
        assert!(!cursor.visible);

        let before = world.resource::<ViewportView>().view;
        world.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(40.0, -20.0);
        world.run_system(system).unwrap();
        let after = world.resource::<ViewportView>().view;
        let (eye, was, now) = (after.camera_position(), -before.back(), -after.back());
        assert!((eye - before.camera_position()).length() < 1e-2, "{eye}");
        assert!(
            now.dot(before.right()) > 0.0 && now.z > was.z,
            "{was} → {now}"
        );
        // 40 px right: a quarter of a drag's 16°.
        let turn = was.truncate().angle_to(now.truncate()).to_degrees().abs();
        assert!((turn - 4.0).abs() < 1e-2, "{turn}");
        let centre = world.resource::<ViewportRect>().0.center();
        let at = world.get::<Window>(window).unwrap().cursor_position();
        assert!(at.is_some_and(|p| p.distance(centre) < 0.5), "{at:?}");

        set_walking(&mut world, false);
        world.run_system(system).unwrap();
        let cursor = world.get::<CursorOptions>(window).unwrap();
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(cursor.visible);
    }

    #[test]
    fn the_upright_opposite_of_the_top_is_the_bottom() {
        let to = ViewState::standard(StandardView::Top).upright_opposite();
        let bottom = ViewState::standard(StandardView::Bottom);
        assert!(
            (to.back() - bottom.back()).length() < 1e-4,
            "{} {}",
            to.back(),
            bottom.back()
        );
    }
}
