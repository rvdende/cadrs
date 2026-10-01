// P3E.3a: the edges hidden behind faces (the render modes Shaded with hidden edges and Hidden
// edges visible), drawn only where something is in front of them (the depth test is reversed,
// `crate::hidden_edges`), dashed along their length (UV x is the distance along the edge, mm).

#import bevy_pbr::forward_io::VertexOutput

struct HiddenLineParams {
    color: vec4<f32>,
    clip: vec4<f32>,
    dash: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: HiddenLineParams;

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    // The section view's removed side.
    if (dot(params.clip.xyz, params.clip.xyz) > 0.5 && dot(params.clip.xyz, in.world_position.xyz) > params.clip.w) {
        discard;
    }
#ifdef VERTEX_UVS_A
    if (params.dash.x > 0.0 && fract(in.uv.x / params.dash.x) > 0.55) {
        discard;
    }
#endif
    // Pulled toward the eye (reversed depth: larger is nearer) by about the part edges' bias,
    // so an edge on a face it bounds counts as visible, not hidden behind it.
    var out: FragmentOutput;
    out.color = params.color;
    out.depth = pow(max(in.position.z, 1e-7), 1.0 - params.dash.y);
    return out;
}
