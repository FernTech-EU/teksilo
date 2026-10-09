// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

// The three-vertex full-screen triangle, prepended to every shader that
// draws one: no vertex buffer, and three calls produce a triangle with
// vertices (-1,-1), (3,-1), (-1,3) that covers the [-1, 1] clip square; the
// rasterizer clips off the parts outside. Shared by the Kawase blur passes
// and the live mip passes.

fn fullscreen_position(idx: u32) -> vec4<f32> {
    let x = f32((idx << 1u) & 2u) * 2.0 - 1.0;  // -1, 3, -1
    let y = f32(idx & 2u) * 2.0 - 1.0;          // -1, -1, 3
    return vec4<f32>(x, y, 0.0, 1.0);
}
