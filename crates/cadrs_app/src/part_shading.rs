//! The viewport's part material: the head light of [`crate::parts::shade`] computed on the GPU
//! (`part_shading.wgsl`) from each fragment's normal and the camera's frame. The part meshes
//! carry only their faces' base colours ([`crate::parts::part_base_mesh`]), so turning the view
//! doesn't touch them; they change only when the parts, their colours or the selection do.

use bevy::asset::embedded_asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
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
///
/// P3E.3a: `params` carries the tab's render mode and section view
/// ([`crate::section_view`]): see [`PartShadingParams`]. `cap` marks a section view's cap
/// material, which the clip plane leaves alone.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Copy, PartialEq)]
#[bind_group_data(PartShadingKey)]
pub struct PartShading {
    pub blend: bool,
    pub cull_back: bool,
    pub cap: bool,
    /// The Translucent render mode's material (its parts drawn see-through).
    pub translucent: bool,
    #[uniform(0)]
    pub params: PartShadingParams,
}

impl PartShading {
    pub fn new(blend: bool, cull_back: bool) -> Self {
        Self { blend, cull_back, cap: false, translucent: false, params: PartShadingParams::default() }
    }
}

/// The per-view uniforms of [`PartShading`].
#[derive(Debug, Clone, Copy, PartialEq, ShaderType)]
pub struct PartShadingParams {
    /// The section view's clip plane: fragments with `dot(n, p) > w` (the removed side) are
    /// discarded; no clipping when `n` is zero.
    pub clip: Vec4,
    /// x: 1 draws the faces flat white (the hidden-line render modes); y: an opacity factor
    /// (the Translucent render mode's).
    pub style: Vec4,
    /// P3E.3b, the analysis tools ([`crate::analysis`]): x the face colouring (0 none, 1 zebra
    /// stripes, 2 draft analysis), y the draft angle needed (degrees), z the zebra stripes per
    /// half turn.
    pub analysis: Vec4,
    /// The draft analysis's pull direction (xyz); for zebra stripes the eye's position.
    pub pull: Vec4,
    /// The draft bands' colours (linear), top band first ([`cadrs_core::analysis::DraftBand`]).
    pub bands: [Vec4; 6],
}

impl Default for PartShadingParams {
    fn default() -> Self {
        Self { clip: Vec4::ZERO, style: Vec4::new(0.0, 1.0, 0.0, 0.0), analysis: Vec4::ZERO, pull: Vec4::ZERO, bands: [Vec4::ZERO; 6] }
    }
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
