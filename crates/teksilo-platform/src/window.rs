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

/// Why a window could not configure its swapchain.
///
/// `Surface::configure` reports nothing: it hands its error to wgpu's
/// uncaptured-error handler, whose default is to panic. Every configure in
/// this file goes through [`configure_surface`] instead, which catches the
/// error and sorts it into one of these two.
///
/// Crate-internal: what a caller outside acts on is
/// [`FrameOutcome::DisplayLost`] and [`PlatformWindow::display_lost`], not the
/// classification behind them.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SurfaceConfigureError {
    /// The surface can no longer answer for its adapter. On every backend
    /// that means the connection to the display server is gone: the
    /// compositor exited or crashed, or the GPU was reset under it.
    ///
    /// wgpu's own wording for this is `Surface does not support the
    /// adapter's queue family`, which reads like a hardware mismatch and has
    /// been reported as one. It is not. The adapter was chosen with
    /// `compatible_surface`, and `request_adapter` filters on the very query
    /// that fails here (`wgpu_core::instance`), so it answered yes to the
    /// same question moments earlier. What changed is the surface, not the
    /// adapter.
    #[error(
        "the display server connection is gone: the window surface now reports no \
         supported formats for an adapter that was selected for it"
    )]
    DisplayLost,
    /// wgpu refused the configuration for some other reason, carrying its own
    /// message so a genuine mistake is not relabelled as a dead compositor.
    #[error("wgpu refused the surface configuration: {0}")]
    Rejected(String),
}

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
    /// The display server is gone, so this window can never present again.
    /// Caller should wind the application down. It must not reconfigure or
    /// ask for another redraw: both come straight back here, and that spin
    /// is the whole reason this is its own outcome rather than an `Error`.
    DisplayLost,
    /// Acquisition failed with a non-transient error.
    Error(SurfaceRenderError),
}

/// A platform window wrapping a winit window, wgpu surface, renderer,
/// and AccessKit adapter for screen reader support.
pub struct PlatformWindow {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    /// The adapter this surface was matched against. Kept so that a configure
    /// failure can ask the surface whether it still has formats for it, which
    /// is how [`classify_configure_failure`] tells a departed display server
    /// from a configuration wgpu genuinely refused.
    adapter: wgpu::Adapter,
    /// Latched the first time the display server is found to be gone. Every
    /// later configure and every frame is then skipped: with no compositor
    /// there is nothing to present to, and retrying only spins the loop.
    display_lost: bool,
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

/// The platform display connection the wgpu instance is built against.
///
/// Installed by the app layer via [`install_display_handle`] before the first
/// window exists, and read once by [`shared_instance`].
static DISPLAY_HANDLE: OnceLock<winit::event_loop::OwnedDisplayHandle> = OnceLock::new();

/// Hand wgpu the platform display connection, before any window is created.
///
/// Load-bearing for the OpenGL backend, which is the only backend a machine
/// with no Vulkan driver has left — an older GPU, or a VM whose guest driver
/// stops at GL. Without a display handle, wgpu-hal's GLES backend has no
/// windowing system to bind EGL to and falls back to
/// `EGL_MESA_platform_surfaceless`: a display that can render offscreen but can
/// never be compatible with a *window* surface. `request_adapter` then rejects
/// the only adapter on the machine with `incompatible_surface_backends: GL`,
/// and the process dies before its first window. Vulkan, Metal and D3D12 ignore
/// the handle entirely, so this costs those paths nothing.
///
/// Only the first call counts; later ones are ignored, because the instance is
/// built once per process and wgpu forbids presenting a surface from a display
/// other than the one the instance was created with.
pub fn install_display_handle(handle: winit::event_loop::OwnedDisplayHandle) {
    let _ = DISPLAY_HANDLE.set(handle);
}

/// The one wgpu instance for this process.
///
/// A surface has to come from the same instance that later enumerates adapters
/// for it, so this is the root every window hangs off. `Instance::new` is
/// synchronous, which is why this one can be a plain `OnceLock` while the
/// adapter and device below cannot.
///
/// The descriptor is built `_from_env`, so wgpu's own variables —
/// `WGPU_BACKEND`, `WGPU_GLES_MINOR_VERSION` and the rest — work here as they
/// do in every other wgpu application. That is the escape hatch for the machine
/// whose preferred backend has a broken driver, and it is worth having
/// precisely where the default choice is the thing under suspicion.
///
/// Its flags come from [`teksilo_render::instance_flags`] rather than from
/// `_from_env` alone, so this instance and the offscreen one
/// [`teksilo_render::test_support`] opens agree about them.
fn shared_instance() -> &'static wgpu::Instance {
    static INSTANCE: OnceLock<wgpu::Instance> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let mut descriptor = match DISPLAY_HANDLE.get() {
            Some(display) => wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
                display.clone(),
            )),
            // No app layer installed one — an embedder driving `PlatformWindow`
            // itself, or a test. Offscreen work is unaffected; only a GL-backed
            // window needs the handle.
            None => wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
        };
        descriptor.flags = teksilo_render::instance_flags();
        wgpu::Instance::new(descriptor)
    })
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
/// The limits a live window asks its device for.
///
/// Deliberately **not** [`wgpu::Limits::default`]. That set demands eight
/// colour attachments, 64 KiB uniform bindings and 8192-pixel textures. This
/// renderer draws every pass into a *single* colour attachment, binds at most
/// 8 KiB of uniforms (128 animation slots of 64 bytes) and caps its path atlas
/// at 4096 pixels. The headroom was inherited from the default, never needed.
///
/// On GLES-3.1 class hardware that headroom is not merely unused, it is
/// refused: a Raspberry Pi 4's V3D driver allows four colour attachments, so
/// `default()` failed device creation outright and the app could not open a
/// window at all.
///
/// `downlevel_defaults` is wgpu's GLES-3.1 floor, which is exactly that class
/// of hardware, and it is already what [`teksilo_render::test_support`] opens
/// its offscreen device with, so a frame that renders in a test now renders in
/// a window too. `using_resolution` lifts the three texture-dimension limits
/// back to whatever this adapter really supports, because the path atlas grows
/// past the 2048-pixel downlevel cap.
fn window_device_limits(adapter_limits: wgpu::Limits) -> wgpu::Limits {
    wgpu::Limits::downlevel_defaults().using_resolution(adapter_limits)
}

/// Open a device on `adapter`, preferring [`window_device_limits`] and falling
/// back to whatever the adapter itself reports.
///
/// The fallback is not redundant. `downlevel_defaults` is a floor for a *class*
/// of hardware, not a promise about any given adapter. Anything below GLES 3.1
/// (an old GL driver, a constrained software rasterizer) can sit under it on a
/// field `using_resolution` does not lift, and then the principled ask fails
/// for the same reason `default()` did on the Pi. `adapter.limits()` is by
/// construction the most that adapter can give, so it cannot be refused on
/// limit grounds; a request that still fails has a real problem rather than a
/// mis-sized ask, and that is the error worth propagating.
async fn open_device(
    adapter: &wgpu::Adapter,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    let descriptor = |limits| wgpu::DeviceDescriptor {
        label: Some("teksilo_device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        ..Default::default()
    };

    match adapter
        .request_device(&descriptor(window_device_limits(adapter.limits())))
        .await
    {
        Ok(pair) => Ok(pair),
        Err(err) => {
            // Say why we dropped to the adapter's own limits: a silent
            // fallback turns "this GPU is below the GLES-3.1 floor" into an
            // unexplained difference in behaviour between two machines.
            eprintln!(
                "teksilo-platform: downlevel device limits refused ({err}); \
                 retrying with the adapter's own limits"
            );
            adapter.request_device(&descriptor(adapter.limits())).await
        }
    }
}

/// The backends this platform prefers, most preferred first.
///
/// wgpu does not rank backends. `Instance::new` initialises them in a fixed
/// order — Vulkan, Metal, D3D12, GLES — and `request_adapter` then sorts the
/// adapters it collected **only** by device type, and only when a power
/// preference is set. Ours is `PowerPreference::None` unless `WGPU_POWER_PREF`
/// says otherwise, which is wgpu's own default and sorts nothing at all. So the
/// winner has been "the first adapter the first initialised backend
/// enumerated", which on Windows means D3D12 was never reached as long as any
/// Vulkan ICD was installed, however old.
///
/// That is the wrong default there. D3D12 is the backend Windows GPU drivers
/// are tested against hardest — it is what the browsers use on Windows — while
/// Vulkan support on older Windows hardware ranges from good to a stub that
/// enumerates an adapter it cannot really drive. The field report that prompted
/// this is one of those: a Windows 10 machine whose Vulkan ICD cannot build
/// wgpu's own indirect-validation pipelines (see
/// [`teksilo_render::instance_flags`]) and, past that, cannot present at all,
/// while D3D12 on the same machine works.
///
/// Elsewhere the order simply writes down what wgpu already did, so this is a
/// change of behaviour on Windows only. An explicit `WGPU_BACKEND` still wins:
/// it is applied at `Instance::new`, so the backends it excludes enumerate
/// nothing here and this order silently narrows to the one that was asked for.
fn preferred_backends() -> &'static [wgpu::Backends] {
    #[cfg(target_os = "windows")]
    {
        &[
            wgpu::Backends::DX12,
            wgpu::Backends::VULKAN,
            wgpu::Backends::GL,
        ]
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        &[
            wgpu::Backends::METAL,
            wgpu::Backends::VULKAN,
            wgpu::Backends::GL,
        ]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "ios")))]
    {
        &[wgpu::Backends::VULKAN, wgpu::Backends::GL]
    }
}

/// The adapters on `backends` that can present to `surface`, ranked as
/// `request_adapter` would rank them.
///
/// `enumerate_adapters` asks each backend for its adapters with no surface, so
/// the "can this one actually present to this window" filter that
/// `request_adapter` applies internally has to be applied here instead. A
/// non-empty format list is wgpu's own answer to that question, and it is the
/// same one [`shared_gpu_for`] uses to revalidate the cached adapter.
///
/// The ordering within a backend deliberately mirrors
/// `wgpu_core::instance::request_adapter`: rank by device type under a power
/// preference, and leave enumeration order alone under `None`. Keeping the two
/// identical means this pass changes *which backend* is tried first and nothing
/// else about how an adapter is chosen.
async fn presentable_adapters(
    surface: &wgpu::Surface<'static>,
    backends: wgpu::Backends,
    power_preference: wgpu::PowerPreference,
) -> Vec<wgpu::Adapter> {
    let mut adapters: Vec<wgpu::Adapter> = shared_instance()
        .enumerate_adapters(backends)
        .await
        .into_iter()
        .filter(|adapter| !surface.get_capabilities(adapter).formats.is_empty())
        .collect();

    let prefer_integrated = match power_preference {
        wgpu::PowerPreference::LowPower => true,
        wgpu::PowerPreference::HighPerformance => false,
        // wgpu does not sort at all here, so neither do we.
        _ => return adapters,
    };
    adapters
        .sort_by_key(|adapter| device_type_rank(adapter.get_info().device_type, prefer_integrated));
    adapters
}

/// `wgpu_core::instance::request_adapter`'s `get_order`, kept in step with it.
///
/// "Other" outranks the virtual and CPU types because a backend that does not
/// report device types at all (OpenGL) lands there, and it is likelier to be
/// real hardware than a software rasterizer is.
fn device_type_rank(device_type: wgpu::DeviceType, prefer_integrated: bool) -> u8 {
    match device_type {
        wgpu::DeviceType::DiscreteGpu if prefer_integrated => 2,
        wgpu::DeviceType::IntegratedGpu if prefer_integrated => 1,
        wgpu::DeviceType::DiscreteGpu => 1,
        wgpu::DeviceType::IntegratedGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::VirtualGpu => 4,
        wgpu::DeviceType::Cpu => 5,
    }
}

/// Find an adapter that can present to `surface` *and* yields a device.
///
/// Adapter selection is a search, not a single request — the same lesson
/// [`teksilo_render::test_support`] already encodes for its offscreen device,
/// which the window path did not have. A host can enumerate an adapter it
/// cannot actually open (a VM's GL driver is the usual one) while a perfectly
/// good software adapter sits behind `force_fallback_adapter`. Treating the
/// first failure as fatal reports "no GPU" on a machine that has one.
///
/// Every pass filters on surface compatibility, so an adapter that cannot
/// present to this window is never chosen — that is the check that failed on a
/// machine with no Vulkan driver, and it is load-bearing, not a formality. The
/// ordered pass tests it with [`presentable_adapters`], the fallback pass with
/// `request_adapter`'s own `compatible_surface`; both ask wgpu the same
/// question.
///
/// Panics only when *every* adapter on the machine declines, with a message
/// naming what was tried and what the user can do about it.
async fn open_gpu_for(
    surface: &wgpu::Surface<'static>,
) -> (wgpu::Adapter, wgpu::Device, wgpu::Queue) {
    // `WGPU_POWER_PREF` is wgpu's own knob; honour it for the same reason the
    // instance is built `_from_env`.
    let power_preference = wgpu::PowerPreference::from_env().unwrap_or_default();
    let mut adapter_error = None;
    let mut device_error = None;

    // First pass: this platform's own backend order (see
    // `preferred_backends`). `request_adapter` cannot express "try D3D12
    // before Vulkan" — its options carry no backend field — so the ordering
    // has to be done by enumerating one backend at a time.
    for &backends in preferred_backends() {
        for adapter in presentable_adapters(surface, backends, power_preference).await {
            match open_device(&adapter).await {
                Ok((device, queue)) => return (adapter, device, queue),
                Err(err) => {
                    eprintln!(
                        "teksilo-platform: adapter {:?} could not open a device ({err}); \
                         trying the next one",
                        adapter.get_info().name
                    );
                    device_error.get_or_insert(err);
                }
            }
        }
    }

    // Second pass: whatever wgpu itself would have picked, then an explicit
    // software adapter. The first arm is not redundant with the loop above —
    // it reaches any backend `preferred_backends` does not name — and the
    // second is the only way to ask for a CPU adapter, which
    // `enumerate_adapters` cannot express.
    for force_fallback_adapter in [false, true] {
        let adapter = match shared_instance()
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference,
                compatible_surface: Some(surface),
                force_fallback_adapter,
                ..Default::default()
            })
            .await
        {
            Ok(adapter) => adapter,
            Err(err) => {
                adapter_error.get_or_insert(err);
                continue;
            }
        };

        match open_device(&adapter).await {
            Ok((device, queue)) => return (adapter, device, queue),
            Err(err) => {
                // Worth saying out loud: the next pass silently landing on a
                // software adapter is a large performance difference, and an
                // unexplained one is the sort of thing that gets reported as
                // "Teksilo is slow on my machine".
                eprintln!(
                    "teksilo-platform: adapter {:?} could not open a device ({err}); \
                     trying the next one",
                    adapter.get_info().name
                );
                device_error.get_or_insert(err);
            }
        }
    }

    panic!(
        "no usable GPU adapter for this window.\n\
         Tried every backend wgpu was built with, then an explicit software \
         fallback; none could both present to the window and open a device.\n\
         adapter search: {adapter_error:?}\n\
         device open:    {device_error:?}\n\
         Teksilo needs Vulkan, Metal, D3D12 or OpenGL (3.3 desktop / ES 3.0). \
         On Linux, installing a Vulkan driver is usually the fix: \
         `mesa-vulkan-drivers` carries both the hardware drivers and the \
         software `lavapipe`. `WGPU_BACKEND=gl|vulkan|dx12|metal` forces a \
         specific backend."
    );
}

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

    let (adapter, device, queue) = open_gpu_for(surface).await;

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

/// Configure `surface`, handing back the failure instead of letting wgpu's
/// default uncaptured-error handler panic.
///
/// Every `Surface::configure` in this file goes through here, and the error
/// scope is what makes that sufficient. Asking the surface whether it is still
/// alive and *then* configuring it leaves a gap between the two in which the
/// compositor can exit, and that gap is the bug: `request_adapter` validated
/// this adapter against this surface with the same query `configure` runs, so
/// only a change in between can make the second one fail. A scope catches the
/// error from this exact call, however late the display server goes away.
///
/// `pop` resolves immediately on native (wgpu answers with a ready future) and
/// wgpu's error scopes are thread-local. Both hold because every window in this
/// process is created and drawn on the winit main thread.
fn configure_surface(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> Result<(), SurfaceConfigureError> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(device, config);
    match pollster::block_on(scope.pop()) {
        None => Ok(()),
        Some(err) => Err(classify_configure_failure(
            err.to_string(),
            !surface.get_capabilities(adapter).formats.is_empty(),
        )),
    }
}

/// Sort a configure failure into "the display server left" and everything
/// else, on the one signal that separates them.
///
/// A live surface offers its adapter a non-empty format list; a surface whose
/// display server has gone offers none, and that transition is observable:
/// seven formats before the compositor exits, zero after. Deliberately not a
/// match on wgpu's message text, which is both misleading here and free to
/// change between releases.
fn classify_configure_failure(
    description: String,
    surface_has_formats: bool,
) -> SurfaceConfigureError {
    if surface_has_formats {
        SurfaceConfigureError::Rejected(description)
    } else {
        SurfaceConfigureError::DisplayLost
    }
}

/// What [`PlatformWindow::surface_and_renderer`] hands to both constructors.
struct WindowGpu {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    adapter: wgpu::Adapter,
    display_lost: bool,
}

impl PlatformWindow {
    /// Everything both constructors do: surface, shared device, swapchain
    /// configuration, renderer. Kept in one place because the two entry points
    /// differ only in whether they attach an AccessKit adapter, and sixty
    /// duplicated lines of GPU setup is exactly the sort of thing that drifts.
    async fn surface_and_renderer(window: &Arc<Window>) -> WindowGpu {
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
        // A compositor that goes away while the device above is being opened
        // lands here, because opening one is the slowest step between the
        // adapter's validation and this call. It used to panic out of wgpu's
        // default error handler before the window ever existed.
        let display_lost =
            match configure_surface(&surface, &gpu.adapter, &gpu.device, &surface_config) {
                Ok(()) => false,
                Err(err) => {
                    eprintln!("teksilo-platform: {err}");
                    matches!(err, SurfaceConfigureError::DisplayLost)
                }
            };

        // The renderer stays per-window: it owns the glyph atlas, the path
        // atlas and the blur pool, and it is `!Sync` besides.
        let renderer = Renderer::new(gpu.device, gpu.queue, surface_format);
        WindowGpu {
            surface,
            surface_config,
            renderer,
            adapter: gpu.adapter,
            display_lost,
        }
    }

    /// Create a new platform window from a winit window.
    /// The `event_loop` parameter is needed for the AccessKit adapter.
    pub async fn new_with_a11y(
        window: Window,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Self {
        let window = Arc::new(window);
        let scale_factor = window.scale_factor();
        let WindowGpu {
            surface,
            surface_config,
            renderer,
            adapter,
            display_lost,
        } = Self::surface_and_renderer(&window).await;

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
            adapter,
            display_lost,
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
        let WindowGpu {
            surface,
            surface_config,
            renderer,
            adapter,
            display_lost,
        } = Self::surface_and_renderer(&window).await;
        let (_action_tx, action_rx) = mpsc::channel();

        Self {
            window,
            surface,
            surface_config,
            adapter,
            display_lost,
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
    ///
    /// A resize that arrives once the display server has gone is dropped:
    /// there is nothing left to present to.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if self.display_lost || new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.apply_surface_config();
    }

    /// Get current surface dimensions.
    pub fn surface_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }

    /// Reconfigure the surface with the current config.
    /// Use after a Lost or Outdated surface error.
    ///
    /// Answers whether this window can still present. `false` means the
    /// display server is gone, and the caller should wind down rather than ask
    /// for another frame: the next one would come back here unchanged.
    pub fn reconfigure_surface(&mut self) -> bool {
        if self.display_lost {
            return false;
        }
        self.apply_surface_config();
        !self.display_lost
    }

    /// Whether the display server has gone away under this window.
    pub fn display_lost(&self) -> bool {
        self.display_lost
    }

    /// Push `surface_config` to the surface, latching a departed display
    /// server and reporting anything else wgpu refused.
    ///
    /// The latch is what keeps the report to one line: every later configure
    /// returns before reaching here.
    fn apply_surface_config(&mut self) {
        if let Err(err) = configure_surface(
            &self.surface,
            &self.adapter,
            self.renderer.device(),
            &self.surface_config,
        ) {
            eprintln!("teksilo-platform: {err}");
            if matches!(err, SurfaceConfigureError::DisplayLost) {
                self.display_lost = true;
            }
        }
    }

    /// Render a frame to the surface.
    pub fn render_frame(
        &mut self,
        frame: &teksilo_canvas::RenderFrame,
        clear_color: [f32; 4],
    ) -> FrameOutcome {
        if self.display_lost {
            return FrameOutcome::DisplayLost;
        }
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
    ///
    /// Hand it what
    /// [`WidgetTree::deliver_accessibility`](teksilo_core::WidgetTree::deliver_accessibility)
    /// returns, not `sync_accessibility`'s update: a node that left the tree a
    /// reader sees and came back must reach the adapter under an id it has
    /// never had, or a reader on AT-SPI holds it defunct and hears nothing
    /// from it.
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
    ///
    /// They name nodes by the ids the adapter was handed; pass each through
    /// [`WidgetTree::resolve_adapter_action`](teksilo_core::WidgetTree::resolve_adapter_action)
    /// before looking its target up in the tree.
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
mod surface_configure_tests {
    use super::{SurfaceConfigureError, classify_configure_failure};

    /// The defect this pins: a compositor crash reached the user as
    /// `Surface does not support the adapter's queue family`, and was read as
    /// a GPU mismatch by everyone who saw it, including the maintainer. A
    /// surface with no formats left for the adapter it was matched against has
    /// lost its display server, whatever wgpu chooses to call it.
    #[test]
    fn a_surface_with_no_formats_left_means_the_display_server_is_gone() {
        let err = classify_configure_failure(
            "Surface does not support the adapter's queue family".to_string(),
            false,
        );
        assert!(matches!(err, SurfaceConfigureError::DisplayLost));
        assert!(err.to_string().contains("display server"));
    }

    /// The other half, and the reason this is not simply "any configure
    /// failure means the compositor left": a surface that still answers with
    /// formats has a real configuration problem, and its own message has to
    /// survive rather than be relabelled as a dead compositor.
    #[test]
    fn a_surface_that_still_has_formats_keeps_wgpus_own_message() {
        let err = classify_configure_failure(
            "Requested format Rgba8Unorm is not in the list of supported formats".to_string(),
            true,
        );
        assert!(matches!(err, SurfaceConfigureError::Rejected(_)));
        assert!(err.to_string().contains("Rgba8Unorm"));
        assert!(!err.to_string().contains("display server"));
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

#[cfg(test)]
mod device_limits_tests {
    use super::*;

    /// A Raspberry Pi 4's V3D driver in the fields that matter here: four
    /// colour attachments and 4096-pixel textures. This is the adapter the
    /// crash report came from.
    fn pi4_class_limits() -> wgpu::Limits {
        wgpu::Limits {
            max_texture_dimension_1d: 4096,
            max_texture_dimension_2d: 4096,
            max_texture_dimension_3d: 256,
            max_color_attachments: 4,
            ..wgpu::Limits::downlevel_defaults()
        }
    }

    #[test]
    fn the_default_limits_are_refused_by_gles_class_hardware() {
        // The bug, stated as a test: this is what the window used to ask for,
        // and `check_limits` is the same comparison wgpu makes inside
        // `request_device`. If this ever starts passing, wgpu changed its
        // defaults and the fallback below is what keeps us honest.
        assert!(
            !wgpu::Limits::default().check_limits(&pi4_class_limits()),
            "the wgpu default limits are supposed to over-ask for a Pi-4 class \
             adapter; that refusal is the crash this module exists to prevent"
        );
    }

    #[test]
    fn the_window_ask_is_satisfiable_on_gles_class_hardware() {
        let adapter = pi4_class_limits();
        assert!(
            window_device_limits(adapter.clone()).check_limits(&adapter),
            "a Pi-4 class adapter must be able to grant what a window asks for"
        );
    }

    #[test]
    fn the_window_never_asks_past_the_downlevel_floor() {
        // The regression pin: whatever the adapter offers, every limit that is
        // not a texture dimension stays at the GLES-3.1 floor. Re-introducing
        // `Limits::default()` fails here on a developer's desktop rather than
        // only on a reviewer's Raspberry Pi.
        let generous = wgpu::Limits::default();
        let asked = window_device_limits(generous.clone());
        let floor = wgpu::Limits::downlevel_defaults();

        assert_eq!(asked.max_color_attachments, floor.max_color_attachments);
        assert_eq!(
            asked.max_uniform_buffer_binding_size,
            floor.max_uniform_buffer_binding_size
        );
        assert_eq!(
            asked.max_inter_stage_shader_variables,
            floor.max_inter_stage_shader_variables
        );
        assert_eq!(
            asked.max_storage_buffers_per_shader_stage,
            floor.max_storage_buffers_per_shader_stage
        );
        assert_ne!(
            asked, generous,
            "asking for the full default set is exactly the regression"
        );
    }

    #[test]
    fn texture_dimensions_follow_the_adapter() {
        // `downlevel_defaults` caps 2D textures at 2048 and the path atlas
        // grows to 4096, so the resolution limits, and only those, are lifted
        // to whatever the adapter really offers.
        const PATH_ATLAS_MAX: u32 = 4096;

        for adapter in [pi4_class_limits(), wgpu::Limits::default()] {
            let asked = window_device_limits(adapter.clone());
            assert_eq!(
                asked.max_texture_dimension_1d,
                adapter.max_texture_dimension_1d
            );
            assert_eq!(
                asked.max_texture_dimension_2d,
                adapter.max_texture_dimension_2d
            );
            assert_eq!(
                asked.max_texture_dimension_3d,
                adapter.max_texture_dimension_3d
            );
            assert!(
                asked.max_texture_dimension_2d >= PATH_ATLAS_MAX,
                "the path atlas grows to {PATH_ATLAS_MAX}; a device that cannot \
                 hold it would fail on a path-heavy frame instead of at startup"
            );
        }
    }

    #[test]
    fn the_floor_still_covers_what_the_renderer_binds() {
        // What the renderer actually needs, so that lowering the ask further
        // fails here rather than in a frame. 128 animation slots of 64 bytes
        // is the largest uniform binding; every render pass has exactly one
        // colour attachment.
        const ANIM_UNIFORM_BYTES: u64 = 128 * 64;
        let asked = window_device_limits(pi4_class_limits());

        assert!(asked.max_color_attachments >= 1);
        assert!(asked.max_uniform_buffer_binding_size >= ANIM_UNIFORM_BYTES);
    }
}
