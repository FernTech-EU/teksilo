// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The public editing surfaces: [`CodeEditor`] and [`PlainTextEditor`].
//!
//! The wrapper is the focus + event target; it owns the gutter (optional), the
//! paint-only body, and the overlay scrollbars, joined to them only through the
//! shared [`CodeEditorState`](super::state::CodeEditorState). This mirrors
//! `RichTextEditor` exactly — the wrapper carries focus so a future style may
//! place the body anywhere in its chrome without the focus semantics moving —
//! and adds the two things a source editor needs on top: a line-number gutter to
//! the left, and a paint pass that draws the current-line band (across gutter and
//! body) and the matched-bracket cells behind the text.
//!
//! `PlainTextEditor` is the same machinery with the code affordances off and
//! wrapping on — a notes field, a commit message — so the two never drift.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{
    CursorIcon, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_text::text_document::TextDocument;
use teksilo_text::{CursorAffinity, WrapMode};

use super::completion::{self, CompletionContext, CompletionItem, CompletionPanel};
use super::config::{BracketPair, CodeConfig, IndentStyle};
use super::gutter::CodeGutter;
use super::policy::{CODE_EDITOR_PRESET, CODE_READ_ONLY_PRESET};
use super::state::SharedState;
use super::{CodeEditorHandle, adopt_shared_typesetter, body_for, construct};
use crate::common::editor_runtime::CaretPolicy;
use crate::common::scroll::OverscrollBehavior;
use crate::rich_text::ScrollPolicy;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVariant};

/// Overlay scrollbar thickness, matching the rich-text editor and `ScrollArea`.
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// A multi-line source-code editing surface: gutter, current-line highlight,
/// indentation, bracket handling, and multiple carets.
///
/// Construct with [`CodeEditor::new`] (editable) or [`CodeEditor::read_only`]
/// (view + select + copy). Every code affordance is injected configuration, not
/// a built-in language — see [`CodeConfig`].
pub struct CodeEditor {
    state: SharedState,
    v_scroll_policy: ScrollPolicy,
    h_scroll_policy: ScrollPolicy,
    overscroll_behavior: OverscrollBehavior,
    min_lines: Option<u32>,
    max_lines: Option<u32>,
    show_gutter: bool,

    // Child ids, filled during `build`.
    gutter_id: Option<WidgetId>,
    body_id: Option<WidgetId>,
    v_scrollbar_id: Option<WidgetId>,
    h_scrollbar_id: Option<WidgetId>,
    // Scrollbar window-local bounds, published by `place_children`, read by the
    // pointer handler to bypass the drag-select latch over an overlay bar.
    v_scrollbar_bounds: Rc<Cell<Rect>>,
    h_scrollbar_bounds: Rc<Cell<Rect>>,
    // Gutter width, published by `place_children` so `paint` can offset the
    // bracket cells into body space.
    gutter_width: Rc<Cell<f32>>,
}

impl std::fmt::Debug for CodeEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeEditor")
            .field("policy", &self.state.borrow().policy)
            .field("show_gutter", &self.show_gutter)
            .finish_non_exhaustive()
    }
}

impl CodeEditor {
    /// An editable code editor bound to `document`: gutter on, current-line
    /// highlight on, no wrapping. Code affordances (comment token, bracket
    /// pairs) stay off until the application supplies them — the editor never
    /// guesses a language.
    pub fn new(document: TextDocument) -> Self {
        let this = Self::from_state(construct(
            document,
            CODE_EDITOR_PRESET,
            CodeConfig::default(),
            WrapMode::None,
        ));
        this.state.borrow_mut().current_line_highlight = true;
        this
    }

    /// A read-only code viewer bound to `document`: no caret, navigation and
    /// copy only, `Role::Document`. Still gets the gutter and syntax colours.
    pub fn read_only(document: TextDocument) -> Self {
        Self::from_state(construct(
            document,
            CODE_READ_ONLY_PRESET,
            CodeConfig::default(),
            WrapMode::None,
        ))
    }

    fn from_state(state: SharedState) -> Self {
        Self {
            state,
            v_scroll_policy: ScrollPolicy::Auto,
            h_scroll_policy: ScrollPolicy::Auto,
            overscroll_behavior: OverscrollBehavior::default(),
            min_lines: None,
            max_lines: None,
            show_gutter: true,
            gutter_id: None,
            body_id: None,
            v_scrollbar_id: None,
            h_scrollbar_id: None,
            v_scrollbar_bounds: Rc::new(Cell::new(Rect::ZERO)),
            h_scrollbar_bounds: Rc::new(Cell::new(Rect::ZERO)),
            gutter_width: Rc::new(Cell::new(0.0)),
        }
    }

    // --- Shared builder methods ------------------------------------------

    /// Set the line-wrap mode. `CodeEditor` defaults to `WrapMode::None` (source
    /// lines must not fold, or the gutter's one-number-per-line correspondence
    /// breaks); pair with `.h_scroll_policy(Auto)` to scroll wide lines.
    pub fn wrap_mode(self, mode: WrapMode) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.wrap_mode = mode;
            st.engine.set_wrap_mode(mode);
            st.needs_full_layout = true;
        }
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

    /// Wheel scroll-chaining at the editor's scroll boundary. `Chain` (default)
    /// hands leftover scroll to an enclosing scrollable; `Contain` absorbs it.
    pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.overscroll_behavior = behavior;
        self
    }

    /// Cull the render to the visible clip band (default `false`). Turn on only
    /// for an editor deliberately laid out at full document height inside an
    /// outer `ScrollArea` (`v_scroll_policy(AlwaysOff)` + `min_lines(1)`): the
    /// body's bounds then span the whole document, and this renders only the
    /// on-screen slice instead of every line. A normally-scrolling editor already
    /// renders just a viewport's worth, so it needs nothing.
    pub fn window_to_clip(self, on: bool) -> Self {
        self.state.borrow_mut().window_to_clip = on;
        self
    }

    /// Minimum visible height in lines — switches the editor from greedy (fill
    /// the proposal) to intrinsic sizing (grow with content up to `max_lines`,
    /// then scroll). The composer pattern.
    pub fn min_lines(mut self, lines: u32) -> Self {
        self.min_lines = Some(lines);
        self
    }

    /// Maximum visible height in lines — caps intrinsic growth.
    pub fn max_lines(mut self, lines: u32) -> Self {
        self.max_lines = Some(lines);
        self
    }

    /// Fallback font family for the document's text. `None` (the default) keeps
    /// the typesetter's registry default; a code editor should pass a monospace
    /// family so columns line up.
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

    /// Per-editor logical font-size multiplier (`1.0` = 100 %), composed with
    /// the accessibility text scale when [`follow_text_scale`](Self::follow_text_scale)
    /// is on. Sharp — shapes at a larger ppem.
    pub fn font_size_scale(self, scale: f32) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.font_size_scale = scale.clamp(0.1, 10.0);
            st.last_font_scale = f32::NAN;
            st.needs_full_layout = true;
        }
        self
    }

    /// Whether the editor grows text with the global accessibility text scale
    /// (default `true`). Turn off for a WYSIWYG surface whose font sizes are
    /// document content. Composed with [`font_size_scale`](Self::font_size_scale).
    pub fn follow_text_scale(self, follow: bool) -> Self {
        self.state.borrow_mut().follow_text_scale = follow;
        self
    }

    /// A callback fired once per drain batch that contained a real content edit.
    pub fn on_change(self, callback: impl Fn() + 'static) -> Self {
        self.state.borrow_mut().on_change = Some(Rc::new(callback));
        self
    }

    /// Override the editor background colour (accepts `Color`, a theme role, or a
    /// `Signal`). `None`-equivalent default tracks the theme's `editor_bg`.
    pub fn background(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self {
        self.state.borrow_mut().background_prop = Some(color.into());
        self
    }

    /// Override the text colour. Default tracks the theme's `editor_fg`.
    pub fn text_color(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self {
        self.state.borrow_mut().text_color_prop = Some(color.into());
        self
    }

    /// Override the caret colour. Default tracks the theme's `editor_caret`.
    pub fn caret_color(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self {
        self.state.borrow_mut().caret_color_prop = Some(color.into());
        self
    }

    /// Override the selection colour. A pinned colour opts out of the
    /// window-inactive desaturation.
    pub fn selection_color(self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self {
        self.state.borrow_mut().selection_color_prop = Some(color.into());
        self
    }

    // --- Code-only builder methods ---------------------------------------

    /// Whether the line-number gutter is shown (default `true`).
    pub fn gutter(mut self, show: bool) -> Self {
        self.show_gutter = show;
        self
    }

    /// Whether the caret's line gets a full-width background wash (default
    /// `true` for `CodeEditor`).
    pub fn current_line_highlight(self, on: bool) -> Self {
        self.state.borrow_mut().current_line_highlight = on;
        self
    }

    /// Set the indentation style directly (spaces of a width, or tabs rendered a
    /// width wide).
    pub fn indent_style(self, style: IndentStyle) -> Self {
        self.state.borrow_mut().config.indent = style;
        self
    }

    /// Set the indent width, keeping the current spaces-vs-tabs kind.
    pub fn tab_width(self, width: u8) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.config.indent = match st.config.indent {
                IndentStyle::Spaces(_) => IndentStyle::Spaces(width),
                IndentStyle::Tabs { .. } => IndentStyle::Tabs { width },
            };
        }
        self
    }

    /// Whether indentation is written with spaces (`true`, the default) or a tab
    /// character (`false`), keeping the current width.
    pub fn use_soft_tabs(self, soft: bool) -> Self {
        {
            let mut st = self.state.borrow_mut();
            let w = st.config.indent.width();
            st.config.indent = if soft {
                IndentStyle::Spaces(w)
            } else {
                IndentStyle::Tabs { width: w }
            };
        }
        self
    }

    /// Whether Enter carries the current line's indentation onto the new line
    /// (default `true`).
    pub fn auto_indent(self, on: bool) -> Self {
        self.state.borrow_mut().config.auto_indent = on;
        self
    }

    /// The delimiter pairs the editor auto-closes and match-highlights. Empty
    /// (the default) disables both.
    pub fn bracket_pairs(self, pairs: impl Into<Vec<BracketPair>>) -> Self {
        self.state.borrow_mut().config.brackets = pairs.into();
        self
    }

    /// Whether typing an opener inserts its closing partner (default `false`;
    /// needs configured `bracket_pairs`).
    pub fn auto_close_brackets(self, on: bool) -> Self {
        self.state.borrow_mut().config.auto_close_brackets = on;
        self
    }

    /// Whether the delimiter matching the caret's is highlighted (default
    /// `false`; needs configured `bracket_pairs`).
    pub fn bracket_matching(self, on: bool) -> Self {
        self.state.borrow_mut().config.match_brackets = on;
        self
    }

    /// The token that starts a line comment (`"//"`, `"#"`, `"--"`). Enables
    /// `Ctrl+/` comment toggling; unset (the default) leaves it a no-op rather
    /// than guessing.
    pub fn line_comment(self, token: impl Into<String>) -> Self {
        self.state.borrow_mut().config.line_comment = Some(token.into());
        self
    }

    /// Supply the completion candidates. The provider is called for the word
    /// being completed and given a [`CompletionContext`]; the editor filters its
    /// result by the live prefix, shows the popup, and replaces the word on
    /// accept. Language-agnostic — the app knows the candidates, the editor knows
    /// the mechanics. Without a provider there is no completion.
    pub fn completion_provider(
        self,
        provider: impl Fn(&CompletionContext) -> Vec<CompletionItem> + 'static,
    ) -> Self {
        self.state.borrow_mut().completion.provider = Some(Rc::new(provider));
        self
    }

    /// Whether typing an identifier character opens the completion popup
    /// automatically (default `true`). When off, only `Ctrl+Space` opens it.
    pub fn auto_complete(self, auto: bool) -> Self {
        self.state.borrow_mut().completion.auto_trigger = auto;
        self
    }

    /// A cloneable handle to drive the editor from a toolbar, shortcut, or test.
    pub fn handle(&self) -> CodeEditorHandle {
        CodeEditorHandle::new(self.state.clone())
    }

    // --- Internal ---------------------------------------------------------

    /// The vertical extent (window y, height) of the caret's line, or `None`
    /// before a layout exists.
    fn caret_line_band(st: &super::state::CodeEditorState) -> Option<(f32, f32)> {
        if !st.engine.has_full_layout() {
            return None;
        }
        let c = st
            .engine
            .caret_rect(st.cursor.position(), st.cursor_affinity);
        let y = st.viewport_origin.y + c[1] - st.scroll_y.get();
        Some((y, c[3]))
    }
}

impl Widget for CodeEditor {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Tell the framework this widget edits text — registered on *this*
        // node, the `.focusable(true)` one below, because the registry is keyed
        // by whichever widget holds the focus. See `teksilo_core::text_surface`.
        ctx.register_text_surface(std::rc::Rc::new(self.handle()));

        // Swap the private engine for one sharing the app's typesetter (no-op
        // headless), carrying over builder-set typography.
        adopt_shared_typesetter(&self.state, ctx);

        {
            let mut st = self.state.borrow_mut();
            st.frame_request = Some(ctx.frame_request_handle());
            st.frame_wake_at = Some(ctx.wake_at_handle());
            st.self_id = Some(ctx.self_id());
        }
        // Same dormancy discipline as `RichTextEditor` / `TextInputField`: a
        // code or plain-text editor parked in a non-selected Switcher branch
        // must not keep the event loop awake. `PlainTextEditor` is a thin
        // wrap of this widget, so it inherits the gate for free.
        let activation = ctx.activation_signal(ctx.self_id());
        if activation.get() {
            ctx.request_frame();
        }

        {
            let state = self.state.clone();
            ctx.effect(&activation, move |&active| {
                if active {
                    // **Re-activated** — re-arm the frame loop. The dormant branch
                    // below does not re-arm `frame_request` (by design: a parked
                    // editor has nothing to paint) and the frame-tick effect is
                    // skipped entirely while dormant, so nothing restarts the tick
                    // on the way back. Only the tick pushes the cursor through to
                    // the engine, so a re-activated editor that is then focused
                    // draws **no caret at all**.
                    //
                    // The in-tree modal path takes this route on every open —
                    // build, `set_dormant`, mount, `activate`, *then* focus (see
                    // `present_in_tree_modal_request`) — as do a tab switch and a
                    // collapsed pane. Same fix as `RichTextEditor`; this file backs
                    // both `CodeEditor` and `PlainTextEditor`.
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
                st.caret_visible.set_if_changed(false);
                st.blink.reset();
            });
        }

        // Frame-tick effect: drain events, blink, lay out, publish metrics.
        // Skipped while dormant so multi-tab / multi-page hosts do not pay
        // O(open editors) per wake for surfaces nobody can see.
        {
            let state = self.state.clone();
            let active = activation.clone();
            let tick_signal = ctx.frame_tick();
            ctx.effect(&tick_signal, move |delta| {
                if !active.get() {
                    return;
                }
                let mut st = state.borrow_mut();
                let more = super::frame_loop::tick(&mut st, *delta);
                if more && let Some(handle) = &st.frame_request {
                    handle.set(true);
                }
            });
        }

        // Window-active effect: hide the caret synchronously on deactivation
        // (the loop may not tick while the window is inactive). Re-arm the
        // frame loop only while this editor is itself active.
        {
            let state = self.state.clone();
            let active = activation.clone();
            let wa_signal = ctx.window_active_signal();
            ctx.effect(&wa_signal, move |&window_active| {
                let mut st = state.borrow_mut();
                st.window_active = window_active;
                if window_active {
                    let show =
                        st.has_focus && !matches!(st.policy.caret_policy, CaretPolicy::Hidden);
                    if show {
                        st.caret_visible.set_if_changed(true);
                    }
                    st.blink.reset();
                } else {
                    st.caret_visible.set_if_changed(false);
                    st.blink.reset();
                }
                if active.get()
                    && let Some(handle) = &st.frame_request
                {
                    handle.set(true);
                }
            });
        }

        // Handlers on the wrapper — the focus + event target.
        let mut handlers = HandlerSet::new();
        if !self.state.borrow().policy.is_read_only() {
            handlers = handlers.ime_input(teksilo_core::ime::ImeContext::text());
        }
        handlers = handlers
            .focusable(true)
            .cursor(CursorIcon::Text)
            .on_focus({
                let state = self.state.clone();
                move |gained, ctx| {
                    {
                        let mut st = state.borrow_mut();
                        st.has_focus = gained;
                        st.focus_signal.set_if_changed(gained);
                        if gained && matches!(st.policy.caret_policy, CaretPolicy::Blinking) {
                            st.blink.restart();
                            st.caret_visible.set_if_changed(true);
                        }
                    }
                    if gained {
                        super::keyboard::report_ime_cursor_area(&state, ctx);
                    } else {
                        super::keyboard::clear_ime_preedit(&state);
                        // A popup that outlived its editor's focus would float
                        // detached — close it on blur.
                        completion::close(&state, ctx);
                        let mut st = state.borrow_mut();
                        st.last_ime_area = None;
                        st.last_chase_pos = None;
                    }
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
                move |event, ctx| super::keyboard::handle_key(&state, event, ctx)
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

        // Body — the pure-paint leaf. Always greedy: the wrapper does intrinsic
        // sizing (min/max_lines) and hands the body its final rect.
        let body = body_for(&self.state, None, None);
        let body_id = ctx.add(body);
        self.body_id = Some(body_id);

        // Reactive colour overrides repaint the body (the leaf that resolves
        // them). Theme-role changes already dirty every node; this covers
        // Signal-bound props.
        {
            let props = {
                let st = self.state.borrow();
                [
                    st.text_color_prop.clone(),
                    st.caret_color_prop.clone(),
                    st.selection_color_prop.clone(),
                ]
            };
            let registry = ctx.binding_registry();
            for prop in props.iter().flatten() {
                prop.register_if_bound(body_id, registry, BindingLevel::RepaintOnly);
            }
        }

        let mut children = Vec::with_capacity(4);
        if self.show_gutter {
            let gutter_id = ctx.add(CodeGutter::new(&self.state));
            self.gutter_id = Some(gutter_id);
            children.push(gutter_id);
        }
        children.push(body_id);

        // Overlay scrollbars driven by the metrics the frame loop publishes.
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

        // Completion popup content — pre-created and kept dormant (the ComboBox
        // dropdown pattern), so it is never an orphan arena root and never
        // ghost-paints while logically closed. `show_overlay` moves it to the
        // overlay layer when completion opens.
        if self.state.borrow().completion.has_provider() {
            let open = self.state.borrow().completion.open.clone();
            // Built the first time completion opens, not on every rebuild of the
            // editor. See `teksilo_core::deferred_subtree::DeferredSubtree`.
            let panel_id = ctx.add_deferred(open.clone(), CompletionPanel::new(&self.state));
            ctx.set_dormant(panel_id);
            ctx.visible_when(panel_id, open);
            self.state.borrow_mut().completion.panel_id = Some(panel_id);
            children.push(panel_id);
        }

        // The `Auto` scrollbars appear only when there is overflow; those maxima
        // are published by the frame loop, so re-place when they cross zero.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        max_y.bind_to(self_id, registry, BindingLevel::Relayout);
        max_x.bind_to(self_id, registry, BindingLevel::Relayout);

        children
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let w = proposal.width.unwrap_or(400.0).max(0.0);

        // Greedy unless the composer knobs are set.
        if self.min_lines.is_none() && self.max_lines.is_none() {
            let h = proposal.height.unwrap_or(300.0).max(0.0);
            return Size::new(w, h).into();
        }

        let st = self.state.borrow();
        let line_scale = st.effective_font_scale(ctx.text_scale);
        let line_h = st.engine.default_line_height() * line_scale;
        let content_h = st.engine.content_height();
        drop(st);

        let min_h = self.min_lines.map(|n| n as f32 * line_h).unwrap_or(0.0);
        let max_h = self
            .max_lines
            .map(|n| n as f32 * line_h)
            .unwrap_or(f32::INFINITY);
        Size::new(w, content_h.clamp(min_h, max_h).max(0.0)).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.state.borrow_mut().node_origin = Point::new(bounds.x, bounds.y);

        // Gutter width, measured from its intrinsic response (it sizes to the
        // widest line number the document will ever hold).
        let gutter_w = self
            .gutter_id
            .and_then(|id| ctx.child_size(id, SizeProposal::with_height(bounds.height)))
            .map(|s| s.width)
            .unwrap_or(0.0);
        self.gutter_width.set(gutter_w);

        let body_x = bounds.x + gutter_w;
        let body_w = (bounds.width - gutter_w).max(0.0);

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
            if Some(child.id) == self.gutter_id {
                child.origin = Point::new(bounds.x, bounds.y);
                child.size = Size::new(gutter_w, bounds.height);
            } else if Some(child.id) == self.body_id {
                child.origin = Point::new(body_x, bounds.y);
                child.size = Size::new(body_w, bounds.height);
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
                        child.origin.x - bounds.x,
                        child.origin.y - bounds.y,
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
                        (body_w - SCROLLBAR_THICKNESS).max(0.0)
                    } else {
                        body_w
                    };
                    child.origin =
                        Point::new(body_x, bounds.y + bounds.height - SCROLLBAR_THICKNESS);
                    child.size = Size::new(w, SCROLLBAR_THICKNESS);
                    h_rect = Rect::new(
                        child.origin.x - bounds.x,
                        child.origin.y - bounds.y,
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
        // Background fill, then the current-line band, then the matched-bracket
        // cells — all behind the children (gutter numbers, body text), which
        // paint on top. A band spanning gutter and body is exactly why it lives
        // on the wrapper rather than in either child.
        let st = self.state.borrow();

        let bg = match &st.background_prop {
            Some(p) => p.resolve(ctx.theme, true),
            None => ctx.theme.colors.editor_bg,
        };
        canvas.fill_rect(bounds, bg);

        // Current-line band: only for a single collapsed caret in a focused,
        // active window — a band under a selection or several carets reads as
        // noise, which is the convention every editor follows.
        let single_collapsed = st.extra_carets.is_empty() && !st.cursor.has_selection();
        if st.current_line_highlight
            && st.has_focus
            && st.window_active
            && single_collapsed
            && let Some((y, h)) = Self::caret_line_band(&st)
            && y + h > bounds.y
            && y < bounds.y + bounds.height
        {
            let band = Rect::new(bounds.x, y, bounds.width, h);
            canvas.fill_rect(band, ctx.theme.colors.surface_hover);
        }

        // Matched-bracket cells: a faint wash behind each of the two brackets.
        if let Some((a, b)) = st.bracket_match.get()
            && st.engine.has_full_layout()
        {
            let origin = st.viewport_origin;
            let scroll_x = st.scroll_x.get();
            let scroll_y = st.scroll_y.get();
            for p in [a, b] {
                let r0 = st.engine.caret_rect(p, CursorAffinity::Downstream);
                let r1 = st.engine.caret_rect(p + 1, CursorAffinity::Downstream);
                let x = origin.x + r0[0] - scroll_x;
                let w = (r1[0] - r0[0]).max(2.0);
                let y = origin.y + r0[1] - scroll_y;
                let h = r0[3];
                // Clip to the body region so a bracket scrolled behind the
                // gutter does not paint over the numbers.
                if x + w > origin.x && y + h > bounds.y && y < bounds.y + bounds.height {
                    canvas.fill_rect(Rect::new(x, y, w, h), ctx.theme.colors.accent_subtle_bg);
                }
            }
        }

        drop(st);

        // A 1 px border that brightens on focus — minimal chrome until a Tier-3
        // style lands.
        let focused = self.state.borrow().focus_signal.get();
        let border = if focused {
            ctx.theme.colors.border_focused
        } else {
            ctx.theme.colors.border
        };
        canvas.stroke_rect(bounds, border, 1.0);
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // The role, actions, and (in the a11y phase) the paragraph/run tree live
        // on the body leaf, mirroring RichTextEditor. The wrapper stays a plain
        // focusable container.
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids = Vec::with_capacity(5);
        ids.extend(self.gutter_id);
        ids.extend(self.body_id);
        ids.extend(self.v_scrollbar_id);
        ids.extend(self.h_scrollbar_id);
        // The completion popup (a dormant overlay node) — tracked here so it is
        // not an orphan; positioned by the overlay manager when shown, skipped by
        // `place_children` otherwise.
        ids.extend(self.state.borrow().completion.panel_id);
        ids
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// A multi-line plain-text editing surface — the code editor with its code
/// affordances off and wrapping on. A notes field, a commit message, a
/// description box.
///
/// It shares [`CodeEditor`]'s machinery (caret, selection, IME, clipboard,
/// scrolling, accessibility); the difference is configuration, so the two never
/// drift. Construct with [`PlainTextEditor::new`] / [`PlainTextEditor::read_only`].
#[derive(Debug)]
pub struct PlainTextEditor {
    inner: Option<CodeEditor>,
    inner_id: Option<WidgetId>,
}

impl PlainTextEditor {
    /// An editable plain-text editor bound to `document`: no gutter, no
    /// current-line highlight, word wrapping, and no code affordances.
    pub fn new(document: TextDocument) -> Self {
        Self::wrap(CodeEditor::new(document))
    }

    /// A read-only plain-text viewer bound to `document`.
    pub fn read_only(document: TextDocument) -> Self {
        Self::wrap(CodeEditor::read_only(document))
    }

    fn wrap(editor: CodeEditor) -> Self {
        // Plain-text defaults: fold the code chrome away, wrap like prose.
        let editor = editor
            .gutter(false)
            .current_line_highlight(false)
            .wrap_mode(WrapMode::Word);
        Self {
            inner: Some(editor),
            inner_id: None,
        }
    }

    /// Restrict growth to `[min, max]` lines (intrinsic sizing — the composer
    /// pattern).
    pub fn min_lines(mut self, lines: u32) -> Self {
        self.map(|e| e.min_lines(lines));
        self
    }

    /// Cap intrinsic growth at `lines`.
    pub fn max_lines(mut self, lines: u32) -> Self {
        self.map(|e| e.max_lines(lines));
        self
    }

    /// Set the line-wrap mode (default `Word`).
    pub fn wrap_mode(mut self, mode: WrapMode) -> Self {
        self.map(|e| e.wrap_mode(mode));
        self
    }

    /// Fallback font family.
    pub fn font_family(mut self, family: impl Into<String>) -> Self {
        self.map(|e| e.font_family(family));
        self
    }

    /// Whether the editor follows the global accessibility text scale.
    pub fn follow_text_scale(mut self, follow: bool) -> Self {
        self.map(|e| e.follow_text_scale(follow));
        self
    }

    /// Per-editor logical font-size multiplier (`1.0` = 100 %).
    pub fn font_size_scale(mut self, scale: f32) -> Self {
        self.map(|e| e.font_size_scale(scale));
        self
    }

    /// A callback fired on each content-changing edit batch.
    pub fn on_change(mut self, callback: impl Fn() + 'static) -> Self {
        self.map(|e| e.on_change(callback));
        self
    }

    /// Override the background colour.
    pub fn background(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self {
        self.map(|e| e.background(color));
        self
    }

    /// A cloneable handle to drive the editor.
    pub fn handle(&self) -> CodeEditorHandle {
        self.inner.as_ref().expect("handle() before build").handle()
    }

    /// Apply `f` to the inner editor in place (builders consume and return it).
    fn map(&mut self, f: impl FnOnce(CodeEditor) -> CodeEditor) {
        if let Some(e) = self.inner.take() {
            self.inner = Some(f(e));
        }
    }
}

impl Widget for PlainTextEditor {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let inner = self.inner.take().expect("PlainTextEditor built once");
        let id = ctx.add(inner);
        self.inner_id = Some(id);
        vec![id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.inner_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(child) = children.first_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.inner_id.into_iter().collect()
    }
}
