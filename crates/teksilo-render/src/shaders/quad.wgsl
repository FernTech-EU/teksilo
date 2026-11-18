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
//
// A monochrome glyph also carries `text_params` (0 for everything else): the
// coverage is remapped by `text_coverage` before the blend, so its edge
// looks blended in a perceptual space rather than in the linear light the
// sRGB target blends in. `text_contrast.rs` explains it and holds the CPU
// twin of `text_coverage`, which must stay identical to it.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coord: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) flags: u32,
    @location(4) text_params: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) @interpolate(flat) text_params: u32,
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
    out.text_params = in.text_params;
    return out;
}

fn srgb_encode(x: f32) -> f32 {
    let c = clamp(x, 0.0, 1.0);
    if (c <= 0.0031308) {
        return c * 12.92;
    }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

fn srgb_decode(x: f32) -> f32 {
    let c = clamp(x, 0.0, 1.0);
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

// The GPU twin of `TextContrast::corrected_coverage` (text_contrast.rs).
fn text_coverage(raw: f32, text_rgb: vec3<f32>, params: u32) -> f32 {
    if (params == 0u) {
        return raw;
    }
    let unpacked = unpack2x16unorm(params);
    let gamma = unpacked.x * 4.0;
    let contrast = unpacked.y;
    let coverage = clamp(raw, 0.0, 1.0);
    let src_lin = dot(text_rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let src = srgb_encode(src_lin);
    // The page is unknown; take it to be the text's opposite.
    let dst = 1.0 - src;
    let dst_lin = srgb_decode(dst);
    let dst_g = pow(dst, gamma);
    let c = contrast * dst_g;
    let a = coverage + (1.0 - coverage) * c * coverage;
    if (abs(src - dst) < 1.0 / 256.0) {
        return clamp(a, 0.0, 1.0);
    }
    let src_g = pow(src, gamma);
    let out = pow(max(dst_g + (src_g - dst_g) * a, 0.0), 1.0 / gamma);
    return clamp((srgb_decode(out) - dst_lin) / (src_lin - dst_lin), 0.0, 1.0);
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
    let coverage = text_coverage(tex_color.a, in.color.rgb, in.text_params);
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}
