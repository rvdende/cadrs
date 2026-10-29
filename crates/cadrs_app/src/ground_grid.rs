//! The ground grid under the 3D view (on by default), drawn like the schematic's grid
//! (`crate::eda`): a faint line on every step and a small cross on every fifth line, with the
//! X and Y axes in their colours (`ground_grid.wesl`). It is a quad on the XY plane around the
//! focus, as wide as the grid is seen, with a material that draws the pattern per pixel, so the
//! lines stay one pixel at any zoom. Its step follows the zoom ([`crate::viewport`]).

use bevy::asset::embedded_asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

pub struct GroundGridPlugin;

impl Plugin for GroundGridPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "ground_grid.wesl");
        app.add_plugins(MaterialPlugin::<GroundGridMaterial>::default());
    }
}

/// The ground grid's quad.
#[derive(Component, Debug, Clone, Copy)]
pub struct GroundGrid;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Copy, PartialEq)]
pub struct GroundGridMaterial {
    #[uniform(0)]
    pub params: GroundGridParams,
}

/// What `ground_grid.wesl` draws with. Colours are linear RGBA.
#[derive(Debug, Clone, Copy, PartialEq, ShaderType)]
pub struct GroundGridParams {
    pub line: Vec4,
    pub cross: Vec4,
    pub x_axis: Vec4,
    pub y_axis: Vec4,
    /// x: steps per mm; y: 1 / the fade distance (mm); z: a cross every so many lines; w: a
    /// cross arm's length, a share of a step.
    pub grid: Vec4,
    /// x: 1 / how quickly it fades at grazing angles.
    pub fade: Vec4,
}

/// A cross on every fifth line, its arms 0.3 of a step each way (the schematic's).
pub const CROSS_EVERY: f32 = 5.0;
pub const CROSS_ARM: f32 = 0.3;

impl Default for GroundGridParams {
    fn default() -> Self {
        Self {
            line: Vec4::ZERO,
            cross: Vec4::ZERO,
            x_axis: Vec4::ZERO,
            y_axis: Vec4::ZERO,
            grid: Vec4::new(1.0, 1e-4, CROSS_EVERY, CROSS_ARM),
            fade: Vec4::new(2.0, 0.0, 0.0, 0.0),
        }
    }
}

impl Material for GroundGridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://cadrs_app/ground_grid.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // Over the view's background, against the parts' depth.
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(false);
        }
        // Seen from below as from above.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The grid's step at `scale` mm per pixel: 1, 2 or 5 times a power of ten, the smallest whose
/// lines are at least [`MIN_SPACING_PX`] apart.
pub fn step_for(scale: f32) -> f32 {
    let want = (scale * MIN_SPACING_PX).max(1e-6);
    let decade = 10f32.powf(want.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * decade)
        .find(|s| *s >= want * 0.9999)
        .unwrap_or(10.0 * decade)
}

/// The least distance between two lines on screen, in pixels.
pub const MIN_SPACING_PX: f32 = 18.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_step_is_one_two_or_five_times_a_power_of_ten() {
        // 0.1 mm per pixel: lines at least 1.8 mm apart.
        assert_eq!(step_for(0.1), 2.0);
        assert_eq!(step_for(0.2), 5.0);
        assert_eq!(step_for(0.5), 10.0);
        assert!((step_for(1.0) - 20.0).abs() < 1e-3);
        assert!((step_for(0.01) - 0.2).abs() < 1e-5);
    }
}
