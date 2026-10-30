//! Six-degree-of-freedom input (3Dconnexion SpaceMouse) for the Part Studio and Assembly
//! viewport.
//!
//! Device access runs on a background thread so a missing driver, device or spacenav daemon
//! cannot block the Bevy event loop. Backends publish axis samples in the device's own axes (as
//! its USB reports carry them); this module owns the camera mapping shared by Windows (the raw
//! HID reports) and Linux (the spacenavd daemon). macOS and the browser have no backend yet.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use web_time::Instant;

use crate::camera::ViewState;
use crate::viewport::{ActiveKind, ViewportView};
use crate::{AppState, eda::Eda2d};

// Rates at full deflection, tuned on a SpaceNavigator.
const PAN_PIXELS_PER_SECOND: f32 = 480.0;
const ROTATION_DEGREES_PER_SECOND: f32 = 100.0;
const ZOOM_PER_SECOND: f32 = 1.4;
const MAX_INPUT_WINDOW: f32 = 0.05;
const MAX_QUEUED_SAMPLES: usize = 64;
/// A cap's full deflection in device counts (spacenavd passes them on unscaled).
#[cfg(any(target_os = "windows", target_os = "linux", test))]
const FULL_DEFLECTION: f32 = 350.0;
/// Counts below this read as rest, as with spacenavd's default dead zone, so a cap that doesn't
/// settle exactly on zero doesn't creep the view.
#[cfg(any(target_os = "windows", target_os = "linux", test))]
const DEAD_ZONE: i32 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct MotionSample {
    /// Translation and rotation rates, normalized to approximately -1..=1.
    axes: [f32; 6],
    /// Time represented by the sample, capped to avoid a large jump after a pause.
    dt: f32,
}

#[derive(Resource)]
struct MotionReceiver(Mutex<Receiver<MotionSample>>);

/// The Windows 3Dconnexion driver can also emit forward/backward SpaceMouse translation as
/// ordinary wheel input. Suppress that legacy event briefly after raw HID translation so zoom is
/// applied only by the smooth 6-DoF input path.
#[derive(Resource, Clone, Default)]
pub(crate) struct LegacyWheelGuard(std::sync::Arc<Mutex<Option<Instant>>>);

impl LegacyWheelGuard {
    #[cfg(any(target_os = "windows", test))]
    pub(crate) fn mark_space_mouse_translation(&self) {
        if let Ok(mut last_motion) = self.0.lock() {
            *last_motion = Some(Instant::now());
        }
    }

    pub(crate) fn suppress_legacy_wheel(&self) -> bool {
        self.0
            .lock()
            .ok()
            .and_then(|last_motion| *last_motion)
            .is_some_and(|last_motion| last_motion.elapsed() < Duration::from_millis(300))
    }
}

pub struct SpaceMousePlugin;

impl Plugin for SpaceMousePlugin {
    fn build(&self, app: &mut App) {
        let (sender, receiver) = mpsc::channel();
        let wheel_guard = LegacyWheelGuard::default();
        start_backend(sender, wheel_guard.clone());
        app.insert_resource(MotionReceiver(Mutex::new(receiver)))
            .insert_resource(wheel_guard)
            .add_systems(Update, apply_motion);
    }
}

/// Starts reading the device on a thread of its own.
#[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
fn start_backend(sender: Sender<MotionSample>, wheel_guard: LegacyWheelGuard) {
    #[cfg(target_os = "windows")]
    windows_backend::start(sender, wheel_guard);
    #[cfg(target_os = "linux")]
    linux_backend::start(sender);
}

/// Moves the view by what the device sent since the last frame. As with 3Dconnexion's own
/// driver, that is wherever the pointer is and whatever field has focus (the device neither
/// clicks nor types), as long as the window has focus.
fn apply_motion(
    input: Res<MotionReceiver>,
    state: Res<State<AppState>>,
    mut view: ResMut<ViewportView>,
    kind: Res<ActiveKind>,
    eda2d: Res<Eda2d>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    // Drained on every screen, so motion from outside a document doesn't queue up for it.
    let sample = input
        .0
        .lock()
        .ok()
        .and_then(|receiver| coalesce_recent_samples(receiver.try_iter()));
    let Some(sample) = sample else {
        return;
    };

    let viewport_active = *state.get() == AppState::Document
        && matches!(*kind, ActiveKind::PartStudio | ActiveKind::Assembly)
        && !eda2d.flat()
        && windows.iter().any(|window| window.focused);
    if viewport_active {
        apply_sample(&mut view, sample);
    }
}

/// Coalesce a short recent input window instead of replaying every queued HID report after a
/// render stall. That keeps device motion responsive without turning a temporary frame drop into
/// a visible camera jump.
fn coalesce_recent_samples(
    samples: impl IntoIterator<Item = MotionSample>,
) -> Option<MotionSample> {
    let mut recent = VecDeque::with_capacity(MAX_QUEUED_SAMPLES);
    for sample in samples {
        if recent.len() == MAX_QUEUED_SAMPLES {
            recent.pop_front();
        }
        recent.push_back(sample);
    }

    let mut weighted_axes = [0.0; 6];
    let mut total_dt = 0.0;
    let mut remaining = MAX_INPUT_WINDOW;
    for sample in recent.iter().rev() {
        if remaining <= 0.0 {
            break;
        }
        let dt = sample.dt.clamp(0.0, MAX_INPUT_WINDOW);
        let used_dt = dt.min(remaining);
        if used_dt == 0.0 {
            continue;
        }
        for (weighted, axis) in weighted_axes.iter_mut().zip(sample.axes) {
            *weighted += axis * used_dt;
        }
        total_dt += used_dt;
        remaining -= used_dt;
    }
    if total_dt == 0.0 {
        return None;
    }

    Some(MotionSample {
        axes: weighted_axes.map(|axis| axis / total_dt),
        dt: total_dt,
    })
}

fn apply_sample(view: &mut ViewportView, sample: MotionSample) {
    if sample.axes.iter().all(|axis| axis.abs() < 0.0001) {
        return;
    }
    let dt = sample.dt.clamp(0.0, 0.05);
    if dt <= 0.0 {
        return;
    }
    let [tx, ty, tz, rx, ry, rz] = sample.axes;
    view.animation = None;

    // Horizontal translation follows screen right. The device's forward/backward translation
    // axis is inverted to match object-mode movement on screen. The third axis zooms about the
    // view centre, keeping the pivot from the current view intact.
    view.view.pan(Vec2::new(
        tx * PAN_PIXELS_PER_SECOND * dt,
        ty * PAN_PIXELS_PER_SECOND * dt,
    ));
    if tz.abs() > 0.0001 {
        let factor = (tz * ZOOM_PER_SECOND * dt).exp();
        view.view.zoom_at(factor, Vec2::ZERO);
    }

    let rotation = ROTATION_DEGREES_PER_SECOND.to_radians() * dt;
    // Map the rotation controls to the displayed frame so their effect does not change when the
    // model's world axes point in a different direction on screen. Read the frame once per sample
    // so simultaneous axis input does not drift as each rotation is applied.
    let screen_right = view.view.right();
    let screen_up = view.view.up();
    let screen_depth = view.view.back();
    let pivot = view.pivot;
    let turn = |state: &mut ViewState| {
        state.rotate_model_about(screen_right, rx * rotation);
        state.rotate_model_about(screen_depth, ry * rotation);
        state.rotate_model_about(screen_up, -rz * rotation);
    };
    if let Some(pivot) = pivot {
        view.view.orbit_about(pivot, turn);
    } else {
        turn(&mut view.view);
    }
}

/// One axis from device counts to -1..=1.
#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn normalize(count: i32) -> f32 {
    if count.abs() < DEAD_ZONE {
        return 0.0;
    }
    (count as f32 / FULL_DEFLECTION).clamp(-1.0, 1.0)
}

/// Reads a HID motion report into `axes`, returning whether it was one. Report 1 carries the
/// translation and report 2 the rotation, except on newer devices, which send no report 2 and
/// put the rotation in report 1 after the translation. Windows pads every report to the
/// device's longest, so report 1's tail is taken for the rotation only until a report 2 shows
/// that the device sends them apart (`split`).
#[cfg(any(target_os = "windows", test))]
fn read_report(report: &[u8], axes: &mut [f32; 6], split: &mut bool) -> bool {
    let (first, count) = match report.first() {
        Some(1) if report.len() >= 13 && !*split => (0, 6),
        Some(1) if report.len() >= 7 => (0, 3),
        Some(2) if report.len() >= 7 => {
            *split = true;
            (3, 3)
        }
        _ => return false,
    };
    for (i, axis) in axes[first..first + count].iter_mut().enumerate() {
        let at = 1 + 2 * i;
        *axis = normalize(i16::from_le_bytes([report[at], report[at + 1]]).into());
    }
    true
}

/// spacenavd swaps Y and Z of 3Dconnexion devices and negates them (its `DF_SWAPYZ |
/// DF_INVYZ`, translation and rotation alike): this turns its axes back into the device's own,
/// as the Windows backend reads them.
#[cfg(any(target_os = "linux", test))]
fn from_spacenavd([x, y, z, rx, ry, rz]: [i32; 6]) -> [f32; 6] {
    [x, -z, -y, rx, -rz, -ry].map(normalize)
}

#[cfg(target_os = "windows")]
mod windows_backend {
    use super::*;
    use hidapi::{DeviceInfo, HidApi, HidDevice};
    use std::thread;

    /// Logitech's vendor id, which 3Dconnexion's older devices carry.
    const LOGITECH: u16 = 0x046d;
    /// 3Dconnexion's own vendor id, on its newer devices.
    const THREE_DCONNEXION: u16 = 0x256f;
    /// The 6-DoF 3Dconnexion devices under Logitech's id (spacenavd's list).
    const LOGITECH_DEVICES: [u16; 9] = [
        0xc605, // CadMan
        0xc621, // SpaceBall 5000
        0xc623, // SpaceTraveler
        0xc625, // SpacePilot
        0xc626, // SpaceNavigator
        0xc627, // SpaceExplorer
        0xc628, // SpaceNavigator for Notebooks
        0xc629, // SpacePilot Pro
        0xc62b, // SpaceMouse Pro
    ];

    pub(super) fn start(sender: Sender<MotionSample>, wheel_guard: LegacyWheelGuard) {
        let _ = thread::Builder::new()
            .name("cadrs-spacemouse-hid".into())
            .spawn(move || run(sender, wheel_guard));
    }

    fn is_space_mouse(device: &DeviceInfo) -> bool {
        match device.vendor_id() {
            LOGITECH => LOGITECH_DEVICES.contains(&device.product_id()),
            // A wireless receiver also lists the mouse paired with it: only its multi-axis
            // controller (Generic Desktop page, usage 8).
            THREE_DCONNEXION => device.usage_page() == 0x01 && device.usage() == 0x08,
            _ => false,
        }
    }

    fn run(sender: Sender<MotionSample>, wheel_guard: LegacyWheelGuard) {
        loop {
            let Ok(api) = HidApi::new() else {
                thread::sleep(Duration::from_secs(2));
                continue;
            };
            let device = api
                .device_list()
                .find(|device| is_space_mouse(device))
                .and_then(|device| device.open_device(&api).ok());
            let Some(device) = device else {
                thread::sleep(Duration::from_millis(750));
                continue;
            };
            bevy::log::info!("SpaceMouse HID input connected");
            if read_device(&device, &sender, &wheel_guard).is_err() {
                return;
            }
            bevy::log::debug!("SpaceMouse HID input disconnected");
            thread::sleep(Duration::from_millis(250));
        }
    }

    /// Forwards the device's motion until it goes away; an error when the app has quit.
    fn read_device(
        device: &HidDevice,
        sender: &Sender<MotionSample>,
        wheel_guard: &LegacyWheelGuard,
    ) -> Result<(), mpsc::SendError<MotionSample>> {
        let mut sample = MotionSample::default();
        let mut split = false;
        let mut last_report = Instant::now();
        let mut report = [0_u8; 64];
        loop {
            let count = match device.read_timeout(&mut report, 20) {
                Ok(count) => count,
                Err(error) => {
                    bevy::log::debug!(%error, "SpaceMouse HID read failed");
                    return Ok(());
                }
            };
            if !read_report(&report[..count], &mut sample.axes, &mut split) {
                continue;
            }
            let now = Instant::now();
            sample.dt = now
                .duration_since(last_report)
                .as_secs_f32()
                .min(MAX_INPUT_WINDOW);
            last_report = now;
            if sample.axes[1] != 0.0 || sample.axes[2] != 0.0 {
                wheel_guard.mark_space_mouse_translation();
            }
            sender.send(sample)?;
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_backend {
    use super::*;
    use spnav_rs::{SpnavClient, SpnavEvent};
    use std::thread;

    pub(super) fn start(sender: Sender<MotionSample>) {
        let _ = thread::Builder::new()
            .name("cadrs-spacemouse-spnav".into())
            .spawn(move || run(sender));
    }

    fn run(sender: Sender<MotionSample>) {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                bevy::log::warn!(%error, "cannot start SpaceMouse event runtime");
                return;
            }
        };
        loop {
            let mut client = match runtime.block_on(SpnavClient::open()) {
                Ok(client) => client,
                Err(_) => {
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            bevy::log::info!("Connected to the spacenav daemon");
            loop {
                // `timeout` registers its timer as soon as it is called. Building it as an
                // argument of `block_on` runs that call before the runtime is entered, and the
                // thread panics with "there is no reactor running".
                let event = runtime.block_on(async {
                    tokio::time::timeout(Duration::from_millis(100), client.wait_event()).await
                });
                match event {
                    Ok(Ok(SpnavEvent::Motion(motion))) => {
                        let sample = MotionSample {
                            axes: from_spacenavd([
                                motion.x, motion.y, motion.z, motion.rx, motion.ry, motion.rz,
                            ]),
                            dt: (motion.period as f32 / 1000.0).clamp(0.0, MAX_INPUT_WINDOW),
                        };
                        if sender.send(sample).is_err() {
                            return;
                        }
                    }
                    Ok(Ok(_)) | Err(_) => {}
                    Ok(Err(error)) => {
                        bevy::log::debug!(%error, "spacenav connection ended");
                        break;
                    }
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_input_does_not_move_the_camera() {
        let mut view = ViewportView::default();
        let before = view.view;
        apply_sample(&mut view, MotionSample::default());
        assert_eq!(view.view, before);
    }

    #[test]
    fn translation_and_rotation_move_the_camera() {
        let mut view = ViewportView::default();
        apply_sample(
            &mut view,
            MotionSample {
                axes: [1.0, 0.5, 0.5, 0.5, 0.5, 0.5],
                dt: 1.0 / 60.0,
            },
        );
        assert_ne!(view.view.focus, Vec3::ZERO);
        assert_ne!(view.view.scale, ViewState::default().scale);
        assert_ne!(view.view.rotation(), ViewState::default().rotation());
    }

    #[test]
    fn forward_translation_matches_object_mode_and_positive_depth_zooms_in() {
        let mut view = ViewportView::default();
        let dt = 0.02;
        let mut expected = view.view;
        expected.pan(Vec2::new(0.0, PAN_PIXELS_PER_SECOND * dt));
        let initial_scale = view.view.scale;

        apply_sample(
            &mut view,
            MotionSample {
                axes: [0.0, 1.0, 1.0, 0.0, 0.0, 0.0],
                dt,
            },
        );

        assert_eq!(view.view.focus, expected.focus);
        assert!(view.view.scale < initial_scale);
    }

    #[test]
    fn orbit_about_pivot_keeps_the_pivot_stationary() {
        let mut view = ViewportView::default();
        let pivot = Vec3::new(20.0, 10.0, 5.0);
        let screen_before = view.view.project(pivot);
        view.pivot = Some(pivot);
        apply_sample(
            &mut view,
            MotionSample {
                axes: [0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                dt: 1.0 / 30.0,
            },
        );
        let screen_after = view.view.project(pivot);
        assert!(screen_before.distance(screen_after) < 1e-4);
    }

    #[test]
    fn rotation_axes_follow_the_displayed_view_frame() {
        for (axes, axis_index, camera_direction) in [
            ([0.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0, -1.0),
            ([0.0, 0.0, 0.0, 0.0, 1.0, 0.0], 1, -1.0),
            ([0.0, 0.0, 0.0, 0.0, 0.0, 1.0], 2, 1.0),
        ] {
            let mut view = ViewportView::default();
            view.view.azimuth = 53.0;
            view.view.elevation = 41.0;
            view.view.roll = 17.0;
            let before = view.view.rotation();
            let screen_axes = [view.view.right(), view.view.back(), view.view.up()];
            let radians = ROTATION_DEGREES_PER_SECOND.to_radians() / 30.0;

            apply_sample(
                &mut view,
                MotionSample {
                    axes,
                    dt: 1.0 / 30.0,
                },
            );

            let expected =
                Quat::from_axis_angle(screen_axes[axis_index], camera_direction * radians) * before;
            // Compares the camera axes: `angle_between` can't resolve angles this small in f32.
            let after = view.view.rotation();
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!((after * axis).distance(expected * axis) < 1e-4);
            }
        }
    }

    #[test]
    fn legacy_wheel_is_suppressed_after_space_mouse_translation() {
        let guard = LegacyWheelGuard::default();
        assert!(!guard.suppress_legacy_wheel());
        guard.mark_space_mouse_translation();
        assert!(guard.suppress_legacy_wheel());
    }

    #[test]
    fn coalesces_only_the_recent_input_window() {
        let sample = |axis, dt| MotionSample {
            axes: [axis, 0.0, 0.0, 0.0, 0.0, 0.0],
            dt,
        };
        let coalesced =
            coalesce_recent_samples([sample(1.0, 0.05), sample(-1.0, 0.025), sample(0.0, 0.025)])
                .unwrap();

        assert!((coalesced.dt - MAX_INPUT_WINDOW).abs() < 1e-6);
        assert!((coalesced.axes[0] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn normalizes_counts_with_a_dead_zone() {
        assert_eq!(normalize(350), 1.0);
        assert_eq!(normalize(-175), -0.5);
        assert_eq!(normalize(700), 1.0);
        assert_eq!(normalize(1), 0.0);
        assert_eq!(normalize(-1), 0.0);
        assert_eq!(normalize(2), 2.0 / 350.0);
    }

    /// A report with `id` and the given axis counts, little-endian, padded to `len` bytes.
    fn report(id: u8, counts: &[i16], len: usize) -> Vec<u8> {
        let mut bytes = vec![id];
        bytes.extend(counts.iter().flat_map(|count| count.to_le_bytes()));
        bytes.resize(len, 0);
        bytes
    }

    #[test]
    fn reads_translation_and_rotation_reports() {
        let mut axes = [0.0; 6];
        let mut split = false;
        assert!(read_report(
            &report(1, &[350, -350, 0], 7),
            &mut axes,
            &mut split
        ));
        assert!(read_report(
            &report(2, &[175, 0, -175], 7),
            &mut axes,
            &mut split
        ));
        assert_eq!(axes, [1.0, -1.0, 0.0, 0.5, 0.0, -0.5]);
    }

    #[test]
    fn reads_all_six_axes_from_one_report() {
        let mut axes = [0.0; 6];
        let mut split = false;
        let combined = report(1, &[350, 0, -175, 0, 175, -350], 13);
        assert!(read_report(&combined, &mut axes, &mut split));
        assert_eq!(axes, [1.0, 0.0, -0.5, 0.0, 0.5, -1.0]);
    }

    #[test]
    fn padding_after_the_translation_does_not_clear_the_rotation() {
        let mut axes = [0.0; 6];
        let mut split = false;
        assert!(read_report(
            &report(2, &[175, 0, 0], 13),
            &mut axes,
            &mut split
        ));
        assert!(read_report(
            &report(1, &[350, 0, 0], 13),
            &mut axes,
            &mut split
        ));
        assert_eq!(axes, [1.0, 0.0, 0.0, 0.5, 0.0, 0.0]);
    }

    #[test]
    fn ignores_unknown_or_short_reports() {
        let mut axes = [0.0; 6];
        let mut split = false;
        assert!(!read_report(&[1, 0, 0], &mut axes, &mut split));
        assert!(!read_report(
            &report(3, &[350, 350, 350], 7),
            &mut axes,
            &mut split
        ));
        assert!(!read_report(&[], &mut axes, &mut split));
        assert_eq!(axes, [0.0; 6]);
    }

    #[test]
    fn undoes_spacenavds_axis_swap() {
        let device = [10, 20, 30, 40, 50, 60];
        // spacenavd's `read_device`: swap Y and Z, then negate the new Y and Z.
        let swapped = [0, 2, 1, 3, 5, 4].map(|i| device[i]);
        let daemon: [i32; 6] =
            std::array::from_fn(|i| if i % 3 == 0 { swapped[i] } else { -swapped[i] });
        assert_eq!(from_spacenavd(daemon), device.map(normalize));
    }
}
