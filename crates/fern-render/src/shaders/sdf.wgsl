// SDF rounded rectangle shader (Tier 2).

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) local_uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) corner_radii: vec4<f32>,
    @location(4) shape_params: vec4<f32>, // [width, height, stroke_width, 0]
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) corner_radii: vec4<f32>,
    @location(3) shape_params: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.local_uv = in.local_uv;
    out.color = in.color;
    out.corner_radii = in.corner_radii;
    out.shape_params = in.shape_params;
    return out;
}

// Signed distance to a rounded rectangle.
// p: point in local coordinates (centered at origin)
// b: half-size of the rectangle
// r: corner radius for the current quadrant
fn sd_rounded_rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = in.shape_params.xy;
    let stroke_width = in.shape_params.z;
    let half_size = size * 0.5;

    // Map UV to local coordinates centered at origin
    let p = (in.local_uv - vec2<f32>(0.5, 0.5)) * size;

    // Select corner radius based on quadrant
    // corner_radii: [top_left, top_right, bottom_right, bottom_left]
    var r: f32;
    if (in.local_uv.x < 0.5) {
        if (in.local_uv.y < 0.5) {
            r = in.corner_radii.x; // top-left
        } else {
            r = in.corner_radii.w; // bottom-left
        }
    } else {
        if (in.local_uv.y < 0.5) {
            r = in.corner_radii.y; // top-right
        } else {
            r = in.corner_radii.z; // bottom-right
        }
    }

    let dist = sd_rounded_rect(p, half_size, r);

    if (stroke_width > 0.0) {
        // Stroke: visible when |dist| < stroke_width/2
        let alpha = 1.0 - smoothstep(-0.5, 0.5, abs(dist) - stroke_width * 0.5);
        return vec4<f32>(in.color.rgb, in.color.a * alpha);
    } else {
        // Fill: visible when dist < 0
        let alpha = 1.0 - smoothstep(-0.5, 0.5, dist);
        return vec4<f32>(in.color.rgb, in.color.a * alpha);
    }
}
