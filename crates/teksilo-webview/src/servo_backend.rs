// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ServoBackend`: the pure-Rust engine backend, through the [`servo`] crate,
//! behind the `servo-backend` feature.
//!
//! Servo renders each page offscreen, into a GL context of this backend's own
//! ([`gl_context`]), and its frames reach the window through the picture the
//! `WebView` widget draws: a [`WebViewRendering::Surface`] backend. Nothing of
//! it touches the window wgpu presents to, so it works under Wayland, where wry
//! cannot embed, and the page clips, sits under overlays and scrolls with its
//! container like any other content.
//!
//! # The loop
//!
//! Servo works on threads of its own and asks to be run on the UI thread
//! through an [`EventLoopWaker`]. The waker posts one [`WebViewPumpPayload`]
//! at a time; `teksilo-app` answers it with `WebViewRegistry::pump`, which runs
//! [`Servo::spin_event_loop`] and then, for each web view Servo reported a new
//! frame for, paints it and starts reading it back. A read is handed on the
//! next time the engine runs (the frame clock wakes it a few milliseconds
//! later), through a `LiveImageDiffWriter`, so a frame identical to the last
//! one commits nothing and wakes no window. A web view that is dormant or
//! scrolled out of view is hidden and throttled in Servo, and paints nothing.
//!
//! # Events
//!
//! Load start and finish, navigations, URL changes, the title, console
//! messages and the page taking or giving up the keyboard reach the widget as
//! [`WebViewEvent`]s.
//!
//! # What is not here yet
//!
//! Input does not reach the page, and JS→Rust messages (`window.ipc`) are not
//! delivered.
//!
//! [`WebViewPumpPayload`]: crate::WebViewPumpPayload

mod frame_clock;
mod gl_context;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use teksilo_canvas::live_image::{LiveImageDiffWriter, LiveImageWriter};
use teksilo_canvas::{ImageOrientation, Rect};
use teksilo_core::AppEventPoster;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::window::TeksiloWindowId;

use dpi::PhysicalSize;
use euclid::Scale;
use servo::{
    ConsoleLogLevel, EventLoopWaker, LoadStatus, NavigationRequest, RenderingContext, Servo,
    ServoBuilder, WebView as ServoWebView, WebViewBuilder, WebViewDelegate,
};
use url::Url;

use crate::backend::{
    ConsoleLevel, NoopWebViewHandle, WebSource, WebViewAttributes, WebViewBackend, WebViewEvent,
    WebViewHandle, WebViewId, WebViewRendering, js_string, post_event, post_pump,
};

use frame_clock::FrameClock;
use gl_context::{AdapterKind, HeadlessGlContext};

/// How long a read may wait on the GPU when the engine runs: past it, the
/// clock looks again a little later instead of holding the UI thread.
const READ_WAIT: Duration = Duration::from_millis(2);

/// When to look again for a read still on the GPU.
const READ_RETRY: Duration = Duration::from_millis(3);

/// The pure-Rust engine backend. Construct and hand to
/// `install_web_view(ServoBackend::new())`.
///
/// Every web view this backend opens shares one Servo engine, built when the
/// first one opens. Servo allows one engine per process, ever: its options are
/// a process-wide once-cell that a dropped engine does not reset. So a second
/// `ServoBackend` in the same process (a second application builder, say)
/// cannot get an engine of its own, and its web views show the error chrome
/// with a console message saying why.
#[derive(Default)]
pub struct ServoBackend {
    engine: Option<Rc<Engine>>,
}

impl std::fmt::Debug for ServoBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServoBackend")
            .field("engine_built", &self.engine.is_some())
            .finish()
    }
}

impl ServoBackend {
    /// Construct the Servo backend.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Set once this process has built a Servo engine. Servo panics on a second
/// build (`opts::initialize_options` sets a once-cell), so this is what turns
/// that panic into an error a web view can report.
static ENGINE_BUILT: AtomicBool = AtomicBool::new(false);

/// The process already built its one engine, for another backend.
#[derive(Debug, PartialEq, Eq)]
struct EngineTaken;

/// The engine `slot` holds, built with `build` if this process has none yet.
///
/// Generic so the rule can be tested without starting Servo.
fn shared_engine<T: Clone>(
    slot: &mut Option<T>,
    built: &AtomicBool,
    build: impl FnOnce() -> T,
) -> Result<T, EngineTaken> {
    if let Some(engine) = slot {
        return Ok(engine.clone());
    }
    if built.swap(true, Ordering::AcqRel) {
        return Err(EngineTaken);
    }
    Ok(slot.insert(build()).clone())
}

/// Report engine-init failure: drive the widget to its Error chrome via
/// `NavigationFinished { success: false }`, plus a console message. Returns a
/// no-op handle. Mirrors `wry_backend::fail`.
fn fail(
    poster: &Option<Arc<dyn AppEventPoster>>,
    window_id: TeksiloWindowId,
    web_view_id: WebViewId,
    text: String,
) -> Box<dyn WebViewHandle> {
    post_event(
        poster,
        window_id,
        web_view_id,
        WebViewEvent::ConsoleMessage {
            level: ConsoleLevel::Error,
            text,
        },
    );
    post_event(
        poster,
        window_id,
        web_view_id,
        WebViewEvent::NavigationFinished {
            url: String::new(),
            success: false,
        },
    );
    Box::new(NoopWebViewHandle)
}

/// Asks the UI thread to run the engine: posts one pump request at a time.
/// Called by Servo from its own threads, and by the frame clock.
#[derive(Clone)]
struct ServoWaker {
    inner: Arc<WakerState>,
}

struct WakerState {
    /// A pump request is posted and not yet answered.
    posted: AtomicBool,
    poster: Option<Arc<dyn AppEventPoster>>,
}

impl ServoWaker {
    fn new(poster: Option<Arc<dyn AppEventPoster>>) -> Self {
        Self {
            inner: Arc::new(WakerState {
                posted: AtomicBool::new(false),
                poster,
            }),
        }
    }

    /// The request is being answered: a wake from now on posts another.
    fn answered(&self) {
        self.inner.posted.store(false, Ordering::Release);
    }
}

impl EventLoopWaker for ServoWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        if !self.inner.posted.swap(true, Ordering::AcqRel) {
            post_pump(&self.inner.poster);
        }
    }
}

/// The one Servo of the process, and what runs it.
struct Engine {
    servo: Servo,
    waker: ServoWaker,
    clock: FrameClock,
    /// Every open web view; dead entries are dropped as the engine runs.
    views: RefCell<Vec<Weak<View>>>,
}

impl Engine {
    fn start(poster: Option<Arc<dyn AppEventPoster>>) -> Rc<Self> {
        let waker = ServoWaker::new(poster);
        let clock_waker = waker.clone();
        let clock = FrameClock::start(move || clock_waker.wake());
        let servo = ServoBuilder::default()
            .event_loop_waker(Box::new(waker.clone()))
            .build();
        Rc::new(Self {
            servo,
            waker,
            clock,
            views: RefCell::new(Vec::new()),
        })
    }

    /// Run Servo once, then paint and read back what changed.
    fn pump(&self) {
        self.waker.answered();
        self.servo.spin_event_loop();
        let views: Vec<Rc<View>> = {
            let mut views = self.views.borrow_mut();
            views.retain(|view| view.strong_count() > 0);
            views.iter().filter_map(Weak::upgrade).collect()
        };
        let now = Instant::now();
        for view in views {
            view.present(&self.clock, now);
        }
    }
}

/// One open web view: its rendering context, its Servo `WebView`, and the
/// picture its frames go to.
struct View {
    gl: Rc<HeadlessGlContext>,
    webview: RefCell<Option<ServoWebView>>,
    picture: RefCell<LiveImageDiffWriter>,
    /// Servo reported a frame not painted yet.
    frame_ready: Cell<bool>,
    /// The widget shows the page: not dormant, and some of it in view.
    shown: Cell<bool>,
    /// The context's size, in device pixels.
    size: Cell<PhysicalSize<u32>>,
    scale: Cell<f32>,
}

impl View {
    /// Hand on a finished read, then paint a reported frame if there is one
    /// and the page is shown.
    fn present(&self, clock: &FrameClock, now: Instant) {
        if self.gl.reads_pending() {
            let mut picture = self.picture.borrow_mut();
            self.gl.finish_reads(READ_WAIT, &mut |rows, width, height| {
                // A refused frame (a size past LiveImage's limits) leaves the
                // previous one shown; the next resize brings it back in range.
                let _ = picture.write_frame(width, height, rows, width as usize * 4);
            });
            if self.gl.reads_pending() {
                clock.wake_at(now + READ_RETRY);
            }
        }

        // The first frame at a new size still shows the page's previous
        // layout, its newly exposed part the page background; the relaid page
        // follows one frame later. Committed as it comes, as a browser shows
        // it.
        if !self.frame_ready.get() || !self.shown.get() {
            return;
        }
        self.frame_ready.set(false);
        let Some(webview) = self.webview.borrow().clone() else {
            return;
        };
        webview.paint();
        self.gl.begin_read();
        clock.wake_at(now + READ_RETRY);
    }
}

/// Translates Servo's delegate callbacks into [`WebViewEvent`]s, and marks
/// its view when a frame is ready.
struct ServoEventDelegate {
    view: Weak<View>,
    poster: Option<Arc<dyn AppEventPoster>>,
    window_id: TeksiloWindowId,
    web_view_id: WebViewId,
}

impl ServoEventDelegate {
    fn post(&self, event: WebViewEvent) {
        post_event(&self.poster, self.window_id, self.web_view_id, event);
    }
}

impl WebViewDelegate for ServoEventDelegate {
    fn notify_new_frame_ready(&self, _webview: ServoWebView) {
        // Painted by `Engine::pump` once `spin_event_loop` returns, so every
        // view's frame of this spin is painted in one place.
        if let Some(view) = self.view.upgrade() {
            view.frame_ready.set(true);
        }
    }

    fn notify_load_status_changed(&self, webview: ServoWebView, status: LoadStatus) {
        match status {
            LoadStatus::Started => self.post(WebViewEvent::PageLoadStarted),
            LoadStatus::HeadParsed => {}
            LoadStatus::Complete => {
                self.post(WebViewEvent::PageLoadFinished);
                self.post(WebViewEvent::NavigationFinished {
                    url: webview.url().map(|u| u.to_string()).unwrap_or_default(),
                    success: true,
                });
            }
        }
    }

    fn notify_page_title_changed(&self, _webview: ServoWebView, title: Option<String>) {
        self.post(WebViewEvent::TitleChanged(title.unwrap_or_default()));
    }

    fn request_navigation(&self, _webview: ServoWebView, request: NavigationRequest) {
        self.post(WebViewEvent::NavigationStarted {
            url: request.url.to_string(),
            can_cancel: false,
        });
        // Dropping the request allows the navigation: the widget observes
        // navigations, it does not veto them.
        drop(request);
    }

    /// A URL change with no load behind it (`history.pushState`, a fragment)
    /// finishes a navigation as far as the widget's URL binding is concerned.
    fn notify_url_changed(&self, _webview: ServoWebView, url: Url) {
        self.post(WebViewEvent::NavigationFinished {
            url: url.to_string(),
            success: true,
        });
    }

    fn notify_focus_changed(&self, _webview: ServoWebView, focused: bool) {
        self.post(WebViewEvent::EngineFocusChanged(focused));
    }

    fn show_console_message(&self, _webview: ServoWebView, level: ConsoleLogLevel, text: String) {
        self.post(WebViewEvent::ConsoleMessage {
            level: console_level(level),
            text,
        });
    }
}

/// The widget's three console levels, from Servo's seven.
fn console_level(level: ConsoleLogLevel) -> ConsoleLevel {
    match level {
        ConsoleLogLevel::Warn => ConsoleLevel::Warn,
        ConsoleLogLevel::Error => ConsoleLevel::Error,
        ConsoleLogLevel::Log
        | ConsoleLogLevel::Debug
        | ConsoleLogLevel::Info
        | ConsoleLogLevel::Trace
        | ConsoleLogLevel::Dir => ConsoleLevel::Log,
    }
}

impl WebViewBackend for ServoBackend {
    fn rendering(&self) -> WebViewRendering {
        // A GL readback stores the bottom row first.
        WebViewRendering::Surface {
            orientation: ImageOrientation::FlipVertical,
        }
    }

    fn open(
        &mut self,
        web_view_id: WebViewId,
        window_id: TeksiloWindowId,
        _parent: Option<ParentHandle>,
        attrs: WebViewAttributes,
        poster: Option<Arc<dyn AppEventPoster>>,
        surface: Option<LiveImageWriter>,
    ) -> Box<dyn WebViewHandle> {
        let Some(surface) = surface else {
            return fail(
                &poster,
                window_id,
                web_view_id,
                "ServoBackend: opened without a picture to render into".to_string(),
            );
        };
        let engine_poster = poster.clone();
        let engine = match shared_engine(&mut self.engine, &ENGINE_BUILT, || {
            Engine::start(engine_poster)
        }) {
            Ok(engine) => engine,
            Err(EngineTaken) => {
                return fail(
                    &poster,
                    window_id,
                    web_view_id,
                    "ServoBackend: this process already built its one Servo engine for another \
                     backend, and Servo allows only one; install a single ServoBackend"
                        .to_string(),
                );
            }
        };

        // A real size arrives with the widget's first `set_bounds`.
        let size = PhysicalSize::new(1, 1);
        let gl = match HeadlessGlContext::new(size, Rc::new(engine.clock.driver())) {
            Ok(gl) => Rc::new(gl),
            Err(error) => {
                return fail(
                    &poster,
                    window_id,
                    web_view_id,
                    format!("ServoBackend: no OpenGL context to render into: {error:?}"),
                );
            }
        };
        if gl.adapter() == AdapterKind::Software {
            post_event(
                &poster,
                window_id,
                web_view_id,
                WebViewEvent::ConsoleMessage {
                    level: ConsoleLevel::Warn,
                    text: format!(
                        "ServoBackend: no hardware OpenGL adapter, rendering on {}",
                        gl.renderer()
                    ),
                },
            );
        }

        let view = Rc::new(View {
            gl: gl.clone(),
            webview: RefCell::new(None),
            picture: RefCell::new(LiveImageDiffWriter::new(surface)),
            frame_ready: Cell::new(false),
            shown: Cell::new(true),
            size: Cell::new(size),
            scale: Cell::new(1.0),
        });
        let delegate: Rc<dyn WebViewDelegate> = Rc::new(ServoEventDelegate {
            view: Rc::downgrade(&view),
            poster: poster.clone(),
            window_id,
            web_view_id,
        });
        let mut builder = WebViewBuilder::new(&engine.servo, gl as Rc<dyn RenderingContext>)
            .delegate(delegate)
            .hidpi_scale_factor(Scale::new(1.0));
        if let Some(url) = attrs.source.as_ref().and_then(source_url) {
            builder = builder.url(url);
        }
        *view.webview.borrow_mut() = Some(builder.build());
        engine.views.borrow_mut().push(Rc::downgrade(&view));
        // The page starts loading on Servo's threads, which wake the engine
        // when there is something to run.
        Box::new(ServoHandle { engine, view })
    }

    fn pump(&mut self) {
        if let Some(engine) = &self.engine {
            engine.pump();
        }
    }
}

/// The URL that shows `source`: inline HTML travels as a `data:` URL, as
/// Servo has no call to load a string.
fn source_url(source: &WebSource) -> Option<Url> {
    match source {
        WebSource::Url(url) => Url::parse(url).ok(),
        WebSource::Html { html, .. } => html_url(html),
    }
}

/// `html` as a `data:` URL. A `data:` document has an opaque origin, so a
/// base URL cannot apply to it; relative links in it resolve to nothing.
fn html_url(html: &str) -> Option<Url> {
    let mut url = String::with_capacity(html.len() + 32);
    url.push_str("data:text/html;charset=utf-8,");
    for byte in html.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    Url::parse(&url).ok()
}

/// A live web view. Dropping it closes the page; the engine lives on while
/// the backend or another handle holds it.
struct ServoHandle {
    engine: Rc<Engine>,
    view: Rc<View>,
}

impl ServoHandle {
    fn with_webview(&self, f: impl FnOnce(&ServoWebView)) {
        if let Some(webview) = self.view.webview.borrow().as_ref() {
            f(webview);
        }
    }
}

impl WebViewHandle for ServoHandle {
    fn set_bounds(&self, bounds: Rect, scale_factor: f32) {
        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let size = PhysicalSize::new(
            (bounds.width * scale).round().max(1.0) as u32,
            (bounds.height * scale).round().max(1.0) as u32,
        );
        let view = &self.view;
        if (view.scale.get() - scale).abs() > f32::EPSILON {
            view.scale.set(scale);
            self.with_webview(|w| w.set_hidpi_scale_factor(Scale::new(scale)));
        }
        if view.size.get() != size {
            view.size.set(size);
            self.with_webview(|w| w.resize(size));
        }
    }
    fn load_url(&self, url: &str) {
        if let Ok(parsed) = Url::parse(url) {
            self.with_webview(|w| w.load(parsed));
        }
    }
    fn load_html(&self, html: &str, _base_url: Option<&str>) {
        if let Some(url) = html_url(html) {
            self.with_webview(|w| w.load(url));
        }
    }
    fn eval(&self, script: &str) {
        self.with_webview(|w| w.evaluate_javascript(script.to_string(), |_result| {}));
    }
    fn post_message(&self, msg: &str) {
        // No built-in IPC channel; emulate Rust→JS via a dispatched event,
        // through the shared escaper.
        let script = format!(
            "window.dispatchEvent(new MessageEvent('teksilo-message',{{data:{}}}))",
            js_string(msg)
        );
        self.with_webview(|w| w.evaluate_javascript(script, |_result| {}));
    }
    fn reload(&self) {
        self.with_webview(|w| w.reload());
    }
    fn go_back(&self) {
        self.with_webview(|w| {
            w.go_back(1);
        });
    }
    fn go_forward(&self) {
        self.with_webview(|w| {
            w.go_forward(1);
        });
    }
    fn stop(&self) {
        self.eval("window.stop()");
    }
    fn set_visible(&self, visible: bool) {
        self.view.shown.set(visible);
        self.with_webview(|w| {
            if visible {
                w.show();
            } else {
                w.hide();
            }
            // A hidden page runs no animation frames and no timers it can
            // defer, so a page parked in another tab costs next to nothing.
            w.set_throttled(!visible);
        });
        if visible && self.view.frame_ready.get() {
            // A frame reported while hidden is painted now.
            self.engine.waker.wake();
        }
    }
    fn set_focus(&self) {
        self.with_webview(|w| w.focus());
    }
    fn set_input_passthrough(&self, _passthrough: bool) {
        // A surface engine takes no input but what the widget forwards, and
        // the widget never asks it to pass any through.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backend_builds_its_engine_once_and_shares_it() {
        let built = AtomicBool::new(false);
        let mut slot = None;
        let mut builds = 0;
        let mut build = || {
            builds += 1;
            builds
        };
        assert_eq!(shared_engine(&mut slot, &built, &mut build), Ok(1));
        assert_eq!(shared_engine(&mut slot, &built, &mut build), Ok(1));
        assert_eq!(builds, 1);
    }

    #[test]
    fn a_second_backend_in_the_process_gets_no_engine() {
        let built = AtomicBool::new(false);
        let mut first = None;
        let mut second = None;
        assert_eq!(shared_engine(&mut first, &built, || 1), Ok(1));
        assert_eq!(shared_engine(&mut second, &built, || 2), Err(EngineTaken));
        assert_eq!(second, None);
    }

    #[test]
    fn inline_html_travels_as_a_data_url() {
        let url = html_url("<p>a&b # 50% é</p>").expect("a data: URL");
        assert_eq!(url.scheme(), "data");
        assert!(
            !url.as_str().contains('#'),
            "a raw '#' would cut the document at a fragment: {url}"
        );
        assert!(url.as_str().contains("%25"), "a raw '%' is escaped: {url}");
    }
}
