// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

// One level of a live texture's mip chain from the level below, for
// `ScalingFilter::Trilinear`. The kernel is `teksilo_canvas::resample::
// downsample_half`'s, the one static images get on the CPU: a 2x2 box in
// linear light, odd sides halving down and reads clamping to the last row
// and column. Prepended with `fullscreen.wgsl`.
//
// Every view is `Rgba8UnormSrgb`: `textureLoad` returns linear light and
// the attachment encodes it again, so no `view_formats` are needed. The
// kernel works per channel, so a BGR source needs nothing: the quad shader
// swaps red and blue after sampling.

@group(0) @binding(0) var src: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4<f32> {
    return fullscreen_position(idx);
}

struct Taps { a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32> };

fn taps(p: vec2<f32>) -> Taps {
    let last = vec2<i32>(textureDimensions(src)) - vec2<i32>(1, 1);
    let b = vec2<i32>(p) * 2;
    let x0 = min(b.x, last.x);
    let y0 = min(b.y, last.y);
    let x1 = min(b.x + 1, last.x);
    let y1 = min(b.y + 1, last.y);
    return Taps(
        textureLoad(src, vec2<i32>(x0, y0), 0),
        textureLoad(src, vec2<i32>(x1, y0), 0),
        textureLoad(src, vec2<i32>(x0, y1), 0),
        textureLoad(src, vec2<i32>(x1, y1), 0),
    );
}

// Straight-alpha sources: weight each texel's colour by its alpha, average,
// and divide by the summed alpha, so a transparent texel does not darken
// its neighbours.
@fragment
fn fs_straight(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let t = taps(pos.xy);
    let sa = t.a.a + t.b.a + t.c.a + t.d.a;
    if (sa <= 0.0) {
        return vec4<f32>(0.0);
    }
    let rgb = t.a.rgb * t.a.a + t.b.rgb * t.b.a + t.c.rgb * t.c.a + t.d.rgb * t.d.a;
    return vec4<f32>(rgb / sa, sa * 0.25);
}

// RGBX and BGRX sources: the fourth byte is not alpha (an emulator leaves
// it 0), so the average is plain and the level is opaque.
@fragment
fn fs_opaque(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let t = taps(pos.xy);
    return vec4<f32>((t.a.rgb + t.b.rgb + t.c.rgb + t.d.rgb) * 0.25, 1.0);
}
