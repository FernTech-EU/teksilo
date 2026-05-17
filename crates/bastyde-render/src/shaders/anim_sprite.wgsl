// Shader-driven animated quads — sprite-atlas kinds.
//
// Same vertex shape and uniform buffer as anim_procedural.wgsl, with
// an additional bind group (group 1) carrying the sprite atlas
// texture + sampler. The fragment shader decodes the current frame
// index from `AnimParams.phase`, maps `uv` to the atlas cell, and
// samples.

const MAX_ANIM_SLOTS: u32 = 128u;

// Must match `bastyde_canvas::AnimParams` (shared with anim_procedural).
struct AnimParams {
    kind: u32,
    phase: f32,
    sweep_ratio: f32,
    _pad0: f32,
    color0: vec4<f32>,
    color1: vec4<f32>,
    atlas_cols: f32,
    atlas_rows: f32,
    _pad1: vec2<f32>,
};

struct AnimUniforms {
    params: array<AnimParams, 128>,
};

@group(0) @binding(0) var<uniform> anim: AnimUniforms;
@group(1) @binding(0) var atlas_texture: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) slot: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) slot: u32,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.slot = in.slot;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = anim.params[in.slot];
    // Sprite path: `phase` encodes the integer frame index as f32
    // (wrote by `AnimatedQuadRegistry::compute_params`). Map uv to
    // the frame's cell in the atlas grid.
    let frame_idx = u32(p.phase);
    let cols = u32(max(p.atlas_cols, 1.0));
    let rows = u32(max(p.atlas_rows, 1.0));
    let clamped = min(frame_idx, cols * rows - 1u);
    let col = clamped % cols;
    let row = clamped / cols;
    let cell_uv = vec2<f32>(
        (f32(col) + in.uv.x) / f32(cols),
        (f32(row) + in.uv.y) / f32(rows),
    );
    let sampled = textureSample(atlas_texture, atlas_sampler, cell_uv);
    // `color1` is the tint: alpha=0 means no tint, leave sprite as-is;
    // alpha>0 means multiply sprite RGBA by the tint (premultiplied
    // against the tint's alpha for opacity).
    if (p.color1.a > 0.0) {
        return vec4<f32>(
            sampled.rgb * p.color1.rgb,
            sampled.a * p.color1.a,
        );
    }
    return sampled;
}
