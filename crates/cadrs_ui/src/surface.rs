//! Where the app renders: the primary window normally, or an offscreen image when the harness
//! runs headless. Cameras spawned by the app use [`RenderSurface::target`].

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::window::WindowRef;

#[derive(Resource, Clone, Debug)]
pub struct RenderSurface {
    pub target: RenderTarget,
    /// Size in logical pixels.
    pub size: UVec2,
    /// True when rendering to an offscreen image without a window.
    pub headless: bool,
}

impl Default for RenderSurface {
    fn default() -> Self {
        Self {
            target: RenderTarget::Window(WindowRef::Primary),
            size: UVec2::new(1600, 1000),
            headless: false,
        }
    }
}
