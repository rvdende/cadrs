// The ground grid (`crate::ground_grid`), drawn like the schematic's: a faint line on every
// step and a small cross on every fifth line, with the X and Y axes in their colours. It fades
// with distance from the eye, at grazing angles, and where its lines would crowd.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;

struct GroundGrid {
    line: vec4<f32>,
    cross: vec4<f32>,
    x_axis: vec4<f32>,
    y_axis: vec4<f32>,
    // x: steps per mm, y: 1 / the fade distance (mm), z: a cross every so many lines, w: a
    // cross arm's length (a share of a step).
    grid: vec4<f32>,
    // x: 1 / how quickly the grid fades at grazing angles.
    fade: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> grid: GroundGrid;

// How much of a pixel `d` px from a line `w` px wide is covered.
fn cover(d: f32, w: f32) -> f32 {
    return clamp(w * 0.5 + 0.5 - d, 0.0, 1.0);
}

// `c` (alpha `a`) over `under`.
fn over(under: vec4<f32>, c: vec3<f32>, a: f32) -> vec4<f32> {
    let out_a = a + under.a * (1.0 - a);
    let rgb = (c * a + under.rgb * under.a * (1.0 - a)) / max(out_a, 1e-5);
    return vec4(rgb, out_a);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xy;
    // In steps, and steps per pixel.
    let coord = p * grid.grid.x;
    let fw = max(fwidth(coord), vec2(1e-6));
    // Lines closer than a few pixels fade, so the far grid doesn't turn into a hatch.
    let spacing = 1.0 / max(fw.x, fw.y);
    let crowd = clamp((spacing - 5.0) / 7.0, 0.0, 1.0);

    // A hairline on every step.
    let d = abs(fract(coord + 0.5) - 0.5) / fw;
    let line = max(cover(d.x, 1.0), cover(d.y, 1.0)) * crowd;

    // A cross on every `every`th line, its arms a share of a step each way.
    let every = grid.grid.z;
    let o = coord - round(coord / every) * every;
    let arm = grid.grid.w;
    let ends = clamp((arm - abs(o) ) / fw + 0.5, vec2(0.0), vec2(1.0));
    let h = cover(abs(o.y) / fw.y, 1.5) * ends.x;
    let v = cover(abs(o.x) / fw.x, 1.5) * ends.y;
    let cross_crowd = clamp((spacing * every - 30.0) / 30.0, 0.0, 1.0);
    let cross = max(h, v) * cross_crowd;

    // The axes: X runs where y is 0, Y where x is 0.
    let axis = abs(coord) / fw;
    let x_axis = cover(axis.y, 1.5);
    let y_axis = cover(axis.x, 1.5);

    var c = vec4(0.0);
    c = over(c, grid.line.rgb, grid.line.a * line);
    c = over(c, grid.cross.rgb, grid.cross.a * cross);
    c = over(c, grid.x_axis.rgb, grid.x_axis.a * x_axis);
    c = over(c, grid.y_axis.rgb, grid.y_axis.a * y_axis);

    // Fades with the distance from the eye, and seen edge-on.
    let pos = in.world_position.xyz;
    let depth = -(view.view_from_world * vec4(pos, 1.0)).z;
    let dist_fade = clamp(1.0 - depth * grid.grid.y, 0.0, 1.0);
    let facing = abs(normalize(view.world_position - pos).z);
    let fade = mix(dist_fade, 1.0, facing) * min(grid.fade.x * facing, 1.0);
    return vec4(c.rgb, c.a * fade);
}
