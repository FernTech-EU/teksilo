// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`LogView`] — a read-only, append-only, tail-following streaming view.
//!
//! The third face of the editor core, and the one that is *not* an editor. A
//! program writes to it, forever, faster than a person types; a person only
//! reads, scrolls, selects, and copies. That inversion is why it does not share
//! the editor's frame step — the details are in [`log_stream`]
//! — but it *is* the same [`CodeEditorState`], so
//! selection, copy, scrolling, theming, and accessibility come for free and
//! cannot drift from the editors'.
//!
//! What it adds over the read-only code viewer:
//!
//! - **Scale.** Only the visible rows are ever laid out, so a 100 000-line
//!   buffer costs a viewport's worth of memory, not the document's. Feed it a
//!   `scrollback_limit` to bound the raw text too.
//! - **Following the tail.** New lines stick the view to the bottom *while it is
//!   already at the bottom*; scroll up to read history and it pauses, scroll back
//!   and it resumes — derived from position, never a fight.
//! - **Severity colour.** An injected classifier paints a line by what it is (an
//!   error line red). Language-agnostic: the view colours a line, the
//!   application decides what an error looks like.

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{
    CursorIcon, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_text::text_document::TextDocument;
use bastyde_tokens::Color;

use super::log_stream::{self, LogStreamState};
use super::policy::CODE_READ_ONLY_PRESET;
use super::state::{CodeEditorState, SharedState};
use super::{adopt_shared_typesetter, construct};
use crate::common::scroll::OverscrollBehavior;
use crate::rich_text::ScrollPolicy;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVariant};

/// Overlay scrollbar thickness, matching the code editor and `ScrollArea`.
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// A read-only, append-only, tail-following log / console view.
///
/// Construct with [`LogView::new`], feed it with a [`LogViewHandle`] from
/// [`handle`](LogView::handle), and add it to the tree. It owns an internal
/// document; the application never touches one directly, it only appends lines.
pub struct LogView {
    state: SharedState,
    v_scroll_policy: ScrollPolicy,
    h_scroll_policy: ScrollPolicy,
    overscroll_behavior: OverscrollBehavior,

    body_id: Option<WidgetId>,
    v_scrollbar_id: Option<WidgetId>,
    h_scrollbar_id: Option<WidgetId>,
    v_scrollbar_bounds: Rc<Cell<Rect>>,
    h_scrollbar_bounds: Rc<Cell<Rect>>,
}

impl std::fmt::Debug for LogView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogView").finish_non_exhaustive()
    }
}

impl Default for LogView {
    fn default() -> Self {
        Self::new()
    }
}

impl LogView {
    /// A fresh, empty log view: read-only, no caret, no wrapping, following the
    /// tail, unbounded. Attach a [`handle`](LogView::handle) and append to it.
    pub fn new() -> Self {
        let state = construct(
            TextDocument::new(),
            CODE_READ_ONLY_PRESET,
            super::config::CodeConfig::default(),
            bastyde_text::WrapMode::None,
        );
        state.borrow_mut().log = Some(LogStreamState::new());
        Self {
            state,
            v_scroll_policy: ScrollPolicy::Auto,
            h_scroll_policy: ScrollPolicy::Auto,
            overscroll_behavior: OverscrollBehavior::default(),
            body_id: None,
            v_scrollbar_id: None,
            h_scrollbar_id: None,
            v_scrollbar_bounds: Rc::new(Cell::new(Rect::ZERO)),
            h_scrollbar_bounds: Rc::new(Cell::new(Rect::ZERO)),
        }
    }

    /// Whether new lines stick the view to the bottom when it is already there
    /// (default `true`). Off makes the view hold position while it grows.
    pub fn follow_tail(self, follow: bool) -> Self {
        if let Some(log) = self.state.borrow_mut().log.as_mut() {
            log.follow_enabled = follow;
        }
        self
    }

    /// Cap the retained lines: older lines beyond `limit` are evicted from the
    /// front. Unset (the default) keeps every line — *memory* stays flat in the
    /// line count, since only the visible window is ever shaped, but the raw text
    /// accumulates in the document and each append stays linear in the document's
    /// size. A genuinely unbounded, sustained high-rate producer should therefore
    /// set a limit; a bounded or bursty one need not. The cap is soft: eviction
    /// is batched, so the count can briefly exceed `limit` (by a band that scales
    /// down with the cap).
    pub fn scrollback_limit(self, limit: usize) -> Self {
        if let Some(log) = self.state.borrow_mut().log.as_mut() {
            log.scrollback_limit = Some(limit);
        }
        self
    }

    /// Colour each line by what it is: the classifier maps a line's text to a
    /// colour, or `None` to leave it in the default colour. The view knows how
    /// to colour a line; the application knows what an error line looks like.
    pub fn severity_highlighter(self, classify: impl Fn(&str) -> Option<Color> + 'static) -> Self {
        if let Some(log) = self.state.borrow_mut().log.as_mut() {
            log.severity = Some(Rc::new(classify));
        }
        self
    }

    /// Whether appended lines are announced to assistive technology (default
    /// `false`). Off is the right default: a live region is correct for a
    /// handful of meaningful events and hostile for a build log at fifty lines a
    /// second. The application says which it is.
    pub fn announce_appends(self, announce: bool) -> Self {
        self.state.borrow_mut().announce_appends = announce;
        self
    }

    /// Fallback font family. A log reads best monospaced, so columns align; pass
    /// a monospace family here.
    pub fn font_family(self, family: impl Into<String>) -> Self {
        {
            let mut st = self.state.borrow_mut();
            let mut d = st.engine.typography_defaults().clone();
            d.font_family = Some(family.into());
            st.engine.set_typography_defaults(d);
            st.needs_full_layout = true;
        }
        self
    }

    /// Whether the view grows text with the global accessibility text scale
    /// (default `true`).
    pub fn follow_text_scale(self, follow: bool) -> Self {
        self.state.borrow_mut().follow_text_scale = follow;
        self
    }

    /// Vertical scrollbar policy (default `Auto`).
    pub fn v_scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.v_scroll_policy = policy;
        self
    }

    /// Horizontal scrollbar policy (default `Auto`).
    pub fn h_scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.h_scroll_policy = policy;
        self
    }

    /// Override the background colour (accepts a `Color`, theme role, or
    /// `Signal`). Default tracks the theme's `editor_bg`.
    pub fn background(self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.state.borrow_mut().background_prop = Some(color.into());
        self
    }

    /// Override the default text colour. Per-line severity colours (from
    /// [`severity_highlighter`](Self::severity_highlighter)) still win.
    pub fn text_color(self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.state.borrow_mut().text_color_prop = Some(color.into());
        self
    }

    /// Override the selection colour.
    pub fn selection_color(self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.state.borrow_mut().selection_color_prop = Some(color.into());
        self
    }

    /// A cloneable handle to append to the view and drive it from anywhere.
    pub fn handle(&self) -> LogViewHandle {
        LogViewHandle {
            state: self.state.clone(),
        }
    }
}

impl Widget for LogView {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        adopt_shared_typesetter(&self.state, ctx);

        {
            let mut st = self.state.borrow_mut();
            st.frame_request = Some(ctx.frame_request_handle());
            st.frame_wake_at = Some(ctx.wake_at_handle());
            st.self_id = Some(ctx.self_id());
        }
        // Same dormancy discipline as `CodeEditor` / `RichTextEditor`: a log
        // view parked in a non-selected Switcher branch must not keep the
        // event loop awake via its streaming tick or window-active re-arm.
        let activation = ctx.activation_signal(ctx.self_id());
        if activation.get() {
            ctx.request_frame();
        }

        {
            let state = self.state.clone();
            ctx.effect(&activation, move |&active| {
                if active {
                    // **Re-activated** — re-arm the frame loop. The dormant branch
                    // below does not re-arm `frame_request` and the frame-tick
                    // effect (the streaming step: drain, evict, window, follow) is
                    // skipped entirely while dormant, so without this a log pane
                    // that is hidden and shown again never resumes streaming. Same
                    // defect and same fix as the editors.
                    let st = state.borrow();
                    if let Some(handle) = &st.frame_request {
                        handle.set(true);
                    }
                    return;
                }
                let mut st = state.borrow_mut();
                if st.has_focus {
                    st.has_focus = false;
                    st.focus_signal.set_if_changed(false);
                }
            });
        }

        // Frame-tick effect: the streaming step (drain, evict, window, follow).
        // Skipped while dormant so a hidden log pane does not pump frames.
        {
            let state = self.state.clone();
            let active = activation.clone();
            let tick_signal = ctx.frame_tick();
            ctx.effect(&tick_signal, move |delta| {
                if !active.get() {
                    return;
                }
                let mut st = state.borrow_mut();
                let more = log_stream::tick(&mut st, *delta);
                if more && let Some(handle) = &st.frame_request {
                    handle.set(true);
                }
            });
        }

        // Window-active effect: mirror the flag so the selection desaturates in
        // an inactive window (there is no caret to hide). Re-arm only while
        // this view is itself active.
        {
            let state = self.state.clone();
            let active = activation.clone();
            let wa_signal = ctx.window_active_signal();
            ctx.effect(&wa_signal, move |&window_active| {
                let mut st = state.borrow_mut();
                st.window_active = window_active;
                if active.get()
                    && let Some(handle) = &st.frame_request
                {
                    handle.set(true);
                }
            });
        }

        // Handlers on the wrapper — focus + event target. Reuses the editor's
        // pointer / scroll / tap handlers (drag-select works: as the drag
        // auto-scrolls, freshly-scrolled rows shape and the hit-test resolves
        // them), and a scroll-based keyboard of its own.
        let handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Text)
            .on_focus({
                let state = self.state.clone();
                move |gained, ctx| {
                    state.borrow_mut().focus_signal.set_if_changed(gained);
                    state.borrow_mut().has_focus = gained;
                    ctx.request_frame();
                }
            })
            .on_pointer_event({
                let state = self.state.clone();
                let v_sb = self.v_scrollbar_bounds.clone();
                let h_sb = self.h_scrollbar_bounds.clone();
                move |event, ctx| {
                    super::mouse::handle_pointer_event(&state, &v_sb, &h_sb, event, ctx)
                }
            })
            .on_scroll({
                let state = self.state.clone();
                let overscroll = self.overscroll_behavior;
                move |event, ctx| super::mouse::handle_scroll(&state, overscroll, event, ctx)
            })
            .on_key({
                let state = self.state.clone();
                move |event, ctx| log_stream::handle_log_key(&state, event, ctx)
            })
            .on_double_tap({
                let state = self.state.clone();
                move |event, ctx| super::mouse::handle_double_tap(&state, event.position, ctx)
            })
            .on_triple_tap({
                let state = self.state.clone();
                move |event, ctx| super::mouse::handle_triple_tap(&state, event.position, ctx)
            })
            .on_access_action_request({
                let state = self.state.clone();
                move |action, target, data, ctx| {
                    super::a11y::handle_access_action(&state, action, target, data, ctx)
                }
            });
        ctx.apply_self_handlers(handlers);

        let body = log_body_for(&self.state);
        let body_id = ctx.add(body);
        self.body_id = Some(body_id);

        // Reactive colour overrides repaint the body (the leaf that resolves
        // them).
        {
            let props = {
                let st = self.state.borrow();
                [st.text_color_prop.clone(), st.selection_color_prop.clone()]
            };
            let registry = ctx.binding_registry();
            for prop in props.iter().flatten() {
                prop.register_if_bound(body_id, registry, BindingLevel::RepaintOnly);
            }
        }

        let mut children = Vec::with_capacity(3);
        children.push(body_id);

        let (scroll_x, scroll_y, max_x, max_y, vr_x, vr_y) = {
            let st = self.state.borrow();
            (
                st.scroll_x.clone(),
                st.scroll_y.clone(),
                st.max_scroll_x.clone(),
                st.max_scroll_y.clone(),
                st.viewport_ratio_x.clone(),
                st.viewport_ratio_y.clone(),
            )
        };
        if self.v_scroll_policy != ScrollPolicy::AlwaysOff {
            let v = ScrollBar::new(
                ScrollBarOrientation::Vertical,
                scroll_y,
                max_y.clone(),
                vr_y,
            )
            .visual(ScrollBarVariant::Overlay);
            let id = ctx.add(v);
            self.v_scrollbar_id = Some(id);
            children.push(id);
        }
        if self.h_scroll_policy != ScrollPolicy::AlwaysOff {
            let h = ScrollBar::new(
                ScrollBarOrientation::Horizontal,
                scroll_x,
                max_x.clone(),
                vr_x,
            )
            .visual(ScrollBarVariant::Overlay);
            let id = ctx.add(h);
            self.h_scrollbar_id = Some(id);
            children.push(id);
        }

        // Re-place when a maximum crosses zero (an `Auto` bar appears/vanishes).
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        max_y.bind_to(self_id, registry, BindingLevel::Relayout);
        max_x.bind_to(self_id, registry, BindingLevel::Relayout);

        children
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Greedy: a log view is the scrollable region of a pane — take the space
        // and scroll.
        let w = proposal.width.unwrap_or(400.0).max(0.0);
        let h = proposal.height.unwrap_or(300.0).max(0.0);
        Size::new(w, h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        self.state.borrow_mut().node_origin = Point::new(bounds.x, bounds.y);

        let (max_y, max_x) = {
            let st = self.state.borrow();
            (st.max_scroll_y.get(), st.max_scroll_x.get())
        };
        let show_v = match self.v_scroll_policy {
            ScrollPolicy::AlwaysOn => true,
            ScrollPolicy::Auto => max_y > 0.0,
            ScrollPolicy::AlwaysOff => false,
        };
        let show_h = match self.h_scroll_policy {
            ScrollPolicy::AlwaysOn => true,
            ScrollPolicy::Auto => max_x > 0.0,
            ScrollPolicy::AlwaysOff => false,
        };

        let mut v_rect = Rect::ZERO;
        let mut h_rect = Rect::ZERO;
        for child in children.iter_mut() {
            if Some(child.id) == self.body_id {
                child.origin = Point::new(bounds.x, bounds.y);
                child.size = Size::new(bounds.width, bounds.height);
            } else if Some(child.id) == self.v_scrollbar_id {
                if show_v {
                    let h = if show_h {
                        (bounds.height - SCROLLBAR_THICKNESS).max(0.0)
                    } else {
                        bounds.height
                    };
                    child.origin =
                        Point::new(bounds.x + bounds.width - SCROLLBAR_THICKNESS, bounds.y);
                    child.size = Size::new(SCROLLBAR_THICKNESS, h);
                    v_rect = Rect::new(
                        bounds.width - SCROLLBAR_THICKNESS,
                        0.0,
                        SCROLLBAR_THICKNESS,
                        h,
                    );
                } else {
                    child.origin = Point::new(bounds.x, bounds.y);
                    child.size = Size::ZERO;
                }
            } else if Some(child.id) == self.h_scrollbar_id {
                if show_h {
                    let w = if show_v {
                        (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
                    } else {
                        bounds.width
                    };
                    child.origin =
                        Point::new(bounds.x, bounds.y + bounds.height - SCROLLBAR_THICKNESS);
                    child.size = Size::new(w, SCROLLBAR_THICKNESS);
                    h_rect = Rect::new(
                        0.0,
                        bounds.height - SCROLLBAR_THICKNESS,
                        w,
                        SCROLLBAR_THICKNESS,
                    );
                } else {
                    child.origin = Point::new(bounds.x, bounds.y);
                    child.size = Size::ZERO;
                }
            }
        }
        self.v_scrollbar_bounds.set(v_rect);
        self.h_scrollbar_bounds.set(h_rect);
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // Background, then a 1 px border that brightens on focus — minimal chrome
        // until a Tier-3 style lands, mirroring the code editor's wrapper.
        let bg = {
            let st = self.state.borrow();
            match &st.background_prop {
                Some(p) => p.resolve(ctx.theme, true),
                None => ctx.theme.colors.editor_bg,
            }
        };
        canvas.fill_rect(bounds, bg);

        let focused = self.state.borrow().focus_signal.get();
        let border = if focused {
            ctx.theme.colors.border_focused
        } else {
            ctx.theme.colors.border
        };
        canvas.stroke_rect(bounds, border, 1.0);
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids = Vec::with_capacity(3);
        ids.extend(self.body_id);
        ids.extend(self.v_scrollbar_id);
        ids.extend(self.h_scrollbar_id);
        ids
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// The paint-only leaf that renders the windowed log, split from the wrapper for
/// the same reason the code editor's body is.
pub(crate) struct LogViewBody {
    state: SharedState,
}

impl std::fmt::Debug for LogViewBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogViewBody").finish_non_exhaustive()
    }
}

/// Mount a log body over an existing state — the `body_for` analogue for the
/// read-only streaming face. Used by `LogView::build` and by tests that drive
/// the log body directly.
pub(crate) fn log_body_for(state: &SharedState) -> LogViewBody {
    LogViewBody {
        state: state.clone(),
    }
}

impl Widget for LogViewBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        let st = self.state.borrow();

        // An append repaints. It does NOT drive the accessibility rebuild — the
        // AT tree is whole-tree (no per-widget dirty tracking), so binding a
        // 100k-line streaming log's per-append version to it would re-walk the
        // entire app tree at frame rate. Instead the tree re-walks on the log's
        // own `a11y_version`, bumped only when the *visible window* changes: a
        // scroll crossing a row, a following-tail append, an eviction — never a
        // pixel-scroll on the same rows or a tail append while scrolled away.
        st.document_version
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        if let Some(log) = st.log.as_ref() {
            log.a11y_version
                .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        }
        // Scroll is repaint-only.
        for sig in [&st.scroll_x, &st.scroll_y] {
            sig.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        }
        // The log is read-only, but it still supports selection — that is what
        // makes its text copyable through AT. A selection change moves the caret
        // and anchor without moving the window, so the `a11y_version` binding
        // above does not fire; bind the caret signals at `AccessibilityOnly` too
        // so a within-window selection re-walks and the reported selection
        // tracks it. `has_selection` is derived from caret/anchor, so those two
        // cover every selection change.
        st.cursor_position
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        st.cursor_position
            .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        st.cursor_anchor
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        st.cursor_anchor
            .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        st.has_selection
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);

        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let w = proposal.width.unwrap_or(200.0).max(0.0);
        let h = proposal.height.unwrap_or(100.0).max(0.0);
        Size::new(w, h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        self.state.borrow_mut().sync_viewport(bounds);
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let mut st = self.state.borrow_mut();

        // Resolve the app's colour overrides against the live theme each paint.
        let new_text = match &st.text_color_prop {
            Some(p) => p.resolve(ctx.theme, true).to_array(),
            None => ctx.theme.colors.editor_fg.to_array(),
        };
        st.engine.set_text_color(new_text);

        let new_sel = if let Some(p) = st.selection_color_prop.as_ref() {
            p.resolve(ctx.theme, true).to_array()
        } else if ctx.window_active {
            ctx.theme.colors.editor_selection_bg.to_array()
        } else {
            ctx.theme.colors.selection_bg_inactive.to_array()
        };
        st.engine.set_selection_color(new_sel);

        // Logical font scale (a11y × font_size_scale) changes glyph advances
        // and the row height, so a change forces a re-window — and the scroll
        // offset must be rescaled into the new row-height coordinate space, or
        // a view scrolled away from the tail would jump to a different set of
        // lines.
        let target_scale = st.effective_font_scale(ctx.text_scale);
        let old_scale = st.last_font_scale;
        if old_scale.is_nan() || (old_scale - target_scale).abs() > f32::EPSILON {
            st.last_font_scale = target_scale;
            st.engine.set_font_scale(target_scale);
            if old_scale.is_finite() && old_scale > 0.0 {
                let ratio = target_scale / old_scale;
                let scaled = st.scroll_y.get() * ratio;
                st.scroll_y.set_if_changed(scaled);
            }
            if let Some(l) = st.log.as_mut() {
                l.needs_rewindow = true;
                l.row_height = 0.0;
            }
        }

        st.sync_viewport(bounds);
        // The authoritative window for the current (post-wheel) scroll offset.
        log_stream::ensure_window(&mut st, false);

        // Publish the selection to the engine — the caret stays hidden, but a
        // selection band is drawn for the resident rows it covers.
        let scroll_offset = st.scroll_y.get();
        let affinity = st.cursor_affinity;
        let cursors: Vec<bastyde_text::CursorDisplay> = st
            .all_carets()
            .map(|c| bastyde_text::CursorDisplay {
                position: c.position(),
                anchor: c.anchor(),
                affinity,
                visible: false,
                selected_cells: Vec::new(),
            })
            .collect();
        st.engine.set_cursors(&cursors);
        st.engine.set_scroll_offset(scroll_offset);

        canvas.set_clip(bounds);
        let CodeEditorState {
            ref mut engine,
            ref document,
            ref mut image_cache,
            ..
        } = *st;
        engine.with_render_frame(|frame| {
            crate::rich_text::paint::paint_frame(
                canvas,
                crate::rich_text::paint::PaintParams {
                    frame,
                    origin: Point::new(bounds.x, bounds.y),
                    document,
                    image_cache,
                    // No inline images on this surface, so none can be missing.
                    image_resolver: None,
                    selection: None,
                    selection_color: [0.0; 4],
                    draw_caret: false,
                },
            );
        });
        canvas.clear_clip();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use bastyde_core::accesskit::Live;

        let st = self.state.borrow();
        // Role::Document (a viewer — `Document` keeps caret + selection reportable
        // where `Log`/`Code` would not), read-only, and the *windowed* paragraph/
        // run tree: only the visible lines, so an append re-walks O(window), not
        // O(document).
        super::a11y::build_log_a11y(&st, builder);

        // A log that asked for it announces its new lines. Off by default —
        // `announce_appends` is the opt-in.
        if st.announce_appends {
            builder.inner_mut().set_live(Live::Polite);
        }
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// A cloneable handle to append to a [`LogView`] and drive it.
///
/// Use it on the UI thread — from an event handler, a timer, or an async
/// completion. It holds an `Rc`, so it is **not** `Send`; feeding a log from a
/// background thread (a PTY reader, a tracing layer) means marshalling the lines
/// to the UI thread first — through the app's async executor, or a channel whose
/// receiver is drained in a handler. Each append wakes the view, which otherwise
/// stops asking for frames when idle.
#[derive(Clone)]
pub struct LogViewHandle {
    state: SharedState,
}

impl std::fmt::Debug for LogViewHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogViewHandle").finish_non_exhaustive()
    }
}

impl LogViewHandle {
    /// Append text, split into lines on `\n`. A single trailing newline is a
    /// terminator, not a blank line, so it is dropped; embedded blank lines are
    /// kept. Enqueues for the next frame and wakes the view.
    pub fn append(&self, text: &str) {
        self.enqueue(text);
    }

    /// Append one line. `\n` is still split defensively — the document rejects a
    /// block containing one — so a value that turns out to be multi-line becomes
    /// several lines rather than an error.
    pub fn append_line(&self, line: &str) {
        self.enqueue(line);
    }

    /// Append many lines.
    pub fn append_lines<I, S>(&self, lines: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        {
            let st = self.state.borrow();
            let Some(log) = st.log.as_ref() else { return };
            let mut q = log.pending.lock().expect("log append queue poisoned");
            for line in lines {
                for piece in line.as_ref().split('\n') {
                    q.push_back(piece.to_string());
                }
            }
        }
        self.wake();
    }

    fn enqueue(&self, text: &str) {
        {
            let st = self.state.borrow();
            let Some(log) = st.log.as_ref() else { return };
            let mut q = log.pending.lock().expect("log append queue poisoned");
            // Drop exactly one trailing newline (a line terminator), then split.
            let body = text.strip_suffix('\n').unwrap_or(text);
            for piece in body.split('\n') {
                q.push_back(piece.to_string());
            }
        }
        self.wake();
    }

    /// Empty the view, resetting it to its pristine state. UI-thread only.
    pub fn clear(&self) {
        {
            let mut st = self.state.borrow_mut();
            if let Some(log) = st.log.as_ref() {
                log.pending
                    .lock()
                    .expect("log append queue poisoned")
                    .clear();
            }
            let _ = st.document.set_plain_text("");
            if let Some(log) = st.log.as_mut() {
                log.pristine = true;
                log.total = 0;
                log.anchor = None;
                log.last_window = None;
                log.needs_rewindow = true;
            }
            st.line_count.set_if_changed(0);
            st.scroll_x.set_if_changed(0.0);
            st.scroll_y.set_if_changed(0.0);
        }
        self.wake();
    }

    /// Scroll to the bottom, resuming tail-following. UI-thread only.
    pub fn scroll_to_bottom(&self) {
        {
            let st = self.state.borrow();
            let max_y = st.max_scroll_y.get();
            st.scroll_y.set_if_changed(max_y);
        }
        self.wake();
    }

    /// The live line count — a status bar can bind it.
    pub fn line_count(&self) -> bastyde_core::Signal<usize> {
        self.state.borrow().line_count.clone()
    }

    /// Bumps on every content change.
    pub fn document_version(&self) -> bastyde_core::Signal<u64> {
        self.state.borrow().document_version.clone()
    }

    /// The vertical scroll offset — a follow-state indicator can read it against
    /// [`max_scroll_y`](Self::max_scroll_y).
    pub fn scroll_y(&self) -> bastyde_core::Signal<f32> {
        self.state.borrow().scroll_y.clone()
    }

    /// The maximum vertical scroll offset.
    pub fn max_scroll_y(&self) -> bastyde_core::Signal<f32> {
        self.state.borrow().max_scroll_y.clone()
    }

    /// Wake the view so it drains and repaints on the next frame.
    fn wake(&self) {
        if let Some(handle) = &self.state.borrow().frame_request {
            handle.set(true);
        }
    }

    #[cfg(test)]
    pub(crate) fn state_handle(&self) -> SharedState {
        self.state.clone()
    }

    #[cfg(test)]
    pub(crate) fn from_state_for_test(state: SharedState) -> Self {
        Self { state }
    }
}
