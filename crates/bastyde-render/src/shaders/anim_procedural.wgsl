// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

// Shader-driven animated quads — procedural kinds (no texture).
//
// Each quad carries a `slot` per vertex (flat-interpolated) that the
// fragment shader uses to look up per-frame state from the
// `anim_uniforms` buffer. The widget tree's `AnimatedQuadRegistry`
// writes fresh `AnimParams` into that buffer every frame — the widget
// paint() runs only on layout changes, and this shader computes the
// animation pixels from `phase` alone.

// 64 bytes per slot; 128 slots = 8192 bytes. Well within the 64 KiB
// UBO cap on every backend. Bump (or switch to a storage buffer) if
// we ever need thousands of concurrent animated quads.
const MAX_ANIM_SLOTS: u32 = 128u;

// Must match `bastyde_canvas::AnimParams`.
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

// sRGB → linear for the uniform-carried colors. The renderer takes raw
// sRGB values from the tree (no CPU-side conversion like the other
// pipelines, because uniform data isn't vertex-packed through
// `srgb_to_linear_rgba`) — we do it here so `Rgba8UnormSrgb` target
// gets linear output and the hardware does the sRGB encode.
fn to_linear(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn srgb_to_linear_rgba(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(to_linear(c.r), to_linear(c.g), to_linear(c.b), c.a);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = anim.params[in.slot];
    switch (p.kind) {
        // IndeterminateSweep: a `sweep_ratio`-wide band moves
        // left→right across the bar, completing one traversal per
        // period. Band is fully off-screen at phase=0 (entirely to the
        // left) and at phase=1 (entirely to the right), so the loop
        // wraps cleanly with no visible jump.
        case 0u: {
            let left = p.phase * (1.0 + p.sweep_ratio) - p.sweep_ratio;
            let right = left + p.sweep_ratio;
            if (in.uv.x < left || in.uv.x > right) {
                discard;
            }
            return srgb_to_linear_rgba(p.color1);
        }
        // SpinnerArc: a `sweep_ratio`-portion of a circle rotating
        // around the centre with period = phase 0→1. Stroke
        // thickness is `_pad0` of the quad's min extent (uv is
        // already normalised so min extent is 1.0 here). Rendered
        // with leading edge at the top (theta=0) and rotating
        // clockwise — matches the CSS spinner convention. Edges
        // are anti-aliased via `fwidth`-driven smoothstep ramps so
        // the radial bounds and arc start/end fade out over a
        // single pixel instead of producing the visibly stair-stepped
        // outline a hard `discard` at each boundary would yield.
        case 2u: {
            let to_centre = in.uv - vec2<f32>(0.5);
            let dist = length(to_centre);
            let stroke = p._pad0;
            let outer = 0.5;
            let inner = 0.5 - stroke;
            let aa_radial = fwidth(dist);
            // Reject pixels that are clearly outside the ring even
            // accounting for the AA band — keeps the discard fast
            // path for the majority of the quad, which is empty.
            if (dist > outer + aa_radial || dist < inner - aa_radial) {
                discard;
            }
            let outer_alpha = smoothstep(outer + aa_radial, outer - aa_radial, dist);
            let inner_alpha = smoothstep(inner - aa_radial, inner + aa_radial, dist);
            let radial_alpha = outer_alpha * inner_alpha;
            // theta_raw normalised to 0..1, with 0 at the leftmost
            // point (atan2 returns -π..π). Shift by 0.75 so 0 lands
            // at the top — the conventional spinner anchor.
            let theta_raw = (atan2(to_centre.y, to_centre.x) + 3.14159265) / 6.28318530;
            let theta = fract(theta_raw + 0.75);
            // Distance behind the leading edge (mod 1). A fragment is
            // in the arc when this distance is less than the arc
            // length.
            let local = fract(theta - p.phase + 1.0);
            // Tangential pixel width in normalised-angle units
            // (0..1 = full circle). One UV pixel maps to `aa_uv`
            // along each axis; the corresponding angular span is
            // `aa_uv / (2π · dist)`. fwidth on `theta` directly
            // would explode at the wrap from 1.0→0.0 (the top of
            // the spinner), so we go through the screen-space UV.
            let aa_uv = max(fwidth(in.uv.x), fwidth(in.uv.y));
            let aa_theta = aa_uv / (6.28318530 * max(dist, 1e-3));
            let lead_alpha = smoothstep(0.0, aa_theta, local);
            let trail_alpha = smoothstep(p.sweep_ratio + aa_theta, p.sweep_ratio, local);
            let arc_alpha = lead_alpha * trail_alpha;
            let alpha = radial_alpha * arc_alpha;
            if (alpha < 1.0 / 255.0) {
                discard;
            }
            let c = srgb_to_linear_rgba(p.color1);
            return vec4<f32>(c.rgb, c.a * alpha);
        }
        default: {
            // Magenta fallback so an unknown `kind` is visually obvious
            // in development — better than silently rendering nothing.
            return vec4<f32>(1.0, 0.0, 1.0, 1.0);
        }
    }
}
