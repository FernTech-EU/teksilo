// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

// SDF rounded rectangle shader (Tier 2) with gradient support.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) local_uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) corner_radii: vec4<f32>,
    @location(4) shape_params: vec4<f32>, // [width, height, stroke_width, paint_type]
    @location(5) gradient_geo: vec4<f32>, // [start_x, start_y, end_x, end_y] or [cx, cy, radius, 0] or [cx, cy, angle, 0]
    @location(6) gradient_color0: vec4<f32>,
    @location(7) gradient_color1: vec4<f32>,
    @location(8) gradient_color2: vec4<f32>,
    @location(9) gradient_color3: vec4<f32>,
    @location(10) gradient_offsets: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) corner_radii: vec4<f32>,
    @location(3) shape_params: vec4<f32>,
    @location(4) gradient_geo: vec4<f32>,
    @location(5) gradient_color0: vec4<f32>,
    @location(6) gradient_color1: vec4<f32>,
    @location(7) gradient_color2: vec4<f32>,
    @location(8) gradient_color3: vec4<f32>,
    @location(9) gradient_offsets: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.local_uv = in.local_uv;
    out.color = in.color;
    out.corner_radii = in.corner_radii;
    out.shape_params = in.shape_params;
    out.gradient_geo = in.gradient_geo;
    out.gradient_color0 = in.gradient_color0;
    out.gradient_color1 = in.gradient_color1;
    out.gradient_color2 = in.gradient_color2;
    out.gradient_color3 = in.gradient_color3;
    out.gradient_offsets = in.gradient_offsets;
    return out;
}

// Signed distance to a rounded rectangle.
fn sd_rounded_rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

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
    let size = in.shape_params.xy;
    let stroke_width = in.shape_params.z;
    let paint_type = u32(in.shape_params.w + 0.5);
    let half_size = size * 0.5;

    // Map UV to local coordinates centered at origin
    let p = (in.local_uv - vec2<f32>(0.5, 0.5)) * size;

    // Select corner radius based on quadrant
    var r: f32;
    if (in.local_uv.x < 0.5) {
        if (in.local_uv.y < 0.5) {
            r = in.corner_radii.x;
        } else {
            r = in.corner_radii.w;
        }
    } else {
        if (in.local_uv.y < 0.5) {
            r = in.corner_radii.y;
        } else {
            r = in.corner_radii.z;
        }
    }

    let dist = sd_rounded_rect(p, half_size, r);

    // Determine fill color based on paint type
    var fill_color: vec4<f32>;
    if (paint_type == 1u) {
        // Linear gradient: project UV onto start→end direction
        let start = in.gradient_geo.xy;
        let end = in.gradient_geo.zw;
        let dir = end - start;
        let len_sq = dot(dir, dir);
        let t = dot(in.local_uv - start, dir) / max(len_sq, 0.0001);
        fill_color = sample_gradient(t, in.gradient_color0, in.gradient_color1, in.gradient_color2, in.gradient_color3, in.gradient_offsets);
    } else if (paint_type == 2u) {
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
    } else if (paint_type == 3u) {
        // Conic gradient: angle from center
        let center = in.gradient_geo.xy;
        let start_angle = in.gradient_geo.z;
        let delta = in.local_uv - center;
        var angle = atan2(delta.y, delta.x) - start_angle;
        if (angle < 0.0) { angle = angle + 2.0 * PI; }
        let t = angle / (2.0 * PI);
        fill_color = sample_gradient(t, in.gradient_color0, in.gradient_color1, in.gradient_color2, in.gradient_color3, in.gradient_offsets);
    } else {
        // Solid
        fill_color = in.color;
    }

    // Screen-space antialiasing band. `dist` is in shape units; under a
    // SceneView's view transform one shape unit maps to ~`zoom` device px, so
    // a fixed band (`smoothstep(-0.5, 0.5, …)`) would widen to ~zoom px and
    // blur edges (notably cosmetic strokes, which hold a constant device
    // width). `fwidth(dist)` is the on-screen gradient of `dist`, so a band of
    // `±0.5·fwidth` tracks ~1 device px at any scale. Capped at 0.5 so it
    // never EXCEEDS the previous fixed band: at zoom ≤ 1 (every non-scene
    // widget) `fwidth ≥ 1` ⇒ `aa = 0.5`, byte-identical to before; only
    // zoomed-in shapes get the sharper (smaller) band. The 1e-4 floor avoids a
    // hard step in flat regions.
    let aa = min(max(fwidth(dist), 1e-4) * 0.5, 0.5);
    if (stroke_width > 0.0) {
        let alpha = 1.0 - smoothstep(-aa, aa, abs(dist) - stroke_width * 0.5);
        return vec4<f32>(fill_color.rgb, fill_color.a * alpha);
    } else {
        let alpha = 1.0 - smoothstep(-aa, aa, dist);
        return vec4<f32>(fill_color.rgb, fill_color.a * alpha);
    }
}
