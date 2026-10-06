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
// * Bit 0 = 1 (color glyph / image): the region holds an RGBA color bitmap
//   with straight (not premultiplied) alpha: a color emoji (COLR / CBDT /
//   sbix), an image or a live picture. The fragment keeps the sampled RGB
//   and multiplies its alpha by the vertex color's (a global opacity when
//   vertex.color = [1, 1, 1, alpha]).
//
// Two more bits, for a live picture's byte order, act on the sample before
// either path:
//
// * Bit 1 (opaque): the fourth byte is not alpha (RGBX, BGRX): alpha = 1.
// * Bit 2 (swap red and blue): the texture holds B, G, R in its R, G, B
//   channels (BGRA, BGRX). Exact: filtering and sRGB decoding act on each
//   channel alone.

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
    // Sampled before any branch: uniform control flow for the derivatives.
    var tex_color = textureSample(atlas_texture, atlas_sampler, in.tex_coord);
    if ((in.flags & 4u) != 0u) {
        tex_color = tex_color.bgra;
    }
    if ((in.flags & 2u) != 0u) {
        tex_color.a = 1.0;
    }
    if ((in.flags & 1u) != 0u) {
        // Color glyph / image: atlas holds the glyph's RGB. Keep the
        // sampled RGB and attenuate alpha by the vertex color's alpha —
        // straight-alpha compositing against the ALPHA_BLENDING target.
        return vec4<f32>(tex_color.rgb, tex_color.a * in.color.a);
    }
    // Monochrome glyph: texture is an alpha mask; tint with vertex RGB.
    return vec4<f32>(in.color.rgb, in.color.a * tex_color.a);
}
