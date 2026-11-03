// Part faces lit by the head light (`crate::parts::shade` on the GPU): the vertex colour is the
// face's base colour (sRGB, 0–1) and opacity, UV x is 1 on a selected face. The light follows
// the camera, so turning the view costs nothing on the CPU.
//
// A mesh with a UV 1 has faces that lie on another part's face (`crate::parts::shared_faces`):
// x their rank among the parts there, y how many (0: not shared; negative on a curved face).
// Such a face's depth is moved a hair, by stripe: the parts' order front to back turns by one
// from stripe to stripe, so they take turns in front, and any of them there differ in depth
// (instead of fighting for each pixel's, flickering as the view turns). Where no other part's
// face is there, it looks as it is. The stripes stay where they are on the model as the view
// turns: on a plane at 45° to axes the plane picks (the same for both parts' faces), running
// another way on a face at an angle to it; on a curved face where diagonal slabs of space
// cross it.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;

// P3E.3a: the section view's planes and the render mode (`PartShadingParams`).
struct PartShadingParams {
    clip: array<vec4<f32>, 4>,
    style: vec4<f32>,
    // P3E.3b: the analysis tools (`crate::analysis`).
    analysis: vec4<f32>,
    pull: vec4<f32>,
    bands: array<vec4<f32>, 6>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: PartShadingParams;

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @if(VERTEX_UVS_B)
    @builtin(frag_depth) depth: f32,
}

// About how many pixels wide a shared face's stripes are: their width in mm is the power of
// two nearest that (so zooming steps it, turning the view doesn't).
const STRIPE_PX: f32 = 7.0;

// How far `p` (world) is across the stripes of the plane with normal `n`: along the diagonal
// of two axes in the plane that only the plane decides (its normal turned to point up its first
// clear axis, so two faces of one plane facing either way agree).
fn stripe_coord(p: vec3<f32>, n_in: vec3<f32>) -> f32 {
    var n = normalize(n_in);
    let first = select(select(n.z, n.y, abs(n.y) > 1e-3), n.x, abs(n.x) > 1e-3);
    n = select(n, -n, first < 0.0);
    let e = select(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), abs(n.x) < 0.6);
    let u = normalize(cross(n, e));
    let v = cross(n, u);
    return (dot(p, u) + dot(p, v)) * 0.70710678;
}

// The depth of a fragment at `pos` (its framebuffer position; reversed depth, larger nearer),
// `world` on a face with normal `n` and `share`: nearer in its own stripes, farther in the
// others'. `px` is the size of a pixel there (mm).
fn shared_depth(pos: vec4<f32>, world: vec3<f32>, n: vec3<f32>, share: vec2<f32>, px: f32) -> f32 {
    let count = abs(round(share.y));
    if (count < 1.5) {
        return pos.z;
    }
    let width = exp2(round(log2(max(STRIPE_PX * px, 1e-6))));
    let across = select(stripe_coord(world, n), dot(world, vec3(0.57735027)), share.y < 0.0);
    // This face's place in the stripe's order: (stripe + rank) mod count, front last.
    let k = floor(across / width) + round(share.x);
    let place = k - count * floor(k / count);
    return pos.z * (1.0 + 4e-6 * (place - (count - 1.0) * 0.5));
}

// The size of a pixel (mm) at `world`: the same everywhere in an orthographic view, growing
// with the distance in a perspective one.
fn pixel_size(world: vec3<f32>) -> f32 {
    let w = max(view.viewport.z, 1.0);
    let scale = 2.0 / (max(abs(view.clip_from_view[0][0]), 1e-12) * w);
    if (view.clip_from_view[3][3] > 0.5) {
        return scale;
    }
    let depth = -(view.view_from_world * vec4(world, 1.0)).z;
    return scale * max(depth, 1e-6);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// The section view's planes (zero for none): `p` is cut away when it is past every plane.
fn section_removed(clip: array<vec4<f32>, 4>, p: vec3<f32>) -> bool {
    var planes = 0;
    var removed = true;
    for (var i = 0; i < 4; i++) {
        let c = clip[i];
        if (dot(c.xyz, c.xyz) > 0.5) {
            planes += 1;
            if (dot(c.xyz, p) <= c.w) {
                removed = false;
            }
        }
    }
    return planes > 0 && removed;
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    // The section view's removed part: on the removed side of every plane.
    if (section_removed(params.clip, in.world_position.xyz)) {
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
    // The hidden-line render modes: flat faces in the scheme's paper grey, `style.z` (white in
    // light; a selected face keeps its orange, flat).
    if (params.style.x > 0.5) {
        c = select(vec3(params.style.z), in.color.rgb, in.uv.x > 0.5);
    }
    var lin = srgb_to_linear(c);
    // P3E.3b, the analysis tools' face colouring (`crate::analysis::ShadingAnalysis`): x the mode
    // (1 zebra, 2 draft, 3 curvature). A selected face keeps its orange.
    let mode = i32(round(params.analysis.x));
    // The zebra's coordinate (`cadrs_core::analysis::zebra_phase_at`): the view ray reflected off
    // the face, in the view's frame, its angle out of the plane across a slanted screen axis.
    // Orthographic views look along −back everywhere; perspective ones along each pixel's ray.
    // Worked out for every fragment, outside the branches: a derivative (`fwidth`) has to be
    // taken in uniform control flow, which WebGPU checks.
    let ortho = view.clip_from_view[3][3] > 0.5;
    let e = select(normalize(view.world_position.xyz - in.world_position.xyz), back, ortho);
    let r = reflect(-e, n);
    let rv = vec3(dot(r, right), dot(r, up), dot(r, back));
    let phi = asin(clamp(dot(normalize(rv), normalize(vec3(0.5, 0.85, 0.0))), -1.0, 1.0));
    let zebra_x = phi * params.analysis.z / 3.14159265;
    let fwidth_x = fwidth(zebra_x);
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
        // Zebra stripes: the bands of `zebra_x` (above), smoothed over a pixel. The dark band is
        // a dark grey, lit a little, so faces and their (grey) edges stay apart.
        let x = zebra_x;
        let w = max(fwidth_x, 1e-4);
        let f = abs(fract(x) - 0.5) * 2.0;
        let white = smoothstep(0.5 - w, 0.5 + w, f);
        let g = mix(0.22, 0.96, white) * mix(1.0, b, 0.3);
        lin = srgb_to_linear(vec3(g));
    }
    var out: FragmentOutput;
    out.color = vec4(lin, in.color.a * params.style.y);
    @if(VERTEX_UVS_B)
    out.depth = shared_depth(in.position, in.world_position.xyz, in.world_normal, in.uv_b, pixel_size(in.world_position.xyz));
    return out;
}
