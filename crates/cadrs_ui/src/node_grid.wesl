// A node canvas's grid (`crate::node_graph`), in the graph's own units as the PCB Studio's
// schematic grid is in mm: faint lines every step and, every fifth line, a stronger cross where
// two meet. Drawn by one node covering the canvas from the canvas's pan and zoom, so a pan or a
// zoom only changes these numbers.

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct Grid {
    // x, y: the pan (logical px); z: the zoom; w: logical px a physical px.
    view: vec4<f32>,
    // x: the step (graph units, already spaced out for the zoom); y: a cross's arm (px).
    step: vec4<f32>,
    // Linear RGBA: the lines' colour, and the crosses'.
    line: vec4<f32>,
    mark: vec4<f32>,
}

@group(1) @binding(0) var<uniform> grid: Grid;

// How far (logical px on screen) `g` (graph units) is from the nearest multiple of `step`.
fn off(g: f32, step: f32, zoom: f32) -> f32 {
    return abs(fract(g / step + 0.5) - 0.5) * step * zoom;
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let p = in.uv * in.size * grid.view.w;
    let zoom = grid.view.z;
    let g = (p - grid.view.xy) / zoom;
    let step = grid.step.x;
    // A line a pixel wide, softened over half a pixel each side.
    let dx = off(g.x, step, zoom);
    let dy = off(g.y, step, zoom);
    let line = 1.0 - smoothstep(0.5, 1.0, min(dx, dy));
    // The crosses where every fifth line meets every fifth.
    let big = step * 5.0;
    let mx = off(g.x, big, zoom);
    let my = off(g.y, big, zoom);
    let arm = grid.step.y;
    let cross = select(0.0, 1.0, (mx < 0.75 && my < arm) || (my < 0.75 && mx < arm));
    let c = mix(grid.line * line, grid.mark, cross);
    // Premultiplied, as the UI blends it.
    return vec4(c.rgb * c.a, c.a);
}
