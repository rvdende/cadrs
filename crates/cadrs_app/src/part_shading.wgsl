// Part faces lit by the head light (`crate::parts::shade` on the GPU): the vertex colour is the
// face's base colour (sRGB, 0–1) and opacity, UV x is 1 on a selected face. The light follows
// the camera, so turning the view costs nothing on the CPU.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

// P3E.3a: the section view's clip plane and the render mode (`PartShadingParams`).
struct PartShadingParams {
    clip: vec4<f32>,
    style: vec4<f32>,
    // P3E.3b: the analysis tools (`crate::analysis`).
    analysis: vec4<f32>,
    pull: vec4<f32>,
    bands: array<vec4<f32>, 6>,
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
    var lin = srgb_to_linear(c);
    // P3E.3b, the analysis tools' face colouring (`crate::analysis::ShadingAnalysis`): x the mode
    // (1 zebra, 2 draft, 3 curvature). A selected face keeps its orange.
    let mode = i32(round(params.analysis.x));
    if (mode == 2 && in.uv.x < 0.5) {
        // Draft analysis: the band of this point's draft against the pull direction
        // (`cadrs_core::analysis::DraftBand::of`, its tolerance `DRAFT_EPS` in w), lit like the
        // faces.
        let d = degrees(asin(clamp(dot(n, normalize(params.pull.xyz)), -1.0, 1.0)));
        let a = abs(params.analysis.y);
        let eps = params.analysis.w;
        var i = 5;
        if (d >= 2.0 * a - eps) {
            i = 0;
        } else if (d >= a - eps) {
            i = 1;
        } else if (d > -eps) {
            i = 2;
        } else if (d > -a + eps) {
            i = 3;
        } else if (d > -2.0 * a + eps) {
            i = 4;
        }
        lin = params.bands[i].rgb * mix(1.0, b, 0.55);
    } else if (mode == 3 && in.uv.x < 0.5) {
        // Curvature: the band of the vertex's mean curvature (UV y, per mm) when the most curved
        // shown is y (`cadrs_core::analysis::curvature_band`; flat at most w).
        let k = abs(in.uv.y);
        let mx = params.analysis.y;
        var i = 5;
        if (k > params.analysis.w && mx > params.analysis.w) {
            i = 4 - i32(clamp(floor(k / mx * 5.0), 0.0, 4.0));
        }
        lin = params.bands[i].rgb * mix(1.0, b, 0.55);
    } else if (mode == 1 && in.uv.x < 0.5) {
        // Zebra stripes (`cadrs_core::analysis::zebra_phase_at`): the view ray reflected off the
        // face, in the view's frame, its angle out of the plane across a slanted screen axis cut
        // into bands (smoothed over a pixel). Orthographic views look along −back everywhere;
        // perspective ones along each pixel's ray. The dark band is a dark grey, lit a little,
        // so faces and their (grey) edges stay apart.
        let ortho = view.clip_from_view[3][3] > 0.5;
        let e = select(normalize(view.world_position.xyz - in.world_position.xyz), back, ortho);
        let r = reflect(-e, n);
        let rv = vec3(dot(r, right), dot(r, up), dot(r, back));
        let phi = asin(clamp(dot(normalize(rv), normalize(vec3(0.5, 0.85, 0.0))), -1.0, 1.0));
        let x = phi * params.analysis.z / 3.14159265;
        let w = max(fwidth(x), 1e-4);
        let f = abs(fract(x) - 0.5) * 2.0;
        let white = smoothstep(0.5 - w, 0.5 + w, f);
        let g = mix(0.22, 0.96, white) * mix(1.0, b, 0.3);
        lin = srgb_to_linear(vec3(g));
    }
    return vec4(lin, in.color.a * params.style.y);
}
