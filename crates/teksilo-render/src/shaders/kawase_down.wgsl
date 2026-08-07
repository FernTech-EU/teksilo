// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

// Dual-Kawase blur — downsample pass.
//
// Samples the input texture at four diagonal offsets around each
// destination texel and averages them. Combined with the matching
// upsample shader, this approximates a Gaussian blur at much lower
// cost than a separable Gaussian for radii ≳ 8 px.
//
// The vertex shader is the standard three-vertex full-screen-triangle
// trick: no vertex buffer, three calls produce a triangle that covers
// clip space (-1..3, -1..3) so the rasterizer clips it to the viewport.

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Params {
    // Texel size of the SOURCE texture in 0..1 UV units, scaled by the
    // sample-offset parameter. Pre-baked CPU-side so the shader stays
    // branch-free. `.zw` is unused (std140 alignment padding).
    offset: vec4<f32>,
};

@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    // Three vertices: (-1,-1), (3,-1), (-1,3). Forms a triangle that
    // completely covers the [-1, 1] clip square; the rasterizer clips
    // off the parts outside.
    let x = f32((idx << 1u) & 2u) * 2.0 - 1.0;  // -1, 3, -1
    let y = f32(idx & 2u) * 2.0 - 1.0;          // -1, -1, 3
    var out: VertexOutput;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    // Map clip XY (-1..1) → UV (0..1), flipping Y so origin is top-left
    // (matches the way the rest of the renderer hands UVs to textures).
    out.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let off = params.offset.xy;
    // Center sample weighted by 4, plus four corner samples each
    // weighted by 1, normalized by 8 — same kernel weights Marius
    // Bjørge published in the Siggraph 2015 dual-Kawase paper.
    let center = textureSample(source_texture, source_sampler, in.uv) * 4.0;
    let c1 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>(-off.x, -off.y));
    let c2 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>( off.x, -off.y));
    let c3 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>(-off.x,  off.y));
    let c4 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>( off.x,  off.y));
    return (center + c1 + c2 + c3 + c4) / 8.0;
}
