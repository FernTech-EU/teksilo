// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::sync::Arc;
use std::sync::mpsc;

use winit::event::WindowEvent;
use winit::window::Window;

use accesskit::ActionRequest;
use teksilo_render::Renderer;

/// Error returned when surface texture acquisition fails during rendering.
#[derive(Debug, thiserror::Error)]
#[error("Surface error: {0}")]
pub struct SurfaceRenderError(pub String);

/// Outcome of [`PlatformWindow::render_frame`]. Mirrors the wgpu
/// surface-status cases that matter to the caller so the app loop can
/// decide how to respond (ignore, reconfigure, log) without every frame
/// getting logged as an error.
#[derive(Debug)]
pub enum FrameOutcome {
    /// Frame was rendered and presented.
    Rendered,
    /// wgpu reported the window as occluded or the acquire timed out.
    /// Per wgpu guidance, skip this frame. On macOS, the initial paint
    /// after window creation often hits `Occluded` one or more times
    /// before Metal finishes compositing, so the caller should still
    /// request another redraw once — unless it already knows the
    /// window is occluded via `WindowEvent::Occluded(true)`.
    Skipped,
    /// Surface became outdated (resize, scale change, device switch).
    /// Caller should reconfigure the surface and try again.
    NeedsReconfigure,
    /// Acquisition failed with a non-transient error.
    Error(SurfaceRenderError),
}

/// A platform window wrapping a winit window, wgpu surface, renderer,
/// and AccessKit adapter for screen reader support.
pub struct PlatformWindow {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    scale_factor: f64,
    a11y_adapter: Option<accesskit_winit::Adapter>,
    /// Receiver for accessibility action requests from the adapter.
    a11y_action_rx: mpsc::Receiver<ActionRequest>,
}

impl PlatformWindow {
    /// Create a new platform window from a winit window.
    /// The `event_loop` parameter is needed for the AccessKit adapter.
    pub async fn new_with_a11y(
        window: Window,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Self {
        let window = Arc::new(window);
        let size = window.inner_size();
        let scale_factor = window.scale_factor();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        let surface = instance
            .create_surface(window.clone())
            .expect("wgpu surface creation failed for the platform window");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .expect("no compatible wgpu adapter available");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("teksilo_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .expect("wgpu device request failed");

        let surface_caps = surface.get_capabilities(&adapter);
        // Guard the index accesses: a degenerate adapter/surface (software
        // fallback, headless) can report empty `formats` / `alpha_modes`, and
        // `[0]` would panic with an opaque out-of-bounds instead of degrading.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .or_else(|| surface_caps.formats.first().copied())
            .unwrap_or(wgpu::TextureFormat::Rgba8UnormSrgb);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            // `Auto` reproduces wgpu's pre-30 behaviour: sRGB for the
            // non-`Rgba16Float` formats we select above.
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &surface_config);

        let renderer = Renderer::new(device, queue, surface_format);

        // Create AccessKit adapter with action channel
        let (action_tx, action_rx) = mpsc::channel();

        let a11y_adapter = accesskit_winit::Adapter::with_direct_handlers(
            event_loop,
            &window,
            TeksiloActivationHandler,
            TeksiloActionHandler { tx: action_tx },
            TeksiloDeactivationHandler,
        );

        // Show the window now that the adapter is created
        window.set_visible(true);

        Self {
            window,
            surface,
            surface_config,
            renderer,
            scale_factor,
            a11y_adapter: Some(a11y_adapter),
            a11y_action_rx: action_rx,
        }
    }

    /// Create a platform window without AccessKit (for contexts without ActiveEventLoop).
    pub async fn new(window: Window) -> Self {
        let window = Arc::new(window);
        let size = window.inner_size();
        let scale_factor = window.scale_factor();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .expect("wgpu surface creation failed for the platform window");

        let gpu_adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .expect("no compatible wgpu adapter available");

        let (device, queue) = gpu_adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("teksilo_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .expect("wgpu device request failed");

        let surface_caps = surface.get_capabilities(&gpu_adapter);
        // See the sibling path above: guard against an empty `formats` /
        // `alpha_modes` rather than panicking on `[0]`.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .or_else(|| surface_caps.formats.first().copied())
            .unwrap_or(wgpu::TextureFormat::Rgba8UnormSrgb);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            // `Auto` reproduces wgpu's pre-30 behaviour: sRGB for the
            // non-`Rgba16Float` formats we select above.
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &surface_config);

        let renderer = Renderer::new(device, queue, surface_format);
        let (_action_tx, action_rx) = mpsc::channel();

        Self {
            window,
            surface,
            surface_config,
            renderer,
            scale_factor,
            a11y_adapter: None,
            a11y_action_rx: action_rx,
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Get a clonable `Arc` reference to the underlying winit window.
    /// Used by `teksilo_platform::create_title_bar_host` and other components
    /// that need shared ownership of the window.
    pub fn window_arc(&self) -> Arc<Window> {
        self.window.clone()
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    pub fn set_scale_factor(&mut self, factor: f64) {
        self.scale_factor = factor;
    }

    /// Resize the surface.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface
                .configure(self.renderer.device(), &self.surface_config);
        }
    }

    /// Get current surface dimensions.
    pub fn surface_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }

    /// Reconfigure the surface with the current config.
    /// Use after a Lost or Outdated surface error.
    pub fn reconfigure_surface(&mut self) {
        self.surface
            .configure(self.renderer.device(), &self.surface_config);
    }

    /// Render a frame to the surface.
    pub fn render_frame(
        &mut self,
        frame: &teksilo_canvas::RenderFrame,
        clear_color: [f32; 4],
    ) -> FrameOutcome {
        let current = self.surface.get_current_texture();
        let output = match current {
            wgpu::CurrentSurfaceTexture::Success(tex)
            | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => tex,
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return FrameOutcome::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return FrameOutcome::NeedsReconfigure;
            }
            other => return FrameOutcome::Error(SurfaceRenderError(format!("{other:?}"))),
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let (w, h) = self.surface_size();
        self.renderer
            .render(frame, &view, self.scale_factor as f32, w, h, clear_color);

        self.renderer.queue().present(output);
        FrameOutcome::Rendered
    }

    /// Render `frame` into an offscreen texture and read it back as
    /// tightly-packed RGBA8 bytes, returning `(rgba, width, height)`.
    ///
    /// Used by the debug-only automation bridge to capture a *live* window
    /// without going through the swapchain — the surface texture is
    /// configured `RENDER_ATTACHMENT` only (no `COPY_SRC`), so it can't be
    /// read back directly. The offscreen texture uses the window's own
    /// surface format so it matches the renderer's pipelines; a BGRA
    /// readback is swizzled to RGBA here so the output is always RGBA. With
    /// `crop = Some(rect)` (physical pixels, clamped to the surface) only
    /// that sub-rectangle is returned. Returns an empty `(vec, 0, 0)` if
    /// the crop is fully outside the surface.
    ///
    /// Note: a native `WebView` subview composites *on top of* the wgpu
    /// surface and is invisible to this readback (a transparent hole).
    pub fn capture_offscreen(
        &mut self,
        frame: &teksilo_canvas::RenderFrame,
        clear_color: [f32; 4],
        crop: Option<teksilo_canvas::Rect>,
    ) -> (Vec<u8>, u32, u32) {
        fn crop_rgba(
            src: &[u8],
            w: u32,
            h: u32,
            rect: teksilo_canvas::Rect,
        ) -> (Vec<u8>, u32, u32) {
            let x0 = (rect.x.floor().max(0.0) as u32).min(w);
            let y0 = (rect.y.floor().max(0.0) as u32).min(h);
            let x1 = ((rect.x + rect.width).ceil().max(0.0) as u32).min(w);
            let y1 = ((rect.y + rect.height).ceil().max(0.0) as u32).min(h);
            if x1 <= x0 || y1 <= y0 {
                return (Vec::new(), 0, 0);
            }
            let cw = x1 - x0;
            let ch = y1 - y0;
            let mut out = Vec::with_capacity((cw * ch * 4) as usize);
            for y in y0..y1 {
                let row_start = ((y * w + x0) * 4) as usize;
                let row_end = row_start + (cw * 4) as usize;
                out.extend_from_slice(&src[row_start..row_end]);
            }
            (out, cw, ch)
        }

        let (w, h) = self.surface_size();
        let format = self.surface_config.format;
        // The readback assumes a 4-byte, 8-bit RGBA/BGRA layout (the BGRA
        // swizzle below + `read_texture_rgba`'s fixed 4-bytes-per-pixel copy).
        // Desktop wgpu surfaces are always one of these four; a packed
        // (Rgb10a2) or wide (Rgba16Float) surface format would read back
        // garbage, so flag it loudly in debug builds.
        debug_assert!(
            matches!(
                format,
                wgpu::TextureFormat::Rgba8Unorm
                    | wgpu::TextureFormat::Rgba8UnormSrgb
                    | wgpu::TextureFormat::Bgra8Unorm
                    | wgpu::TextureFormat::Bgra8UnormSrgb
            ),
            "capture_offscreen: unsupported surface format {format:?} (expected 8-bit RGBA/BGRA)"
        );
        let texture = self
            .renderer
            .device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("teksilo-automation capture"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer
            .render(frame, &view, self.scale_factor as f32, w, h, clear_color);
        let mut bytes = teksilo_render::test_support::read_texture_rgba(
            self.renderer.device(),
            self.renderer.queue(),
            &texture,
            w,
            h,
        );
        // `read_texture_rgba` copies raw channel bytes; a BGRA surface
        // needs its B/R swapped to become RGBA for PNG encoding.
        if matches!(
            format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for px in bytes.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        match crop {
            Some(rect) => crop_rgba(&bytes, w, h, rect),
            None => (bytes, w, h),
        }
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Push an AccessKit TreeUpdate to the adapter (called after layout).
    pub fn update_accessibility(&mut self, update: accesskit::TreeUpdate) {
        if let Some(adapter) = &mut self.a11y_adapter {
            adapter.update_if_active(|| update);
        }
    }

    /// Forward a winit WindowEvent to the AccessKit adapter.
    pub fn process_accessibility_event(&mut self, event: &WindowEvent) {
        if let Some(adapter) = &mut self.a11y_adapter {
            adapter.process_event(&self.window, event);
        }
    }

    /// Drain any pending AccessKit action requests from the adapter.
    pub fn drain_accessibility_actions(&self) -> Vec<ActionRequest> {
        let mut actions = Vec::new();
        while let Ok(req) = self.a11y_action_rx.try_recv() {
            actions.push(req);
        }
        actions
    }
}

// --- AccessKit handler implementations ---

/// Activation handler — returns an empty initial tree.
/// The real tree is sent via `update_if_active` on the next frame.
struct TeksiloActivationHandler;

impl accesskit::ActivationHandler for TeksiloActivationHandler {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        // Return a minimal tree; the real one arrives on the next frame
        let root = accesskit::Node::new(accesskit::Role::Window);
        Some(accesskit::TreeUpdate {
            nodes: vec![(accesskit::NodeId(0), root)],
            tree: Some(accesskit::Tree::new(accesskit::NodeId(0))),
            tree_id: accesskit::TreeId::ROOT,
            focus: accesskit::NodeId(0),
        })
    }
}

/// Action handler — forwards action requests to the main thread via a channel.
struct TeksiloActionHandler {
    tx: mpsc::Sender<ActionRequest>,
}

impl accesskit::ActionHandler for TeksiloActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        let _ = self.tx.send(request);
    }
}

/// Deactivation handler — no-op.
struct TeksiloDeactivationHandler;

impl accesskit::DeactivationHandler for TeksiloDeactivationHandler {
    fn deactivate_accessibility(&mut self) {
        // Nothing to clean up
    }
}
