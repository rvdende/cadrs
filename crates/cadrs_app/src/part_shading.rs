//! The viewport's part material: the head light of [`crate::parts::shade`] computed on the GPU
//! (`part_shading.wgsl`) from each fragment's normal and the camera's frame. The part meshes
//! carry only their faces' base colours ([`crate::parts::part_base_mesh`]), so turning the view
//! doesn't touch them; they change only when the parts, their colours or the selection do.

use bevy::asset::embedded_asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

pub struct PartShadingPlugin;

impl Plugin for PartShadingPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "part_shading.wgsl");
        app.add_plugins(MaterialPlugin::<PartShading>::default());
    }
}

/// Unlit, head-lit part faces: opaque (both sides drawn), or blended for the preview and
/// transparent appearances (front faces only); `cull_back` for a surface, whose mesh carries
/// both sides.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Copy, PartialEq, Eq)]
#[bind_group_data(PartShadingKey)]
pub struct PartShading {
    pub blend: bool,
    pub cull_back: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PartShadingKey {
    cull_back: bool,
}

impl From<&PartShading> for PartShadingKey {
    fn from(m: &PartShading) -> Self {
        Self { cull_back: m.cull_back }
    }
}

impl Material for PartShading {
    fn fragment_shader() -> ShaderRef {
        "embedded://cadrs_app/part_shading.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        if self.blend { AlphaMode::Blend } else { AlphaMode::Opaque }
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
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = key.bind_group_data.cull_back.then_some(Face::Back);
        Ok(())
    }
}
