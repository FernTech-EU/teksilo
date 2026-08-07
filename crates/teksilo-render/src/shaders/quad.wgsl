// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

// Textured quad shader (Tier 1 — glyphs and images).
//
// The quad pipeline has two fragment paths, selected per-vertex via the
// `flags` attribute:
//
// * Bit 0 = 0 (monochrome glyph): the atlas region is an alpha mask with
//   RGB = white. The fragment multiplies the vertex color's RGB by the
//   texture alpha, tinting the glyph.
//
// * Bit 0 = 1 (color glyph / image): the atlas region holds a
//   pre-multiplied RGBA color bitmap (color emoji via COLR / CBDT / sbix,
//   or a real image). The fragment samples `texture.rgb` directly and
//   multiplies the sample's RGBA by the vertex color (acting as a global
//   opacity when vertex.color = [1, 1, 1, alpha]).

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coord: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) flags: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
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
    out.color = in.color;
    out.flags = in.flags;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(atlas_texture, atlas_sampler, in.tex_coord);
    if ((in.flags & 1u) != 0u) {
        // Color glyph / image: atlas holds the glyph's RGB. Keep the
        // sampled RGB and attenuate alpha by the vertex color's alpha —
        // straight-alpha compositing against the ALPHA_BLENDING target.
        return vec4<f32>(tex_color.rgb, tex_color.a * in.color.a);
    }
    // Monochrome glyph: texture is an alpha mask; tint with vertex RGB.
    return vec4<f32>(in.color.rgb, in.color.a * tex_color.a);
}
