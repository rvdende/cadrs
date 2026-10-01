// Part faces lit by the head light (`crate::parts::shade` on the GPU): the vertex colour is the
// face's base colour (sRGB, 0–1) and opacity, UV x is 1 on a selected face. The light follows
// the camera, so turning the view costs nothing on the CPU.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

// P3E.3a: the section view's clip plane and the render mode (`PartShadingParams`).
struct PartShadingParams {
    clip: vec4<f32>,
    style: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: PartShadingParams;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The section view's removed side.
    if (dot(params.clip.xyz, params.clip.xyz) > 0.5 && dot(params.clip.xyz, in.world_position.xyz) > params.clip.w) {
        discard;
    }
    let len = length(in.world_normal);
    let n = select(in.world_normal, in.world_normal / len, len > 1e-6);
    // The camera frame: its columns are the view's right, up and back (`ViewState::rotation`).
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let back = view.world_from_view[2].xyz;
    // `crate::parts::brightness`.
    let b = clamp(0.705 + 0.146 * dot(n, right) + 0.185 * dot(n, up) + 0.25 * dot(n, back), 0.3, 1.1);
    // A selected face's sides are less dark (`crate::parts::shade`).
    let k = select(b, 1.0 - (1.0 - b) * 0.6, in.uv.x > 0.5);
    var c = clamp(in.color.rgb * k, vec3(0.0), vec3(1.0));
    // The hidden-line render modes: white faces (a selected face keeps its orange, flat).
    if (params.style.x > 0.5) {
        c = select(vec3(1.0), in.color.rgb, in.uv.x > 0.5);
    }
    return vec4(srgb_to_linear(c), in.color.a * params.style.y);
}
