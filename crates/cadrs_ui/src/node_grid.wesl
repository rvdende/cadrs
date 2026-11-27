// A node canvas's grid (`crate::node_graph`), in the graph's own units as the PCB Studio's
// schematic grid is in mm: faint lines every step and, every fifth line, a stronger cross where
// two meet. Drawn by one node covering the canvas from the canvas's pan and zoom, so a pan or a
// zoom only changes these numbers.

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct Grid {
    // x, y: the pan (logical px); z: the zoom; w: logical px a physical px.
    view: vec4<f32>,
    // x: the step (graph units, already spaced out for the zoom); y: a cross's arm (px);
    // z: the style: 0 none, 1 crosses on every fifth line, 2 every tenth line stronger.
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
    if grid.step.z < 0.5 {
        return vec4(0.0);
    }
    let p = in.uv * in.size * grid.view.w;
    let zoom = grid.view.z;
    let g = (p - grid.view.xy) / zoom;
    let step = grid.step.x;
    // A line a pixel wide, softened over half a pixel each side.
    let dx = off(g.x, step, zoom);
    let dy = off(g.y, step, zoom);
    let line = 1.0 - smoothstep(0.5, 1.0, min(dx, dy));
    var strong = 0.0;
    if grid.step.z < 1.5 {
        // The crosses where every fifth line meets every fifth.
        let big = step * 5.0;
        let mx = off(g.x, big, zoom);
        let my = off(g.y, big, zoom);
        let arm = grid.step.y;
        strong = select(0.0, 1.0, (mx < 0.75 && my < arm) || (my < 0.75 && mx < arm));
    } else {
        // Every tenth line, stronger.
        let big = step * 10.0;
        strong = 1.0 - smoothstep(0.5, 1.0, min(off(g.x, big, zoom), off(g.y, big, zoom)));
    }
    let c = mix(grid.line * line, grid.mark, strong);
    // Premultiplied, as the UI blends it.
    return vec4(c.rgb * c.a, c.a);
}
