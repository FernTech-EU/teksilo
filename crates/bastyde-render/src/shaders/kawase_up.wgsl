// Dual-Kawase blur — upsample pass.
//
// Samples the input texture at eight offsets (four cardinal +
// four diagonal, with different weights) around each destination
// texel and averages them. Combined with the matching downsample
// shader, this approximates a Gaussian blur at much lower cost than
// a separable Gaussian for radii ≳ 8 px.

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Params {
    offset: vec4<f32>,  // .zw unused (std140 alignment)
};

@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    let x = f32((idx << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(idx & 2u) * 2.0 - 1.0;
    var out: VertexOutput;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let off = params.offset.xy;
    // Four diagonal samples weighted 2.0
    let d1 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>(-off.x, -off.y)) * 2.0;
    let d2 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>( off.x, -off.y)) * 2.0;
    let d3 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>(-off.x,  off.y)) * 2.0;
    let d4 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>( off.x,  off.y)) * 2.0;
    // Four cardinal samples weighted 1.0
    let c1 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>(-off.x * 2.0, 0.0));
    let c2 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>( off.x * 2.0, 0.0));
    let c3 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>(0.0, -off.y * 2.0));
    let c4 = textureSample(source_texture, source_sampler, in.uv + vec2<f32>(0.0,  off.y * 2.0));
    return (d1 + d2 + d3 + d4 + c1 + c2 + c3 + c4) / 12.0;
}
