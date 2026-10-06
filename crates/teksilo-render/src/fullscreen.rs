// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One full-screen-triangle pass into a target: the blur's Kawase passes and
//! the live mip passes. Their shaders are prepended with
//! `shaders/fullscreen.wgsl`.

/// Run `pipeline` with `bind_group` (and its dynamic offsets) into `target`,
/// over a `width × height` viewport: the target may be larger, and only the
/// part the viewport covers is written. `load` clears or keeps what the
/// target holds; `scissor` (x, y, width, height) limits the pass further.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    dynamic_offsets: &[u32],
    target: &wgpu::TextureView,
    width: u32,
    height: u32,
    load: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<[u32; 4]>,
    label: &str,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, dynamic_offsets);
    pass.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
    if let Some([x, y, w, h]) = scissor {
        pass.set_scissor_rect(x, y, w, h);
    }
    pass.draw(0..3, 0..1);
}
