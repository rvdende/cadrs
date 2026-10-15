//! The window's icon (issue #14): the cadrs logo (`assets/icons/cadrs.png`, built into the
//! binary) set on the window once winit has made it, for the taskbar and title bar on Windows
//! and X11. (The Windows executable carries the same icon as a resource: `crates/cadrs/build.rs`.
//! Wayland shows a desktop entry's icon instead.)

use bevy::prelude::*;
use bevy::winit::WINIT_WINDOWS;

pub struct AppIconPlugin;

impl Plugin for AppIconPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, set_window_icon);
    }
}

const ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/cadrs.png");

fn set_window_icon(windows: Query<Entity, With<bevy::window::PrimaryWindow>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Ok(entity) = windows.single() else { return };
    WINIT_WINDOWS.with_borrow(|w| {
        // Not made yet (or headless: no winit window at all, nothing to do then).
        let Some(window) = w.get_window(entity) else { return };
        *done = true;
        let icon = image::load_from_memory(ICON_PNG).ok().map(|i| i.into_rgba8()).and_then(|i| {
            let (wd, ht) = i.dimensions();
            winit::window::Icon::from_rgba(i.into_raw(), wd, ht).ok()
        });
        if icon.is_none() {
            warn!("the app icon couldn't be read");
        }
        window.set_window_icon(icon);
    });
}
