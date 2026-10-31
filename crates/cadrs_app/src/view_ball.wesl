// The view ball's parts (`crate::view_cube`): the glass, brighter toward its rim and with a
// highlight up and to the left, and the axes, knobs, dots and rings inside it, which fade on
// the ball's far side. Unlit; the views are orthographic.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;

struct Ball {
    // Linear RGBA: the part's colour (the glass's at its centre).
    color: vec4<f32>,
    // What the far side fades toward; the glass's rim colour.
    far: vec4<f32>,
    // x: 1 for the glass; y: how far the far side fades (0 to 1); z: the glass's rim opacity;
    // w: its highlight's.
    mode: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> ball: Ball;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Toward the viewer: the camera's own +Z.
    let back = normalize(view.world_from_view[2].xyz);
    if ball.mode.x > 0.5 {
        let n = normalize(in.world_normal);
        let rim = pow(1.0 - clamp(dot(n, back), 0.0, 1.0), 2.5);
        let n_view = normalize((view.view_from_world * vec4(n, 0.0)).xyz);
        let spec = pow(max(dot(n_view, normalize(vec3(-0.45, 0.55, 0.7))), 0.0), 40.0);
        let rgb = mix(mix(ball.color.rgb, ball.far.rgb, rim), vec3(1.0), spec);
        let a = max(mix(ball.color.a, ball.mode.z, rim), spec * ball.mode.w);
        return vec4(rgb, a);
    }
    // The ball's centre is the origin: a point's depth toward the viewer, -1.2 to 1.2.
    let depth = dot(in.world_position.xyz, back);
    let fade = clamp((0.2 - depth) / 1.0, 0.0, 1.0) * ball.mode.y;
    return vec4(mix(ball.color.rgb, ball.far.rgb, fade), 1.0);
}
