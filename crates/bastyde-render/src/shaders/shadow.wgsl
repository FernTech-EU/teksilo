// Shadow shader — renders soft box shadows using SDF + Gaussian blur approximation.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) local_uv: vec2<f32>,
    @location(2) shadow_color: vec4<f32>,
    @location(3) corner_radii: vec4<f32>,
    // [shape_width, shape_height, blur_radius, spread]
    @location(4) shadow_params: vec4<f32>,
    // [shape_offset_x, shape_offset_y, 0, 0] — offset of inner shape within the shadow quad
    @location(5) shape_offset: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_uv: vec2<f32>,
    @location(1) shadow_color: vec4<f32>,
    @location(2) corner_radii: vec4<f32>,
    @location(3) shadow_params: vec4<f32>,
    @location(4) shape_offset: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.local_uv = in.local_uv;
    out.shadow_color = in.shadow_color;
    out.corner_radii = in.corner_radii;
    out.shadow_params = in.shadow_params;
    out.shape_offset = in.shape_offset;
    return out;
}

// Signed distance to a rounded rectangle.
fn sd_rounded_rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

// Approximate Gaussian integral using erf-like function.
// sigma = blur_radius / 2.5 gives a good visual match.
fn gaussian_alpha(dist: f32, sigma: f32) -> f32 {
    // Use smoothstep as a fast approximation of the Gaussian CDF
    // The transition happens over roughly 3*sigma
    return 1.0 - smoothstep(-sigma * 1.5, sigma * 1.5, dist);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let shape_w = in.shadow_params.x;
    let shape_h = in.shadow_params.y;
    let blur_radius = in.shadow_params.z;
    let spread = in.shadow_params.w;

    // The shadow quad is larger than the shape. We need to compute the
    // distance from the current pixel to the inner shape boundary.
    // shape_offset tells us where the inner shape center is within the shadow quad.
    let shadow_quad_size = vec2<f32>(
        shape_w + (blur_radius + spread) * 2.0,
        shape_h + (blur_radius + spread) * 2.0,
    );

    // Map UV to shadow quad coordinates centered at origin
    let p_shadow = (in.local_uv - vec2<f32>(0.5, 0.5)) * shadow_quad_size;

    // Offset to get position relative to inner shape center
    let p = p_shadow - vec2<f32>(in.shape_offset.x, in.shape_offset.y);

    // Inner shape half-size (expanded by spread)
    let half_shape = vec2<f32>((shape_w + spread * 2.0) * 0.5, (shape_h + spread * 2.0) * 0.5);

    // Select corner radius based on quadrant of the inner shape
    var r: f32;
    if (p.x < 0.0) {
        if (p.y < 0.0) {
            r = in.corner_radii.x; // top-left
        } else {
            r = in.corner_radii.w; // bottom-left
        }
    } else {
        if (p.y < 0.0) {
            r = in.corner_radii.y; // top-right
        } else {
            r = in.corner_radii.z; // bottom-right
        }
    }

    let dist = sd_rounded_rect(p, half_shape, r);

    // Use blur_radius to control the softness
    let sigma = max(blur_radius / 2.5, 0.5);
    let alpha = gaussian_alpha(dist, sigma);

    return vec4<f32>(in.shadow_color.rgb, in.shadow_color.a * alpha);
}
