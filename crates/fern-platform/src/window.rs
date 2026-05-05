use std::sync::Arc;
use std::sync::mpsc;

use winit::event::WindowEvent;
use winit::window::Window;

use accesskit::ActionRequest;
use fern_render::Renderer;

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

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fern_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let renderer = Renderer::new(device, queue, surface_format);

        // Create AccessKit adapter with action channel
        let (action_tx, action_rx) = mpsc::channel();

        let a11y_adapter = accesskit_winit::Adapter::with_direct_handlers(
            event_loop,
            &window,
            FernActivationHandler,
            FernActionHandler { tx: action_tx },
            FernDeactivationHandler,
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
        let surface = instance.create_surface(window.clone()).unwrap();

        let gpu_adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = gpu_adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fern_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&gpu_adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
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
    /// Used by `fern_platform::create_title_bar_host` and other components
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
        frame: &fern_canvas::RenderFrame,
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

        output.present();
        FrameOutcome::Rendered
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
struct FernActivationHandler;

impl accesskit::ActivationHandler for FernActivationHandler {
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
struct FernActionHandler {
    tx: mpsc::Sender<ActionRequest>,
}

impl accesskit::ActionHandler for FernActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        let _ = self.tx.send(request);
    }
}

/// Deactivation handler — no-op.
struct FernDeactivationHandler;

impl accesskit::DeactivationHandler for FernDeactivationHandler {
    fn deactivate_accessibility(&mut self) {
        // Nothing to clean up
    }
}
