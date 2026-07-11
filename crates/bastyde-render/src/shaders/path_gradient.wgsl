// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

// Gradient-filled path shader (Tier 3 — arbitrary CPU-rasterized paths,
// gradient paint only). Solid-filled paths stay on the lean
// `quad_pipeline` (see quad.wgsl); this pipeline only draws
// `PathEntry`s whose `paint_data` is a gradient variant.
//
// The path atlas holds a pure AA **coverage mask** (opaque white RGB,
// alpha = coverage — see `path_atlas::rasterize_path`), exactly like the
// monochrome-glyph convention `quad.wgsl` uses for `flags = 0`. Instead
// of tinting that mask with a single flat vertex color, this shader
// computes an analytic gradient color per-fragment from `local_uv` (the
// same shape-local 0..1 placement `sdf.wgsl` uses) and modulates its
// alpha by the sampled coverage.
//
// `sample_gradient` and the linear/radial/conic branches below are
// duplicated verbatim from sdf.wgsl — there is no WGSL #include, and the
// two shaders' fragment stages otherwise diverge (SDF distance-to-alpha
// vs atlas-coverage-to-alpha), so sharing a common file isn't a clean
// fit. Keep both in sync by hand if the gradient math changes.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coord: vec2<f32>,
    @location(2) local_uv: vec2<f32>,
    @location(3) paint_type: u32,
    @location(4) gradient_geo: vec4<f32>,
    @location(5) gradient_color0: vec4<f32>,
    @location(6) gradient_color1: vec4<f32>,
    @location(7) gradient_color2: vec4<f32>,
    @location(8) gradient_color3: vec4<f32>,
    @location(9) gradient_offsets: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) local_uv: vec2<f32>,
    @location(2) @interpolate(flat) paint_type: u32,
    @location(3) gradient_geo: vec4<f32>,
    @location(4) gradient_color0: vec4<f32>,
    @location(5) gradient_color1: vec4<f32>,
    @location(6) gradient_color2: vec4<f32>,
    @location(7) gradient_color3: vec4<f32>,
    @location(8) gradient_offsets: vec4<f32>,
};

@group(0) @binding(0)
var atlas_texture: texture_2d<f32>;
@group(0) @binding(1)
var atlas_sampler: sampler;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.tex_coord = in.tex_coord;
    out.local_uv = in.local_uv;
    out.paint_type = in.paint_type;
    out.gradient_geo = in.gradient_geo;
    out.gradient_color0 = in.gradient_color0;
    out.gradient_color1 = in.gradient_color1;
    out.gradient_color2 = in.gradient_color2;
    out.gradient_color3 = in.gradient_color3;
    out.gradient_offsets = in.gradient_offsets;
    return out;
}

// --- Duplicated verbatim from sdf.wgsl (see file header comment) ---

// Interpolate between 4 gradient stops at parameter t (0..1).
fn sample_gradient(
    t: f32,
    c0: vec4<f32>, c1: vec4<f32>, c2: vec4<f32>, c3: vec4<f32>,
    offsets: vec4<f32>,
) -> vec4<f32> {
    let tc = clamp(t, 0.0, 1.0);
    if (tc <= offsets.y) {
        let f = (tc - offsets.x) / max(offsets.y - offsets.x, 0.0001);
        return mix(c0, c1, clamp(f, 0.0, 1.0));
    } else if (tc <= offsets.z) {
        let f = (tc - offsets.y) / max(offsets.z - offsets.y, 0.0001);
        return mix(c1, c2, clamp(f, 0.0, 1.0));
    } else {
        let f = (tc - offsets.z) / max(offsets.w - offsets.z, 0.0001);
        return mix(c2, c3, clamp(f, 0.0, 1.0));
    }
}

const PI: f32 = 3.14159265359;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Coverage mask: the path atlas stores AA coverage in alpha (RGB is
    // opaque white, unused here) — see path_atlas::rasterize_path.
    let coverage = textureSample(atlas_texture, atlas_sampler, in.tex_coord).a;

    var fill_color: vec4<f32>;
    if (in.paint_type == 1u) {
        // Linear gradient: project UV onto start→end direction
        let start = in.gradient_geo.xy;
        let end = in.gradient_geo.zw;
        let dir = end - start;
        let len_sq = dot(dir, dir);
        let t = dot(in.local_uv - start, dir) / max(len_sq, 0.0001);
        fill_color = sample_gradient(t, in.gradient_color0, in.gradient_color1, in.gradient_color2, in.gradient_color3, in.gradient_offsets);
    } else if (in.paint_type == 2u) {
        // Radial gradient: distance from center, normalized by radius
        // gradient_geo.w contains aspect ratio (height/width) for correct elliptical mapping
        let center = in.gradient_geo.xy;
        let radius = in.gradient_geo.z;
        let aspect = in.gradient_geo.w;
        // Scale y-distance by inverse aspect so the gradient is circular in screen space
        let delta = in.local_uv - center;
        let corrected = vec2<f32>(delta.x, delta.y / max(aspect, 0.0001));
        let d = length(corrected);
        let t = d / max(radius, 0.0001);
        fill_color = sample_gradient(t, in.gradient_color0, in.gradient_color1, in.gradient_color2, in.gradient_color3, in.gradient_offsets);
    } else if (in.paint_type == 3u) {
        // Conic gradient: angle from center
        let center = in.gradient_geo.xy;
        let start_angle = in.gradient_geo.z;
        let delta = in.local_uv - center;
        var angle = atan2(delta.y, delta.x) - start_angle;
        if (angle < 0.0) { angle = angle + 2.0 * PI; }
        let t = angle / (2.0 * PI);
        fill_color = sample_gradient(t, in.gradient_color0, in.gradient_color1, in.gradient_color2, in.gradient_color3, in.gradient_offsets);
    } else {
        // paint_type == 0 (Solid) never reaches this pipeline — solid
        // path fills are routed through quad_pipeline instead (see
        // `path_gradient_quad_verts` in renderer.rs). Fall back to
        // transparent so a stray Solid entry doesn't paint garbage.
        fill_color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    return vec4<f32>(fill_color.rgb, fill_color.a * coverage);
}
