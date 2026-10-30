// The view ball's image in the UI (`crate::view_cube`): its camera rendered it over a clear
// background, so its colours are already multiplied by their alpha; the pipeline blends them
// as such (the glass would otherwise darken what is behind it).

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    return textureSample(image, image_sampler, in.uv);
}
