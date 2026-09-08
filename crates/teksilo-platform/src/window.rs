// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

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
    /// Set by the activation handler when an assistive technology asks for
    /// the tree; cleared by the first delivery after it. Shared because the
    /// handler may run off the main thread.
    a11y_needs_full_tree: Arc<AtomicBool>,
    /// Receiver for accessibility action requests from the adapter.
    a11y_action_rx: mpsc::Receiver<ActionRequest>,
    /// The accessibility state the adapter's off-thread handlers share with
    /// the UI thread. See [`AccessibilityBridge`].
    a11y_bridge: Arc<AccessibilityBridge>,
}

/// The state an AccessKit adapter's handlers share with the UI thread.
///
/// `accesskit_winit::Adapter::with_direct_handlers` requires every handler to
/// be `Send` and calls it from whatever thread the platform's accessibility
/// stack happens to use — the UIA provider thread on Windows, an AT-SPI task on
/// Linux. A [`teksilo_core::WidgetTree`] is `!Send`, so no handler can reach
/// one. Everything they need to say to the UI thread therefore goes through
/// this, and everything they need to read from it is a snapshot the UI thread
/// leaves here.
/// The whole policy lives here rather than in the three handler types,
/// because a handler owns an `Arc<Window>` and so cannot be built in a test
/// without an event loop, while this can.
#[derive(Debug, Default)]
pub(crate) struct AccessibilityBridge {
    /// The most recent `TreeUpdate` the UI thread published, kept so that
    /// `request_initial_tree` can answer with the real tree instead of a
    /// placeholder. `None` before the first frame.
    snapshot: Mutex<Option<accesskit::TreeUpdate>>,
    /// Whether an AccessKit client is attached right now. Set on activation,
    /// cleared on deactivation.
    active: std::sync::atomic::AtomicBool,
}

impl AccessibilityBridge {
    /// Leave a tree where the activation handler can find it. Called from the
    /// UI thread on every published update.
    ///
    /// A no-op while a client is attached, and that is the point: the snapshot
    /// is read by `request_initial_tree` alone, which by definition runs while
    /// nothing is attached — an attached client already has the live tree
    /// through `update_if_active`. Skipping the clone there keeps the cost off
    /// the frame path exactly when a screen reader is running and frames matter
    /// most. The window between a detach and the next frame leaves the snapshot
    /// one frame stale, which is a frame-old application rather than an empty
    /// one; the deactivation handler asks for that frame.
    pub(crate) fn publish(&self, update: &accesskit::TreeUpdate) {
        if self.is_active() {
            return;
        }
        if let Ok(mut slot) = self.snapshot.lock() {
            *slot = Some(update.clone());
        }
    }

    /// A client attached: record it and answer with the best tree available.
    ///
    /// The last published one if there is one — an assistive technology
    /// attaching to an idle window must not be shown an empty application —
    /// and the bare window node only before this window has ever drawn.
    pub(crate) fn on_activate(&self) -> accesskit::TreeUpdate {
        self.active
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.snapshot
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .unwrap_or_else(empty_initial_tree)
    }

    /// The last client detached.
    pub(crate) fn on_deactivate(&self) {
        self.active
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether a client is attached right now.
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// The wgpu objects every window in the process shares.
///
/// All three are `Arc` handles internally, so cloning one is a refcount bump,
/// not a second GPU object.
#[derive(Clone)]
struct SharedGpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// The one wgpu instance for this process.
///
/// A surface has to come from the same instance that later enumerates adapters
/// for it, so this is the root every window hangs off. `Instance::new` is
/// synchronous, which is why this one can be a plain `OnceLock` while the
/// adapter and device below cannot.
fn shared_instance() -> &'static wgpu::Instance {
    static INSTANCE: OnceLock<wgpu::Instance> = OnceLock::new();
    INSTANCE
        .get_or_init(|| wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle()))
}

/// The adapter, device and queue every window shares.
///
/// One device per process, not one per window. A device is a heavyweight,
/// process-level object and a second one buys nothing: each window still needs
/// its own surface and its own [`Renderer`] (that is where the glyph and path
/// atlases live), but the driver objects underneath are the same for every
/// window on the same adapter. Opening one per window duplicated the entire
/// pipeline set and both atlas textures for every window a user opened.
///
/// It also closes a latent crash. Two D3D12 **WARP** devices rasterizing at the
/// same time fault inside `d3d10warp.dll` — Microsoft's software rasterizer,
/// and what a GPU-less Windows host actually draws with. Teksilo renders its
/// windows sequentially on the winit main thread, so that was not reachable
/// here; it would have become reachable the moment any window work moved off
/// that thread. `teksilo_render::test_support` shares its offscreen device for
/// the same reason, where it *was* reachable and did crash.
///
/// `surface` is used only to pick an adapter that can actually present to it.
/// If a later window's surface turns out to be incompatible with the adapter we
/// cached — a genuinely multi-GPU machine, where the second window opens on the
/// other GPU — that window quietly gets its own device rather than failing.
async fn shared_gpu_for(surface: &wgpu::Surface<'static>) -> SharedGpu {
    static SHARED: Mutex<Option<SharedGpu>> = Mutex::new(None);

    // Clone out and release the lock: it is never held across the awaits below.
    let cached = SHARED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(gpu) = cached {
        // A non-empty format list is wgpu's own answer to "can this adapter
        // present to this surface".
        if !surface.get_capabilities(&gpu.adapter).formats.is_empty() {
            return gpu;
        }
    }

    let adapter = shared_instance()
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(surface),
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

    let gpu = SharedGpu {
        adapter,
        device,
        queue,
    };
    // First one in becomes the shared device. Losing here is the multi-GPU case
    // above (or a race that cannot happen while windows are created on one
    // thread): the loser keeps the device it just opened, which is the old
    // per-window behaviour and still correct.
    let mut slot = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = Some(gpu.clone());
    }
    gpu
}

impl PlatformWindow {
    /// Everything both constructors do: surface, shared device, swapchain
    /// configuration, renderer. Kept in one place because the two entry points
    /// differ only in whether they attach an AccessKit adapter, and sixty
    /// duplicated lines of GPU setup is exactly the sort of thing that drifts.
    async fn surface_and_renderer(
        window: &Arc<Window>,
    ) -> (wgpu::Surface<'static>, wgpu::SurfaceConfiguration, Renderer) {
        let size = window.inner_size();
        let surface = shared_instance()
            .create_surface(window.clone())
            .expect("wgpu surface creation failed for the platform window");

        let gpu = shared_gpu_for(&surface).await;

        let surface_caps = surface.get_capabilities(&gpu.adapter);
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
        surface.configure(&gpu.device, &surface_config);

        // The renderer stays per-window: it owns the glyph atlas, the path
        // atlas and the blur pool, and it is `!Sync` besides.
        let renderer = Renderer::new(gpu.device, gpu.queue, surface_format);
        (surface, surface_config, renderer)
    }

    /// Create a new platform window from a winit window.
    /// The `event_loop` parameter is needed for the AccessKit adapter.
    pub async fn new_with_a11y(
        window: Window,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Self {
        let window = Arc::new(window);
        let scale_factor = window.scale_factor();
        let (surface, surface_config, renderer) = Self::surface_and_renderer(&window).await;

        // Create AccessKit adapter with action channel
        let (action_tx, action_rx) = mpsc::channel();

        let a11y_needs_full_tree = Arc::new(AtomicBool::new(true));
        let a11y_bridge = Arc::new(AccessibilityBridge::default());

        // Every handler below runs off the UI thread and ends by asking winit
        // to redraw this window. That request is the *only* thing that wakes
        // the event loop: `handle_accessibility_actions` — the sole drain of
        // the action channel — runs from `window_event`, so without a wakeup an
        // action issued by Narrator or Orca would sit in the channel until some
        // unrelated window event happened to arrive. `Window::request_redraw`
        // is thread-safe, which is why an `Arc<Window>` clone is all a handler
        // needs.
        let a11y_adapter = accesskit_winit::Adapter::with_direct_handlers(
            event_loop,
            &window,
            TeksiloActivationHandler {
                needs_full_tree: a11y_needs_full_tree.clone(),
                bridge: Arc::clone(&a11y_bridge),
                window: Arc::clone(&window),
            },
            TeksiloActionHandler {
                tx: action_tx,
                window: Arc::clone(&window),
            },
            TeksiloDeactivationHandler {
                bridge: Arc::clone(&a11y_bridge),
                window: Arc::clone(&window),
            },
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
            a11y_needs_full_tree,
            a11y_bridge,
        }
    }

    /// Create a platform window without AccessKit (for contexts without ActiveEventLoop).
    pub async fn new(window: Window) -> Self {
        let window = Arc::new(window);
        let scale_factor = window.scale_factor();
        let (surface, surface_config, renderer) = Self::surface_and_renderer(&window).await;
        let (_action_tx, action_rx) = mpsc::channel();

        Self {
            window,
            surface,
            surface_config,
            renderer,
            scale_factor,
            a11y_adapter: None,
            a11y_action_rx: action_rx,
            a11y_needs_full_tree: Arc::new(AtomicBool::new(false)),
            a11y_bridge: Arc::new(AccessibilityBridge::default()),
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
            for px in bytes.as_chunks_mut::<4>().0 {
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
    /// Publish a freshly built `TreeUpdate` to the adapter, and leave a copy
    /// where the activation handler can find it.
    ///
    /// The copy is what lets an assistive technology that attaches to an *idle*
    /// window see the application instead of an empty window node: the handler
    /// runs off the UI thread and cannot build a tree, so the last one the UI
    /// thread built is the best answer available synchronously.
    pub fn update_accessibility(&mut self, update: accesskit::TreeUpdate) {
        self.a11y_bridge.publish(&update);
        if let Some(adapter) = &mut self.a11y_adapter {
            adapter.update_if_active(|| update);
        }
    }

    /// Push an update the adapter builds only when it is actually going to
    /// be delivered, and only when `build` says there is one worth sending.
    ///
    /// The caller decides *inside* the closure, because that is where the
    /// decision belongs: `update_if_active` runs its closure only when an
    /// assistive technology is attached, and on Linux it runs it under the
    /// adapter's own state lock. Deciding outside would build a tree for
    /// nobody on every frame, and would make the throttle count frames
    /// nothing was listening to.
    ///
    /// `build` returning `None` means "nothing to deliver"; the previously
    /// delivered tree is re-sent, which the consumer treats as a no-op.
    pub fn update_accessibility_with(
        &mut self,
        build: impl FnOnce() -> Option<accesskit::TreeUpdate>,
        previous: impl FnOnce() -> accesskit::TreeUpdate,
    ) {
        if let Some(adapter) = &mut self.a11y_adapter {
            adapter.update_if_active(|| build().unwrap_or_else(previous));
        }
    }

    /// Whether an assistive technology has asked this window for its tree
    /// and has not yet been given a full one.
    ///
    /// Set by the activation handler, which runs on whichever thread the
    /// platform's accessibility layer calls it from, and cleared by the
    /// first delivery after it — so a reader that attaches mid-session gets
    /// a complete tree rather than a geometry patch onto a tree it has
    /// never seen.
    pub fn accessibility_needs_full_tree(&self) -> bool {
        self.a11y_needs_full_tree.load(Ordering::Relaxed)
    }

    /// Clear the flag above, reporting what it was.
    pub fn take_accessibility_needs_full_tree(&self) -> bool {
        self.a11y_needs_full_tree.swap(false, Ordering::Relaxed)
    }

    /// Whether an AccessKit client is attached to this window's adapter.
    ///
    /// True from the moment the platform accessibility stack asks for an
    /// initial tree until it says it has gone away. Read once per frame by
    /// `teksilo-app` and pushed into the window's tree; see
    /// [`WidgetTree::set_at_client_attached`](teksilo_core::WidgetTree::set_at_client_attached)
    /// for why attaching and detaching are read asymmetrically.
    ///
    /// Always `false` for a window built without an adapter
    /// ([`PlatformWindow::new`]).
    pub fn accessibility_active(&self) -> bool {
        self.a11y_bridge.is_active()
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

/// Activation handler — answers with the last tree the UI thread built.
///
/// An assistive technology attaching to a window that is sitting idle used to
/// be shown a bare `Role::Window` node with no children, and stayed shown it
/// until something unrelated caused a frame. Answering from the published
/// snapshot fixes the common case; the redraw request covers the rest, since
/// the adapter is active from here on and the next
/// [`PlatformWindow::update_accessibility`] reaches it.
/// `needs_full_tree` is what makes the delivery that follows a *full*
/// tree rather than a geometry patch: updates are otherwise throttled to
/// the moves-only rate, and a reader that attaches mid-session has never
/// seen the tree such a patch would be applied to.
struct TeksiloActivationHandler {
    needs_full_tree: Arc<AtomicBool>,
    bridge: Arc<AccessibilityBridge>,
    window: Arc<Window>,
}

/// The tree handed to a client that attached before this window ever drew.
///
/// A window node with no children — the same placeholder as before — because
/// there is genuinely nothing else to say yet. The accompanying redraw request
/// is what makes it short-lived.
fn empty_initial_tree() -> accesskit::TreeUpdate {
    let root = accesskit::Node::new(accesskit::Role::Window);
    let root_id = teksilo_core::accessibility::root_node_id();
    accesskit::TreeUpdate {
        nodes: vec![(root_id, root)],
        tree: Some(accesskit::TreeInfo::new(root_id)),
        tree_id: accesskit::TreeId::ROOT,
        focus: root_id,
    }
}

impl accesskit::ActivationHandler for TeksiloActivationHandler {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        self.needs_full_tree.store(true, Ordering::Relaxed);
        let update = self.bridge.on_activate();
        // Whether or not we could answer with a real tree, ask for a frame: it
        // is what carries the *next* update to the now-active adapter, and it
        // is also how the UI thread learns that a client attached.
        self.window.request_redraw();
        Some(update)
    }
}

/// Action handler — forwards action requests to the main thread via a channel,
/// then wakes the loop so the channel is actually drained.
struct TeksiloActionHandler {
    tx: mpsc::Sender<ActionRequest>,
    window: Arc<Window>,
}

impl accesskit::ActionHandler for TeksiloActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        let _ = self.tx.send(request);
        self.window.request_redraw();
    }
}

/// Deactivation handler — records that the last client detached.
///
/// Unlike activation, this *is* evidence about screen readers: when no client
/// is attached, none of them is reading the tree either.
struct TeksiloDeactivationHandler {
    bridge: Arc<AccessibilityBridge>,
    window: Arc<Window>,
}

impl accesskit::DeactivationHandler for TeksiloDeactivationHandler {
    fn deactivate_accessibility(&mut self) {
        self.bridge.on_deactivate();
        // The UI thread reads the flag once per frame, so it needs a frame.
        self.window.request_redraw();
    }
}

#[cfg(test)]
mod accessibility_bridge_tests {
    use super::{AccessibilityBridge, empty_initial_tree};

    /// A recognisable tree that is not the placeholder.
    fn published_tree() -> accesskit::TreeUpdate {
        let root_id = teksilo_core::accessibility::root_node_id();
        let child_id = accesskit::NodeId(4242);
        let mut root = accesskit::Node::new(accesskit::Role::Window);
        root.push_child(child_id);
        let mut child = accesskit::Node::new(accesskit::Role::Button);
        child.set_label("Save");
        accesskit::TreeUpdate {
            nodes: vec![(root_id, root), (child_id, child)],
            tree: Some(accesskit::TreeInfo::new(root_id)),
            tree_id: accesskit::TreeId::ROOT,
            focus: root_id,
        }
    }

    #[test]
    fn a_fresh_bridge_reports_no_client() {
        assert!(!AccessibilityBridge::default().is_active());
    }

    #[test]
    fn activation_before_the_first_frame_answers_with_the_placeholder() {
        let bridge = AccessibilityBridge::default();
        let update = bridge.on_activate();
        assert_eq!(update.nodes.len(), empty_initial_tree().nodes.len());
        assert_eq!(update.nodes[0].1.children().len(), 0);
        assert!(bridge.is_active());
    }

    #[test]
    fn activation_after_a_frame_answers_with_the_real_tree() {
        // The defect this pins: an assistive technology attaching to an idle
        // window was shown a childless window node and nothing scheduled a
        // frame to replace it.
        let bridge = AccessibilityBridge::default();
        bridge.publish(&published_tree());
        let update = bridge.on_activate();
        assert_eq!(
            update.nodes.len(),
            2,
            "the published tree, not a placeholder"
        );
        assert_eq!(update.nodes[0].1.children().len(), 1);
    }

    #[test]
    fn the_snapshot_is_the_latest_published_tree() {
        let bridge = AccessibilityBridge::default();
        bridge.publish(&empty_initial_tree());
        bridge.publish(&published_tree());
        assert_eq!(bridge.on_activate().nodes.len(), 2);
    }

    #[test]
    fn publishing_while_a_client_is_attached_is_skipped() {
        // Not a behaviour change anyone can observe through `on_activate` —
        // an attached client cannot ask for an initial tree — but it is what
        // keeps a per-frame `TreeUpdate` clone off the frame path while a
        // screen reader is running.
        let bridge = AccessibilityBridge::default();
        bridge.publish(&published_tree());
        let _ = bridge.on_activate();
        bridge.publish(&empty_initial_tree());
        bridge.on_deactivate();
        assert_eq!(
            bridge.on_activate().nodes.len(),
            2,
            "the tree published while attached must not have replaced the snapshot"
        );
    }

    #[test]
    fn deactivation_clears_the_attached_flag() {
        let bridge = AccessibilityBridge::default();
        let _ = bridge.on_activate();
        assert!(bridge.is_active());
        bridge.on_deactivate();
        assert!(!bridge.is_active());
        // And the tree it published is still there for a client that comes back.
        bridge.publish(&published_tree());
        assert_eq!(bridge.on_activate().nodes.len(), 2);
        assert!(bridge.is_active());
    }
}
