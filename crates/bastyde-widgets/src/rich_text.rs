// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Rich text editor and viewer widget.
//!
//! Two construction presets share the same implementation: [`RichTextEditor::editor`]
//! provides a full editing surface (blinking caret, keyboard commands, clipboard,
//! undo/redo, `Role::MultilineTextInput`) and [`RichTextEditor::read_only`] is a
//! view-only surface (hidden caret, mutations rejected, `Role::Document`). Both
//! bind to an external [`TextDocument`]
//! via `on_change` subscriptions, so any number of editors and viewers can share
//! one document and observe each other's edits live.
//!
//! The widget owns a per-widget `RichTextEngine` (typesetter), and drives its own
//! scroll bars independently of `ScrollArea` to avoid the wrap/scrollbar circular
//! measurement dependency. Use [`RichTextEditor::min_lines`] /
//! [`RichTextEditor::max_lines`] to switch from greedy sizing to intrinsic
//! (messenger-composer) sizing. A detachable [`EditorHandle`] lets toolbars and
//! palette panels issue formatting commands from closures that cannot borrow the
//! editor directly.
//!
//! ```ignore
//! use bastyde_text::text_document::TextDocument;
//! let doc = TextDocument::new();
//! let editor = RichTextEditor::editor(doc)
//!     .min_lines(3)
//!     .max_lines(8)
//!     .wrap_mode(WrapMode::Word);
//! ```

pub mod caret_highlight;
mod clipboard;
mod context_menu;
mod find_session;
mod frame_loop;
// `pub(crate)` so the code editor can reuse the hit-test wrapper rather than
// re-deriving pointer-to-offset resolution. Both surfaces ask the same engine
// the same question; the answer should not have two implementations.
pub(crate) mod hit_test;
pub(crate) mod image_cache;
mod keyboard;
mod mouse;
pub(crate) mod paint;
mod policy;
mod state;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod window_tests;

pub use context_menu::{
    INTENT_COPY, INTENT_CUT, INTENT_PASTE, INTENT_PASTE_UNFORMATTED, INTENT_SELECT_ALL,
};
pub use find_session::FindSession;
pub use hit_test::ContextTarget;
pub use policy::{
    AccessibilityRole, CaretPolicy, ClipboardPolicy, CommandFilter, EDITOR_PRESET, EditCommandKind,
    PolicyBundle, READ_ONLY_PRESET,
};

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    RichTextEditorStyle, RichTextEditorStyleConfig, SharedRichTextEditorStyle,
};
use bastyde_core::widget::{CursorIcon, LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_text::text_document::{
    Alignment, BlockFormat, CharVerticalAlignment, ListStyle, MoveMode, SelectionType,
    TextDirection, TextDocument, TextFormat,
};
use bastyde_text::{
    EditorTypographyDefaults, FontRegistrar, RichTextEngine, SharedTypesetter, WrapMode,
};

use self::paint::{PaintParams, paint_frame};
use self::state::{EditorState, SharedState};
use crate::common::scroll::OverscrollBehavior;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVariant};
use crate::styles::RecipeRichTextEditorStyle;

/// Scroll bar visibility policy for [`RichTextEditor`], applied independently per axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollPolicy {
    /// Show the scroll bar only when content overflows the visible area (default).
    #[default]
    Auto,
    /// Always show the scroll bar, reserving gutter space even when content fits.
    AlwaysOn,
    /// Never show the scroll bar; useful when embedding the editor inside an outer
    /// `ScrollArea` or in headless tests.
    AlwaysOff,
}

/// The main rich text widget. Construct via [`RichTextEditor::read_only`]
/// (view/select only) or [`RichTextEditor::editor`] (full editing).
pub struct RichTextEditor {
    state: SharedState,
    v_scroll_policy: ScrollPolicy,
    h_scroll_policy: ScrollPolicy,
    /// Whether to install the built-in context-menu factory during
    /// `build()`. Defaults to `true`. Set `false` via
    /// [`default_context_menu`](Self::default_context_menu) to suppress
    /// the default entirely (right-click then bubbles past the widget;
    /// `context_target_at` stays available for apps that render their
    /// own menu).
    default_context_menu_enabled: bool,
    /// User-supplied context-menu factory (see
    /// [`context_menu`](Self::context_menu)). When set, it takes
    /// precedence over the default factory regardless of
    /// `default_context_menu_enabled`. Taken out (via `Option::take`)
    /// during `build()` because `Box<dyn Fn>` is not `Clone`.
    custom_context_menu: Option<
        Box<
            dyn Fn(
                bastyde_canvas::Point,
                &mut bastyde_core::widget::EventContext,
            ) -> Option<Box<dyn bastyde_core::widget::Widget>>,
        >,
    >,
    /// Minimum visible-text height expressed in lines. When set,
    /// switches `size_that_fits` from greedy (consume the proposal)
    /// to **intrinsic** sizing — see [`min_lines`](Self::min_lines).
    min_lines: Option<u32>,
    /// Maximum visible-text height expressed in lines. Hard-caps
    /// the intrinsic height — see [`max_lines`](Self::max_lines).
    max_lines: Option<u32>,
    /// Per-call style override for the chrome (border, padding, focus
    /// ring). Replaces the theme-wide `style_slots.rich_text_editor`
    /// and the default [`RecipeRichTextEditorStyle`] for just this
    /// editor.
    style_override: Option<SharedRichTextEditorStyle>,
    /// Root of the composed subtree returned by
    /// [`RichTextEditorStyle::make_body`]. Cached so layout queries
    /// route through the chrome without re-running the style call.
    root_child_id: Option<WidgetId>,
    /// Vertical scrollbar child id. `None` when
    /// `v_scroll_policy == ScrollPolicy::AlwaysOff` — in that case
    /// the scrollbar isn't even instantiated.
    v_scrollbar_id: Option<WidgetId>,
    /// Horizontal scrollbar child id. `None` when
    /// `h_scroll_policy == ScrollPolicy::AlwaysOff`.
    h_scrollbar_id: Option<WidgetId>,
    /// Scrollbar window-space bounds, written by `place_children` and
    /// read by the wrapper's `on_pointer_event` handler. Used to bail
    /// out of the drag-select latch when the press lands over an
    /// overlay scrollbar — without this guard the preview-pass pointer
    /// handler on the wrapper runs *before* the scrollbar (its child)
    /// gets the event, sets `drag_state = Selecting` on text under the
    /// overlay, and then steals every subsequent `PointerMove` with
    /// `EventResponse::Handled`, so the scrollbar's gesture arena
    /// never sees the drag.
    v_scrollbar_bounds: Rc<Cell<Rect>>,
    h_scrollbar_bounds: Rc<Cell<Rect>>,
    /// Per-edge `(top, right, bottom, left)` padding between the text
    /// content and the chrome. `None` lets the style apply its own
    /// default (TextInput-style insets for editable, no padding for
    /// read-only). Set via [`content_padding`](Self::content_padding) /
    /// [`content_padding_symmetric`](Self::content_padding_symmetric) /
    /// [`content_padding_each`](Self::content_padding_each).
    content_padding: Option<(f32, f32, f32, f32)>,
    /// Wheel scroll-chaining behavior at the editor's scroll boundary.
    /// [`OverscrollBehavior::Chain`] (the default) declines a wheel event the
    /// editor can no longer absorb so it bubbles to an ancestor scrollable —
    /// the editor embedded in a scrolling form/page hands the leftover scroll
    /// to the page. [`OverscrollBehavior::Contain`] absorbs the event at the
    /// boundary instead. Mirrors the identical knob on `ScrollArea` /
    /// `ListView` / `TableView` / `GridView`. See
    /// [`overscroll_behavior`](Self::overscroll_behavior).
    overscroll_behavior: OverscrollBehavior,
}

impl std::fmt::Debug for RichTextEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichTextEditor")
            .field("policy", &self.state.borrow().policy)
            .finish_non_exhaustive()
    }
}

impl RichTextEditor {
    /// Construct a read-only rich text viewer bound to `document`. The
    /// document can also back an editable `RichTextEditor::editor` in
    /// another part of the UI — both widgets receive document events
    /// independently via `on_change` subscriptions.
    pub fn read_only(document: TextDocument) -> Self {
        // A viewer defaults to *bare*: it can mirror the same shared document
        // as an editor pane, but stays free of the document's search / spell /
        // syntax highlighting (those are authoring affordances). Opt back in
        // with `.show_highlights(true)` — e.g. a read-only code viewer that
        // *wants* syntax coloring.
        Self::construct(document, READ_ONLY_PRESET).show_highlights(false)
    }

    /// Construct an editable rich text editor bound to `document`.
    /// Uses the full editor preset: every command accepted, caret
    /// blinks, `MultilineTextInput` accessibility role, full clipboard
    /// support. Multiple editors on the same document share live edits
    /// via per-widget `on_change` subscriptions.
    pub fn editor(document: TextDocument) -> Self {
        Self::construct(document, EDITOR_PRESET)
    }

    fn construct(document: TextDocument, policy: PolicyBundle) -> Self {
        // Start with a private engine. `build()` swaps it for one that
        // shares the application's `SharedTypesetter` when one is
        // reachable via `ctx.app_state`, so rendered glyphs land in
        // the atlas that bastyde-render actually uploads to the GPU.
        // Outside a windowed bastyde-app (headless tests) the private
        // engine is correct: no renderer is ever invoked.
        let mut engine = RichTextEngine::private_default();
        engine.set_wrap_mode(WrapMode::Word);
        // Prose editor: hyphenate justified paragraphs. Single-line / label
        // widgets (e.g. TextInputField) deliberately don't enable this.
        engine.set_hyphenate_justified(true);
        let state = EditorState::new(document, engine, policy, WrapMode::Word);
        Self {
            state,
            v_scroll_policy: ScrollPolicy::Auto,
            h_scroll_policy: ScrollPolicy::Auto,
            default_context_menu_enabled: true,
            custom_context_menu: None,
            min_lines: None,
            max_lines: None,
            style_override: None,
            root_child_id: None,
            v_scrollbar_id: None,
            h_scrollbar_id: None,
            v_scrollbar_bounds: Rc::new(Cell::new(Rect::ZERO)),
            h_scrollbar_bounds: Rc::new(Cell::new(Rect::ZERO)),
            content_padding: None,
            overscroll_behavior: OverscrollBehavior::default(),
        }
    }

    /// Per-call style override for the editor chrome (border, padding,
    /// focus ring). Replaces the theme-wide
    /// `style_slots.rich_text_editor` and the IntUI default
    /// `RecipeRichTextEditorStyle` for just this editor.
    pub fn style(mut self, style: impl RichTextEditorStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Set a uniform padding (logical pixels) between the text content
    /// and the editor's chrome. Replaces the style's default insets
    /// (TextInput-style for editable, none for read-only). Use
    /// [`content_padding_symmetric`](Self::content_padding_symmetric) or
    /// [`content_padding_each`](Self::content_padding_each) for
    /// per-axis / per-edge control.
    pub fn content_padding(mut self, amount: f32) -> Self {
        self.content_padding = Some((amount, amount, amount, amount));
        self
    }

    /// Set vertical and horizontal padding (logical pixels) between the
    /// text content and the editor's chrome. Replaces the style's
    /// default insets.
    pub fn content_padding_symmetric(mut self, vertical: f32, horizontal: f32) -> Self {
        self.content_padding = Some((vertical, horizontal, vertical, horizontal));
        self
    }

    /// Set per-edge padding `(top, right, bottom, left)` between the
    /// text content and the editor's chrome. Replaces the style's
    /// default insets.
    pub fn content_padding_each(mut self, top: f32, right: f32, bottom: f32, left: f32) -> Self {
        self.content_padding = Some((top, right, bottom, left));
        self
    }

    /// Set just the top inset between the text and the chrome. Leaves
    /// the other edges at their previously-set values, defaulting to
    /// `0.0` for any edge never touched.
    pub fn content_padding_top(mut self, top: f32) -> Self {
        let (_, r, b, l) = self.content_padding.unwrap_or((0.0, 0.0, 0.0, 0.0));
        self.content_padding = Some((top, r, b, l));
        self
    }

    /// Set just the right inset between the text and the chrome.
    pub fn content_padding_right(mut self, right: f32) -> Self {
        let (t, _, b, l) = self.content_padding.unwrap_or((0.0, 0.0, 0.0, 0.0));
        self.content_padding = Some((t, right, b, l));
        self
    }

    /// Set just the bottom inset between the text and the chrome.
    pub fn content_padding_bottom(mut self, bottom: f32) -> Self {
        let (t, r, _, l) = self.content_padding.unwrap_or((0.0, 0.0, 0.0, 0.0));
        self.content_padding = Some((t, r, bottom, l));
        self
    }

    /// Set just the left inset between the text and the chrome.
    pub fn content_padding_left(mut self, left: f32) -> Self {
        let (t, r, b, _) = self.content_padding.unwrap_or((0.0, 0.0, 0.0, 0.0));
        self.content_padding = Some((t, r, b, left));
        self
    }

    // --- Builder methods ------------------------------------------------

    /// Set the line-wrap mode. `WrapMode::Word` (the default) wraps at word
    /// boundaries; `WrapMode::None` allows horizontal overflow — pair with
    /// `.h_scroll_policy(ScrollPolicy::Auto)` to expose a scroll bar.
    pub fn wrap_mode(self, mode: WrapMode) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.wrap_mode = mode;
            st.engine.set_wrap_mode(mode);
            st.needs_full_layout = true;
        }
        self
    }

    /// Whether this view applies the document's syntax / search / spell
    /// highlighting. `editor` defaults to `true`; `read_only` defaults to
    /// `false` (a bare preview). A highlights-off view pulls a *clean*
    /// snapshot (no highlights at all, even metric ones like keyword bold) and
    /// ignores paint-only highlight events entirely, so it does zero work when
    /// the shared document's search/spell highlights change.
    pub fn show_highlights(self, show: bool) -> Self {
        {
            let mut st = self.state.borrow_mut();
            if st.show_highlights != show {
                st.show_highlights = show;
                // Re-pull the snapshot in the new flavor on the next tick.
                st.needs_full_layout = true;
            }
        }
        self
    }

    /// Set which highlight sessions **this view** renders, at runtime.
    ///
    /// [`HighlightMask::all`](bastyde_text::text_document::HighlightMask::all) shows every
    /// session on the document (the default);
    /// [`HighlightMask::only`](bastyde_text::text_document::HighlightMask::only) shows a
    /// chosen set — which is how a per-editor find banner
    /// keeps one pane's find highlighting out of another pane over the same document.
    /// `show_highlights(false)` still overrides this to nothing.
    ///
    /// Forces a re-pull on the next tick so the change is visible immediately.
    pub fn set_highlight_mask(&self, mask: bastyde_text::text_document::HighlightMask) {
        let mut st = self.state.borrow_mut();
        if st.highlight_mask != mask {
            st.highlight_mask = mask;
            st.needs_full_layout = true;
            // A mask change fires no document event, so the AT-cache invalidation the
            // event path does won't run — do it here. Dropping a metric session (syntax
            // bold) out of this view changes what the AT tree should report, and a stale
            // cached tree would keep announcing formatting the pane no longer draws.
            st.invalidate_accessibility_cache();
        }
    }

    /// Set the initial non-destructive default typography (font family / line
    /// height / first-line indent) applied to runs and blocks that carry no
    /// explicit override. Applied before the first layout. These are display
    /// defaults — they never mutate the bound document (no undo entry, no
    /// `modified`); use [`set_typography_defaults`](Self::set_typography_defaults)
    /// or [`EditorHandle::set_typography_defaults`] to change them after mount.
    /// Preferred text size is [`font_size_scale`](Self::font_size_scale).
    pub fn typography_defaults(self, defaults: EditorTypographyDefaults) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.engine.set_typography_defaults(defaults);
            st.needs_full_layout = true;
        }
        self
    }

    /// Override the editor background fill. Accepts a `Color`, a theme role
    /// (`SurfaceRole::Content`, …), or a `Signal`. Threaded into the active
    /// [`RichTextEditorStyle`]'s `make_body`, so the common case ("give the
    /// editor a surface") needs no custom style. `None` uses the style's
    /// default surface.
    pub fn background(self, color: impl Into<ColorProp>) -> Self {
        self.state.borrow_mut().background_prop = Some(color.into());
        self
    }

    /// Override the selection-highlight color. Accepts a `Color`, theme role,
    /// or `Signal`. Resolved against the active theme on every paint; `None`
    /// uses the engine/theme default.
    pub fn selection_color(self, color: impl Into<ColorProp>) -> Self {
        self.state.borrow_mut().selection_color_prop = Some(color.into());
        self
    }

    /// Override the caret / insertion-point color. Accepts a `Color`, theme
    /// role, or `Signal`. Resolved against the active theme on every paint;
    /// `None` tracks the theme's `editor_caret` role.
    pub fn caret_color(self, color: impl Into<ColorProp>) -> Self {
        self.state.borrow_mut().caret_color_prop = Some(color.into());
        self
    }

    /// Override the default text color. Accepts a `Color`, theme role, or
    /// `Signal`. Resolved against the active theme on every paint; `None`
    /// tracks the theme's `editor_fg` role (so dark / light swaps follow
    /// automatically). A role or `Signal` stays reactive; a bare `Color` pins
    /// it.
    pub fn text_color(self, color: impl Into<ColorProp>) -> Self {
        self.state.borrow_mut().text_color_prop = Some(color.into());
        self
    }

    /// Set the vertical scroll-bar visibility policy.
    pub fn v_scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.v_scroll_policy = policy;
        self
    }

    /// Set the horizontal scroll-bar visibility policy.
    pub fn h_scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.h_scroll_policy = policy;
        self
    }

    /// Window paint-time culling to the accumulated ancestor clip rather than
    /// this editor's own bounds.
    ///
    /// Enable this **only** for an editor deliberately laid out at its full
    /// document height inside an outer [`ScrollArea`](crate::ScrollArea)
    /// (`v_scroll_policy(ScrollPolicy::AlwaysOff)`, no `max_lines`) — "dubious
    /// mode". Such an editor's own viewport spans the whole document, so the
    /// viewport-derived render cull keeps nothing; this makes it cull to the
    /// visible clip band instead, so a huge document only rasterizes the rows on
    /// screen. Correct under nested ScrollAreas (the clip is the intersection of
    /// all clipping ancestors), and positioning / hit-testing are unaffected.
    ///
    /// A normal self-scrolling editor already culls correctly from its own scroll
    /// offset and doesn't need this — leave it **off** (the default). (The window
    /// is computed relative to the editor's own scroll offset as well, so enabling
    /// it on a self-scroller degrades to a correct-but-redundant cull rather than
    /// rendering the wrong rows.)
    pub fn window_to_clip(self, on: bool) -> Self {
        self.state.borrow_mut().window_to_clip = on;
        self
    }

    /// Set the same scroll-bar visibility policy on both axes.
    pub fn scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.v_scroll_policy = policy;
        self.h_scroll_policy = policy;
        self
    }

    /// Whether moving the caret also scrolls any *enclosing* scroll area to
    /// keep the caret on screen — the standard editor "caret stays visible as
    /// you type / navigate" behaviour. **On by default.**
    ///
    /// It fires only on a caret *move*, never on a plain wheel / scrollbar
    /// scroll, so the reader can still scroll freely away from the caret and the
    /// view holds until the caret next moves. This is what makes an editor that
    /// **grows** to its content with its own scroll suppressed (a flowing page
    /// inside an outer `ScrollArea`) track the caret at all — there the editor's
    /// internal caret-visibility is a no-op, so the enclosing-page follow is the
    /// only mechanism that reveals the caret. Pass `false` for the rare layout
    /// where a caret change must never move the surrounding page.
    pub fn follow_caret_in_page(self, follow: bool) -> Self {
        self.state.borrow_mut().follow_caret_in_page = follow;
        self
    }

    /// **Typewriter scrolling**: pin the caret's line at `fraction` of the way
    /// down the enclosing scroll area — `0.0` at the top, `0.5` centred, `1.0`
    /// at the bottom — and let the document scroll under it. `None` (the
    /// default) leaves the ordinary minimal-reveal follow in charge.
    ///
    /// Unlike that follow, which only acts once the caret would leave the
    /// viewport, a pin re-asserts on every caret move, so the line being written
    /// holds a constant height on screen. The classic writing-app feature.
    ///
    /// Three behaviours come with it, each of them the consensus answer among
    /// the editors that ship this well:
    ///
    /// - **The pointer stands the pin down.** A click places the caret without
    ///   scrolling, and that position becomes the new resting place; a
    ///   drag-selection is never interrupted. The next keystroke resumes
    ///   pinning. Editors that re-centre on pointer input instead have open bugs
    ///   about the view fighting the mouse and about drag-selection becoming
    ///   unusable.
    /// - **The rendered row is pinned, not the paragraph.** Under soft wrap a
    ///   long paragraph spans several visual rows; pinning the logical line
    ///   would leave the caret far from the mark.
    /// - **Typing snaps, page jumps glide.** Animating a pin that updates on
    ///   every keystroke is what produces the "screen bouncing" complaint other
    ///   implementations attract.
    ///
    /// Requires [`follow_caret_in_page`](Self::follow_caret_in_page) (on by
    /// default). `fraction` is clamped to `0.0..=1.0`.
    ///
    /// Near the start of the document the pin gives way to the scroll range —
    /// the caret rides above its line until there is room — and near the end it
    /// would do the same, which is usually not what you want: pair this with
    /// `ScrollArea::scroll_past_end(1.0 - fraction)` so the last line can still
    /// reach the pin.
    ///
    /// Takes a plain value, like [`typography_defaults`](Self::typography_defaults);
    /// to follow a setting live, push changes onto the handle with
    /// [`EditorHandle::set_typewriter`].
    pub fn typewriter(self, anchor: Option<f32>) -> Self {
        self.state.borrow_mut().typewriter = anchor.map(|f| f.clamp(0.0, 1.0));
        self
    }

    /// Set the wheel scroll-chaining behavior at the editor's boundary
    /// (default [`OverscrollBehavior::Chain`]). With `Chain`, a wheel event the
    /// editor can no longer absorb (already at the top/bottom, or content that
    /// fits so there is nothing to scroll) is declined so it bubbles to an
    /// ancestor scrollable — an editor embedded in a scrolling form/page lets
    /// the page scroll once the editor reaches its edge.
    /// [`OverscrollBehavior::Contain`] keeps the event at the editor instead.
    /// Mirrors the identical knob on `ScrollArea` / `ListView` / `TableView` /
    /// `GridView`.
    pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.overscroll_behavior = behavior;
        self
    }

    /// Set a minimum height (in lines of text) for the editor's
    /// **intrinsic** size.
    ///
    /// Setting either `min_lines` or [`max_lines`](Self::max_lines)
    /// switches the editor from greedy sizing (consume the
    /// proposal) to intrinsic sizing: `size_that_fits` returns
    /// `clamp(content_height, min_lines × line_height, max_lines × line_height)`
    /// for the dimension the parent leaves unspecified. A parent
    /// like `VStack` proposes unbounded height to non-Expand
    /// children, so the editor lands at its intrinsic height —
    /// exactly the messenger-composer / chat-input pattern.
    ///
    /// A parent that *forces* the height (e.g. `FixedSize`) wins
    /// regardless. This is intentional and matches Bastyde's
    /// general layout discipline: parents always have the final
    /// say on the dimensions they pin.
    ///
    /// `min_lines` measures the *visible text area*, not the outer
    /// widget — `min_lines(1)` reports a height equal to one line
    /// of text at the typesetter's default font + size, even
    /// before the document has any content.
    pub fn min_lines(mut self, n: u32) -> Self {
        self.min_lines = Some(n);
        self
    }

    /// Set a maximum height (in lines of text) for the editor's
    /// intrinsic size. Past this cap the vertical scroll bar
    /// absorbs further content growth.
    ///
    /// See [`min_lines`](Self::min_lines) for the intrinsic-mode
    /// switch and the parent-proposal interaction. `max_lines`
    /// measures the visible text area, not the outer widget.
    pub fn max_lines(mut self, n: u32) -> Self {
        self.max_lines = Some(n);
        self
    }

    /// Whether this editor's text grows with the global accessibility text
    /// scale (`ctx.text_scale`). Defaults to `true` — like every other text
    /// surface, the editor magnifies when the user raises the app-wide text
    /// size. Pass `false` for an editor whose font sizes are **document
    /// content** (a WYSIWYG / print-layout editor) that must stay at its true
    /// point size regardless of the reader's UI accessibility setting.
    ///
    /// Composed with [`font_size_scale`](Self::font_size_scale):  
    /// `engine.font_scale = (follow ? text_scale : 1.0) × font_size_scale`.
    pub fn follow_text_scale(self, follow: bool) -> Self {
        self.state.borrow_mut().follow_text_scale = follow;
        self
    }

    /// Per-editor logical font-size multiplier (`1.0` = 100 %). Applied
    /// *before* shaping (same channel as accessibility text scale), so text
    /// grows, re-wraps, and stays sharp — the knob for a "Text size"
    /// preference. Composed as
    /// `(follow_text_scale ? ctx.text_scale : 1.0) × font_size_scale`.
    /// Clamped to `[0.1, 10.0]`. Use [`set_font_size_scale`](Self::set_font_size_scale)
    /// after mount.
    pub fn font_size_scale(self, scale: f32) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.font_size_scale = scale.clamp(0.1, 10.0);
            st.needs_full_layout = true;
            st.content_dirty = true;
        }
        self
    }

    /// Replace the built-in right-click context menu with a
    /// user-provided factory. Same shape as the framework's
    /// [`bastyde_core::widget_builder::ContextMenuFactory`]: the
    /// closure receives the click position (widget-local) and a full
    /// [`EventContext`](bastyde_core::widget::EventContext), and returns
    /// `Some(menu_widget)` to mount or `None` to decline (falling
    /// through to the next ancestor with a factory).
    ///
    /// Taking this branch disables the default menu unconditionally.
    /// The framework's
    /// [`show_context_menu_for`](bastyde_core::widget_tree) handles
    /// the overlay lifecycle (open at pointer, dismiss on
    /// click-outside / Escape, focus-restore on dismiss), so the
    /// factory only needs to build the menu content.
    ///
    /// This is an **inherent method**: it shadows the blanket
    /// [`WidgetBuilder::context_menu`](bastyde_core::widget_builder::WidgetBuilder::context_menu)
    /// trait method so the user can chain it directly on the editor.
    /// Internally, the factory is installed on the editor's arena
    /// node via the same `HandlerSet::context_menu` plumbing.
    pub fn context_menu(
        mut self,
        factory: impl Fn(
            bastyde_canvas::Point,
            &mut bastyde_core::widget::EventContext,
        ) -> Option<Box<dyn bastyde_core::widget::Widget>>
        + 'static,
    ) -> Self {
        self.custom_context_menu = Some(Box::new(factory));
        self
    }

    /// Enable (default) or disable the widget's built-in right-click
    /// context menu (Cut / Copy / Paste / Paste Unformatted / Select
    /// All). When disabled, right-click bubbles past the widget
    /// unhandled and
    /// [`context_target_at`](Self::context_target_at) stays
    /// available for applications that render their own menu.
    ///
    /// Note: if a user factory is installed via
    /// [`context_menu`](Self::context_menu), that factory wins
    /// regardless of this flag — this setter only governs the
    /// *default* menu.
    pub fn default_context_menu(mut self, enabled: bool) -> Self {
        self.default_context_menu_enabled = enabled;
        self
    }

    /// Install a custom font registrar for the fallback private
    /// engine. Only has effect when the editor is built outside a
    /// windowed bastyde-app — once `build()` sees a `SharedTypesetter`
    /// in `app_state`, the private engine is replaced with one that
    /// shares the app's typesetter and this registrar is ignored.
    pub fn font_registrar(self, registrar: &dyn FontRegistrar) -> Self {
        {
            let mut st = self.state.borrow_mut();
            let mut engine = RichTextEngine::private_with_registrar(registrar);
            engine.set_wrap_mode(st.wrap_mode);
            engine.set_hyphenate_justified(true);
            st.engine = engine;
            st.needs_full_layout = true;
        }
        self
    }

    /// Install a callback fired once per batch of genuine **user content
    /// edits** (typing, paste, cut, delete) — and *not* on a programmatic
    /// `set_djot` / `set_markdown` / `set_html` load or a document reset, and
    /// *not* while an IME composition (CJK/Kana candidate preview, dead-key
    /// accent) is still in progress — only the settled result of a commit
    /// fires it. The callback runs on the UI thread during the editor's frame
    /// drain, so it may touch `Signal`s directly — e.g. flip a "dirty" flag or
    /// kick a debounced autosave. Replaces any prior change callback on this
    /// editor.
    ///
    /// For a reactive change *token* (which also bumps on loads/format-only
    /// changes, and on intermediate IME composition steps), observe
    /// [`document_version`](Self::document_version) instead.
    pub fn on_change(self, f: impl Fn() + 'static) -> Self {
        self.state.borrow_mut().on_change = Some(Rc::new(f));
        self
    }

    // --- Observable signals ---------------------------------------------

    /// Reactive counter that bumps on every document change (content edits,
    /// format changes, load events). Starts at `0`. Use as a change token to
    /// invalidate external caches.
    pub fn document_version(&self) -> Signal<u64> {
        self.state.borrow().document_version.clone()
    }

    /// Current cursor position in the document, in character units.
    /// Exposed for tests and for applications that need to mirror the
    /// caret position externally (status bar, outline panel, etc.).
    pub fn cursor_position(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    /// Current selection anchor (equal to `cursor_position` when there
    /// is no selection).
    pub fn cursor_anchor(&self) -> usize {
        self.state.borrow().cursor.anchor()
    }

    /// `true` while an IME composition (CJK/Kana candidate preview, dead-key
    /// accent) is actively in progress — i.e. [`on_change`](Self::on_change)
    /// is currently suppressed for this editor. Exposed so a caller doing its
    /// own while-typing scanning (e.g. an autocorrect feature) can gate its
    /// own trigger logic the same way, as defense-in-depth alongside
    /// `on_change`'s own gate.
    pub fn is_composing(&self) -> bool {
        self.state.borrow().ime_preedit.is_some()
    }

    /// Reactive cursor position signal. Observers fire whenever the
    /// cursor moves (arrow keys, click, Home/End, …). Useful for
    /// status bars and tests.
    pub fn cursor_position_signal(&self) -> Signal<usize> {
        self.state.borrow().cursor_position.clone()
    }

    /// Reactive selection anchor signal.
    pub fn cursor_anchor_signal(&self) -> Signal<usize> {
        self.state.borrow().cursor_anchor.clone()
    }

    /// Reactive signal — `true` whenever the editor has a non-empty
    /// selection. Updates synchronously after every cursor mutation.
    pub fn has_selection(&self) -> Signal<bool> {
        self.state.borrow().has_selection.clone()
    }

    /// Reactive undo-availability signal, suitable for toolbar button
    /// enable-state. Updated through the frame loop's debounce drain
    /// so toolbars don't flicker during rapid editing.
    pub fn can_undo(&self) -> Signal<bool> {
        self.state.borrow().can_undo.clone()
    }

    /// Reactive redo-availability signal.
    pub fn can_redo(&self) -> Signal<bool> {
        self.state.borrow().can_redo.clone()
    }

    /// Read the current character format at the widget's caret —
    /// the right source for toolbars that mirror bold/italic/underline
    /// state.
    ///
    /// When a selection is active, the format is read from
    /// [`selection_start()`](bastyde_text::text_document::TextCursor::selection_start)
    /// rather than [`position()`](bastyde_text::text_document::TextCursor::position).
    /// Rationale (matches godot-rich-text's `query_char_format`):
    /// `position()` lands at the **end** of the selection and may fall
    /// on a run with different formatting (or past the last character,
    /// on an empty virtual element) — a toolbar observing that value
    /// would flicker or lie. `selection_start()` always points at the
    /// first character of the selected range, so the reading is
    /// stable and matches what a user would expect from "tell me the
    /// format of what I have selected."
    pub fn caret_char_format(&self) -> TextFormat {
        let st = self.state.borrow();
        let probe_pos = if st.cursor.has_selection() {
            st.cursor.selection_start()
        } else {
            st.cursor.position()
        };
        // Read through a fresh cursor so we don't disturb the widget's
        // own cursor (the widget's own cursor has its own position /
        // anchor state that we must not move).
        let probe = st.document.cursor();
        probe.set_position(probe_pos, bastyde_text::text_document::MoveMode::MoveAnchor);
        probe.char_format().unwrap_or_default()
    }

    /// Clone the internal shared state handle for test observation.
    /// Tests take this before `tree.add(editor)` moves the widget
    /// into the arena, so they can read the widget's live cursor,
    /// signal state, and debounce fields through the very same
    /// `Rc<RefCell<EditorState>>` that the arena-stored editor is
    /// mutating.
    #[cfg(test)]
    #[doc(hidden)]
    pub(crate) fn state_handle(&self) -> SharedState {
        self.state.clone()
    }

    /// Reactive vertical scroll offset in logical pixels. Bind to a
    /// scroll bar or observe for scroll-position persistence.
    pub fn scroll_y(&self) -> Signal<f32> {
        self.state.borrow().scroll_y.clone()
    }

    /// Reactive horizontal scroll offset in logical pixels. Non-zero
    /// only when [`wrap_mode`](Self::wrap_mode) is `WrapMode::None`.
    pub fn scroll_x(&self) -> Signal<f32> {
        self.state.borrow().scroll_x.clone()
    }

    // --- Context-menu support (external menus) --------------------------

    /// Classify what is under `point` in the widget's local coordinates
    /// (origin at the widget's top-left, scroll offset handled
    /// internally by the typesetter), for applications building an
    /// external context menu. Returns `None` if the point does not
    /// land on any hit region.
    pub fn context_target_at(&self, point: Point) -> Option<hit_test::ContextTarget> {
        let st = self.state.borrow();
        let hit = hit_test::hit_test_at(&st.engine, point, 0.0, 0.0)?;
        let selection = Some((st.cursor.anchor(), st.cursor.position()));
        Some(hit_test::classify(&hit, selection, &st.document))
    }

    // --- Selection helpers (allowed under both presets) -----------------

    /// Currently selected text, or an empty string if nothing is selected.
    pub fn selected_text(&self) -> String {
        self.state
            .borrow()
            .cursor
            .selected_text()
            .unwrap_or_default()
    }

    /// Select the entire document programmatically. Equivalent to
    /// the final step of the Ctrl+A ladder; resets the ladder state
    /// so a subsequent Ctrl+A starts fresh at level 1.
    pub fn select_all(&self) {
        {
            let mut st = self.state.borrow_mut();
            st.cursor.select(SelectionType::Document);
            st.select_all_level = 0;
            st.select_all_anchor_cell = None;
        }
        sync_cursor_signals(&self.state);
    }

    /// Clear any current selection.
    pub fn deselect(&self) {
        {
            let mut st = self.state.borrow_mut();
            st.cursor.clear_selection();
            st.select_all_level = 0;
            st.select_all_anchor_cell = None;
        }
        sync_cursor_signals(&self.state);
    }

    // --- Cursor mirror API -------------------------------------------------
    //
    // These mirror the corresponding `TextCursor` methods but act on the
    // widget's **internal** cursor (the one tied to caret rendering /
    // blink / focus) rather than a fresh `doc.cursor()`. An application
    // that reaches through `TextDocument::cursor()` gets an independent
    // cursor whose position is decoupled from the widget's caret — any
    // mutation would be invisible to the paint pass. Use these methods
    // when you want programmatic effects to feel like user-typed edits.

    /// Insert plain text at the widget's caret. Replaces any selection.
    pub fn insert_text(&self, text: &str) {
        let st = self.state.borrow();
        let _ = st.cursor.insert_text(text);
        drop(st);
        sync_cursor_signals(&self.state);
    }

    /// Insert a fragment parsed from HTML at the widget's caret.
    /// Replaces any selection. Uses text-document's
    /// [`TextCursor::insert_html`](bastyde_text::text_document::TextCursor::insert_html),
    /// which parses the HTML into a `DocumentFragment` and inserts it.
    pub fn insert_html(&self, html: &str) {
        let st = self.state.borrow();
        let _ = st.cursor.insert_html(html);
        drop(st);
        sync_cursor_signals(&self.state);
    }

    /// Insert a fragment parsed from djot at the widget's caret.
    /// Replaces any selection. Uses text-document's
    /// [`TextCursor::insert_djot`](bastyde_text::text_document::TextCursor::insert_djot),
    /// which parses the djot into a `DocumentFragment` and inserts it — so
    /// unlike [`insert_text`](Self::insert_text), block-level source really
    /// does produce new blocks rather than literal newlines in one paragraph.
    pub fn insert_djot(&self, djot: &str) {
        let st = self.state.borrow();
        let _ = st.cursor.insert_djot(djot);
        drop(st);
        sync_cursor_signals(&self.state);
    }

    /// Split the current block at the widget's caret, as pressing Enter does.
    pub fn insert_block(&self) {
        let st = self.state.borrow();
        let _ = st.cursor.insert_block();
        drop(st);
        sync_cursor_signals(&self.state);
    }

    /// Insert an inline image by logical resource name. `width` and
    /// `height` are in logical pixels.
    pub fn insert_image(&self, name: &str, width: u32, height: u32) {
        let st = self.state.borrow();
        let _ = st.cursor.insert_image(name, width, height);
        drop(st);
        sync_cursor_signals(&self.state);
    }

    /// Delete the current selection. No-op when nothing is selected.
    pub fn delete_selection(&self) {
        let st = self.state.borrow();
        if st.cursor.has_selection() {
            let _ = st.cursor.remove_selected_text();
        }
        drop(st);
        sync_cursor_signals(&self.state);
    }

    /// Select the word under the widget's caret.
    pub fn select_word(&self) {
        {
            let st = self.state.borrow();
            st.cursor.select(SelectionType::WordUnderCursor);
        }
        sync_cursor_signals(&self.state);
    }

    /// Select the paragraph / block under the widget's caret.
    pub fn select_line(&self) {
        {
            let st = self.state.borrow();
            st.cursor.select(SelectionType::LineUnderCursor);
        }
        sync_cursor_signals(&self.state);
    }

    /// Move the caret to an absolute character position. Collapses any
    /// existing selection (passes [`MoveMode::MoveAnchor`]). Resets
    /// `CursorAffinity` to `Downstream` — programmatic placement
    /// can't know whether the caller wanted the upstream side of a
    /// wrap boundary, so we default to the same placement that
    /// existed before affinity was introduced.
    pub fn set_caret_position(&self, position: usize) {
        {
            let mut st = self.state.borrow_mut();
            st.cursor.set_position(position, MoveMode::MoveAnchor);
            st.cursor_affinity = bastyde_text::CursorAffinity::Downstream;
        }
        sync_cursor_signals(&self.state);
    }

    // --- Search / find-banner support (B3) --------------------------------

    /// Reactive signal — `true` while **this** editor holds keyboard focus.
    ///
    /// A per-editor find banner (Ctrl+F) targets whichever editor is focused, and the split
    /// view has two of them; `focused_side` only names the Primary/Secondary *pane*, not which
    /// editor. This is the per-editor answer, mirroring [`has_selection`](Self::has_selection).
    pub fn focused_signal(&self) -> Signal<bool> {
        self.state.borrow().focus_signal.clone()
    }

    /// Select the character range `[start, end)`, **without** collapsing — unlike
    /// [`set_caret_position`](Self::set_caret_position), which always moves both ends together.
    ///
    /// The anchor lands at `start` and the caret (focus) at `end`, so the standard selection
    /// highlight marks the range and a subsequent replace acts on it. Used to select a search
    /// match. (The non-collapsing two-call shape is the same one the AccessKit
    /// `SetTextSelection` handler uses.)
    pub fn select_range(&self, start: usize, end: usize) {
        {
            let mut st = self.state.borrow_mut();
            st.cursor.set_position(start, MoveMode::MoveAnchor);
            st.cursor.set_position(end, MoveMode::KeepAnchor);
            // The caret sits at `end`; downstream affinity matches placement at a range end.
            st.cursor_affinity = bastyde_text::CursorAffinity::Downstream;
        }
        sync_cursor_signals(&self.state);
    }

    /// Scroll the character range `[start, end)` into view within the enclosing scroll area.
    ///
    /// Reveals an **arbitrary** offset range — the current search match — rather than the live
    /// caret the follow-into-view path tracks, and works whether or not the editor is focused.
    /// A no-op until the editor has a full layout.
    ///
    /// Under [`typewriter`](Self::typewriter) scrolling the range is *pinned* to
    /// the anchor rather than merely revealed, so a search walks matches to the
    /// same height the caret writes at instead of leaving them wherever they
    /// happened to fall. Because a search jump is a deliberate, screen-sized
    /// move, it glides.
    pub fn reveal_range(
        &self,
        ctx: &mut bastyde_core::widget::EventContext,
        start: usize,
        end: usize,
    ) {
        reveal_range_impl(&self.state, ctx, start, end);
    }

    // --- Character-format commands ----------------------------------------
    //
    // Each setter writes to `TextCursor::merge_char_format`, which
    // applies to the current selection (or acts as a typing format when
    // there is no selection — see text-document's semantics). Toggle
    // variants (`toggle_bold`, `toggle_italic`, `toggle_underline`,
    // `toggle_strikethrough`) read the current state via
    // [`caret_char_format`](Self::caret_char_format) first and flip,
    // which matches the Ctrl+B / Ctrl+I / Ctrl+U keyboard shortcuts.

    fn apply_char_format(&self, fmt: TextFormat) {
        let st = self.state.borrow();
        let _ = st.cursor.merge_char_format(&fmt);
        // `pending_format_changed` gets set by `drain_events` when the
        // document emits its `FormatChanged` event in response to the
        // cursor mutation, so no manual bookkeeping is needed here.
    }

    /// Apply **bold** to the current selection (or set the typing bold
    /// state when no selection is active). Pairs with
    /// [`is_bold`](Self::is_bold) and [`toggle_bold`](Self::toggle_bold).
    pub fn set_bold(&self, enabled: bool) {
        self.apply_char_format(TextFormat {
            font_bold: Some(enabled),
            ..Default::default()
        });
    }

    /// Apply *italic* to the current selection.
    pub fn set_italic(&self, enabled: bool) {
        self.apply_char_format(TextFormat {
            font_italic: Some(enabled),
            ..Default::default()
        });
    }

    /// Apply underline to the current selection.
    pub fn set_underline(&self, enabled: bool) {
        self.apply_char_format(TextFormat {
            font_underline: Some(enabled),
            ..Default::default()
        });
    }

    /// Apply strikethrough to the current selection.
    pub fn set_strikethrough(&self, enabled: bool) {
        self.apply_char_format(TextFormat {
            font_strikeout: Some(enabled),
            ..Default::default()
        });
    }

    /// Set the font size (in points) for the current selection.
    pub fn set_font_size(&self, size: u32) {
        self.apply_char_format(TextFormat {
            font_point_size: Some(size),
            ..Default::default()
        });
    }

    /// Set the font family for the current selection. `family` must be
    /// a name resolvable by the shared typesetter's font registrar.
    pub fn set_font_family(&self, family: impl Into<String>) {
        self.apply_char_format(TextFormat {
            font_family: Some(family.into()),
            ..Default::default()
        });
    }

    /// Toggle bold on the current selection, reading the current state
    /// via [`caret_char_format`](Self::caret_char_format). Matches the
    /// Ctrl+B keyboard shortcut's behaviour.
    pub fn toggle_bold(&self) {
        let current = self.caret_char_format().font_bold.unwrap_or(false);
        self.set_bold(!current);
    }

    /// Toggle italic; see [`toggle_bold`](Self::toggle_bold).
    pub fn toggle_italic(&self) {
        let current = self.caret_char_format().font_italic.unwrap_or(false);
        self.set_italic(!current);
    }

    /// Toggle underline; see [`toggle_bold`](Self::toggle_bold).
    pub fn toggle_underline(&self) {
        let current = self.caret_char_format().font_underline.unwrap_or(false);
        self.set_underline(!current);
    }

    /// Toggle strikethrough; see [`toggle_bold`](Self::toggle_bold).
    pub fn toggle_strikethrough(&self) {
        let current = self.caret_char_format().font_strikeout.unwrap_or(false);
        self.set_strikethrough(!current);
    }

    // --- Vertical alignment (super / subscript) ---------------------------
    //
    // One property with three meaningful states, surfaced as two independent
    // toggles because that is how a toolbar presents it. Setting one clears
    // the other, since a run cannot be both.

    /// Raise the selection to superscript, or drop it back to the baseline.
    pub fn set_superscript(&self, enabled: bool) {
        self.set_vertical_alignment(if enabled {
            CharVerticalAlignment::SuperScript
        } else {
            CharVerticalAlignment::Normal
        });
    }

    /// Lower the selection to subscript, or drop it back to the baseline.
    pub fn set_subscript(&self, enabled: bool) {
        self.set_vertical_alignment(if enabled {
            CharVerticalAlignment::SubScript
        } else {
            CharVerticalAlignment::Normal
        });
    }

    /// Set the selection's vertical alignment directly. `Normal` is the
    /// baseline; `Middle` exists in the model but has no toolbar affordance.
    pub fn set_vertical_alignment(&self, alignment: CharVerticalAlignment) {
        self.apply_char_format(TextFormat {
            vertical_alignment: Some(alignment),
            ..Default::default()
        });
    }

    /// The caret's vertical alignment, `Normal` when unset.
    pub fn get_vertical_alignment(&self) -> CharVerticalAlignment {
        self.caret_char_format()
            .vertical_alignment
            .unwrap_or(CharVerticalAlignment::Normal)
    }

    /// True while the caret sits in superscript text.
    pub fn is_superscript(&self) -> bool {
        self.get_vertical_alignment() == CharVerticalAlignment::SuperScript
    }

    /// True while the caret sits in subscript text.
    pub fn is_subscript(&self) -> bool {
        self.get_vertical_alignment() == CharVerticalAlignment::SubScript
    }

    /// Flip superscript on the selection. Turning it on replaces subscript.
    pub fn toggle_superscript(&self) {
        self.set_superscript(!self.is_superscript());
    }

    /// Flip subscript on the selection. Turning it on replaces superscript.
    pub fn toggle_subscript(&self) {
        self.set_subscript(!self.is_subscript());
    }

    // --- Block-format commands --------------------------------------------

    /// Set an arbitrary [`BlockFormat`] on the caret's current block.
    /// The higher-level helpers [`set_alignment`](Self::set_alignment)
    /// and [`set_heading_level`](Self::set_heading_level) go through
    /// this method. Exposed so apps that need less common fields
    /// (`indent`, `left_margin`, `line_height`, …) don't have to
    /// reach through `TextDocument::cursor()` and lose the widget's
    /// caret continuity.
    pub fn apply_block_format(&self, fmt: BlockFormat) {
        let st = self.state.borrow();
        let _ = st.cursor.set_block_format(&fmt);
        // See `apply_char_format` — `FormatChanged` propagates
        // through `drain_events` and updates `pending_format_changed`
        // + `format_version` there.
    }

    /// Set an arbitrary [`TextFormat`] on the current selection.
    /// Public counterpart of the private `apply_char_format` helper,
    /// for apps that need fields beyond the dedicated
    /// `set_bold` / `set_italic` / … setters (e.g. `letter_spacing`,
    /// `foreground_color`).
    pub fn apply_text_format(&self, fmt: TextFormat) {
        self.apply_char_format(fmt);
    }

    /// Set the paragraph alignment for the current block (or the block
    /// containing the selection anchor).
    pub fn set_alignment(&self, alignment: Alignment) {
        self.apply_block_format(BlockFormat {
            alignment: Some(alignment),
            ..Default::default()
        });
    }

    /// Unset the block's direction, handing the paragraph back to
    /// automatic detection.
    ///
    /// Not the same as setting left-to-right. An explicit direction
    /// *pins* the paragraph and overrides the bidi algorithm, so
    /// "clearing" a direction by writing `LeftToRight` would force
    /// Arabic and Hebrew prose to lay out backwards. Only an unset
    /// direction lets the text speak for itself.
    pub fn clear_direction(&self) {
        self.apply_block_format(BlockFormat {
            clear_direction: true,
            ..Default::default()
        });
    }

    /// Set the base reading direction of the current block.
    ///
    /// This is the *paragraph* direction, not a character property: it
    /// decides which edge unaligned text sits against and, more
    /// importantly, overrides the bidi algorithm's first-strong-character
    /// guess — which misreads an Arabic paragraph opening with a Latin
    /// acronym as left-to-right.
    pub fn set_direction(&self, direction: TextDirection) {
        self.apply_block_format(BlockFormat {
            direction: Some(direction),
            ..Default::default()
        });
    }

    /// Set the heading level of the current block. `0` = plain
    /// paragraph; `1..=6` follow the HTML `<h1>..<h6>` convention.
    pub fn set_heading_level(&self, level: u8) {
        self.apply_block_format(BlockFormat {
            heading_level: Some(level),
            ..Default::default()
        });
    }

    // --- List commands ----------------------------------------------------

    /// Create a list at the current selection. `ordered = true` uses
    /// decimal numbering; `ordered = false` uses a bullet disc.
    /// Choose a specific style with [`create_list`](Self::create_list).
    pub fn insert_list(&self, ordered: bool) {
        let style = if ordered {
            ListStyle::Decimal
        } else {
            ListStyle::Disc
        };
        self.create_list(style);
    }

    /// Create a list with an explicit [`ListStyle`]. Exposed for
    /// applications that want e.g. lowercase Roman numerals or circle
    /// bullets.
    pub fn create_list(&self, style: ListStyle) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.create_list(style);
        }
        sync_cursor_signals(&self.state);
    }

    /// Increase the nesting depth of the caret's current list item by
    /// one. No-op when the caret is not inside a list. Equivalent to
    /// pressing Tab while the caret is on a list item — same behaviour,
    /// same `nest_current_list_item` codepath, exposed for toolbar
    /// buttons that do not want to synthesise key events.
    pub fn indent(&self) {
        keyboard::indent_current_block(&mut self.state.borrow_mut());
        sync_cursor_signals(&self.state);
    }

    /// Decrease the nesting depth of the caret's current list item by
    /// one. No-op at depth 0 (use `Backspace` at block-start to exit
    /// the list entirely). Toolbar counterpart of Shift+Tab.
    pub fn outdent(&self) {
        keyboard::dedent_current_block(&mut self.state.borrow_mut());
        sync_cursor_signals(&self.state);
    }

    /// Take the caret's block out of its list entirely, leaving a plain
    /// paragraph. No-op when the caret is not inside a list.
    ///
    /// [`outdent`](Self::outdent) deliberately stops at depth 0 — Shift+Tab
    /// should not silently destroy the list — so a toolbar that offers
    /// "remove list formatting" needs this instead. Backspace at block-start
    /// reaches the same codepath from the keyboard.
    pub fn remove_from_list(&self) {
        let _ = self.state.borrow().cursor.remove_current_block_from_list();
        sync_cursor_signals(&self.state);
    }

    // --- Blockquote commands ----------------------------------------------

    /// True iff the caret currently sits inside a blockquote frame at
    /// any nesting depth. Used by the toolbar to drive the toggle
    /// button's pressed state and the context menu's label.
    pub fn is_in_blockquote(&self) -> bool {
        let st = self.state.borrow();
        st.cursor.is_in_blockquote()
    }

    /// True iff the current selection spans more than one frame. The
    /// "Toggle blockquote" affordance is disabled in this case because
    /// wrapping a cross-frame range has no well-defined semantics
    /// (different blocks already belong to different containers).
    pub fn selection_spans_multiple_frames(&self) -> bool {
        let st = self.state.borrow();
        st.cursor.selection_spans_multiple_frames()
    }

    /// Wrap the current block (or selection) in a blockquote, or
    /// unwrap the innermost enclosing blockquote if already inside one.
    /// No-op (returns silently) when the selection spans multiple
    /// frames.
    pub fn toggle_blockquote(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.toggle_blockquote();
        }
        sync_cursor_signals(&self.state);
    }

    /// Equivalent to pressing Tab inside a blockquote — wraps the
    /// current block in a deeper nested quote. No-op when the caret is
    /// not in a quote.
    pub fn increase_blockquote_depth(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.increase_blockquote_depth();
        }
        sync_cursor_signals(&self.state);
    }

    /// Equivalent to pressing Shift+Tab inside a blockquote — pops one
    /// nesting level. At depth 1 unwraps the block to a plain
    /// paragraph. No-op when the caret is not in a quote.
    pub fn decrease_blockquote_depth(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.decrease_blockquote_depth();
        }
        sync_cursor_signals(&self.state);
    }

    // --- Table commands ---------------------------------------------------
    //
    // Each table command drops through `sync_cursor_signals` because
    // the underlying `cursor.*` calls move the caret (insert_table
    // lands past the new table; row/column ops may shift the caret's
    // logical position). Callers observing `cursor_position_signal`
    // see the post-operation position without waiting for the next
    // frame tick.

    /// Insert a fresh `rows × columns` table at the caret. Any
    /// existing selection is replaced.
    pub fn insert_table(&self, rows: usize, columns: usize) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_table(rows, columns);
        }
        sync_cursor_signals(&self.state);
    }

    /// Remove the table containing the caret (if any). No-op when the
    /// caret is not inside a table.
    pub fn remove_current_table(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.remove_current_table();
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert a row above the caret's current table row. No-op when
    /// outside a table.
    pub fn insert_row_above(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_row_above();
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert a row below the caret's current table row.
    pub fn insert_row_below(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_row_below();
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert a column before the caret's current table column.
    pub fn insert_column_before(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_column_before();
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert a column after the caret's current table column.
    pub fn insert_column_after(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_column_after();
        }
        sync_cursor_signals(&self.state);
    }

    /// Remove the caret's current table row.
    pub fn remove_current_row(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.remove_current_row();
        }
        sync_cursor_signals(&self.state);
    }

    /// Remove the caret's current table column.
    pub fn remove_current_column(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.remove_current_column();
        }
        sync_cursor_signals(&self.state);
    }

    /// Whether the caret is currently inside a table cell.
    pub fn is_in_table(&self) -> bool {
        self.state.borrow().cursor.current_table().is_some()
    }

    // --- Format query methods (toolbar state) -----------------------------
    //
    // Every query goes through [`caret_char_format`](Self::caret_char_format)
    // which honours the selection-start rule — toolbar buttons reflect
    // "the format of what's selected," not "the format after the
    // selection ends."

    /// Whether the current selection / typing position is bold.
    pub fn is_bold(&self) -> bool {
        self.caret_char_format().font_bold.unwrap_or(false)
    }

    /// Whether italic.
    pub fn is_italic(&self) -> bool {
        self.caret_char_format().font_italic.unwrap_or(false)
    }

    /// Whether underline.
    pub fn is_underline(&self) -> bool {
        self.caret_char_format().font_underline.unwrap_or(false)
    }

    /// Whether strikethrough.
    pub fn is_strikethrough(&self) -> bool {
        self.caret_char_format().font_strikeout.unwrap_or(false)
    }

    /// Current heading level (0 = plain paragraph). Reads the caret's
    /// current block format.
    pub fn get_heading_level(&self) -> u8 {
        self.state
            .borrow()
            .cursor
            .block_format()
            .ok()
            .and_then(|f| f.heading_level)
            .unwrap_or(0)
    }

    /// Current block alignment.
    pub fn get_alignment(&self) -> Alignment {
        self.state
            .borrow()
            .cursor
            .block_format()
            .ok()
            .and_then(|f| f.alignment)
            .unwrap_or(Alignment::Left)
    }

    /// The block's explicitly-set reading direction, if it has one.
    /// `None` means the bidi algorithm decides from the text.
    pub fn get_direction(&self) -> Option<TextDirection> {
        self.state
            .borrow()
            .cursor
            .block_format()
            .ok()
            .and_then(|f| f.direction)
    }

    // --- History ---------------------------------------------------------
    //
    // Programmatic Undo / Redo. Failures (e.g. empty undo stack) are
    // silently discarded — toolbars gate the buttons on
    // [`can_undo`](Self::can_undo) / [`can_redo`](Self::can_redo)
    // signals so the error path is unreachable in normal use, and the
    // keyboard handlers at `keyboard.rs:357-366` use the same
    // `let _ =` discipline.

    /// Undo the most recent edit. Mirrors Ctrl+Z. No-op when the undo
    /// stack is empty.
    pub fn undo(&self) {
        let _ = self.state.borrow().document.undo();
        sync_cursor_signals(&self.state);
    }

    /// Redo the most recently undone edit. Mirrors Ctrl+Y /
    /// Ctrl+Shift+Z. No-op when the redo stack is empty.
    pub fn redo(&self) {
        let _ = self.state.borrow().document.redo();
        sync_cursor_signals(&self.state);
    }

    // --- Edit blocks (composite undo) ------------------------------------
    //
    // Every command on this type is its own transaction, so a caller that
    // composes several of them into one user-visible action — "clear
    // formatting" turning off four marks and flattening a heading — leaves
    // the user pressing Ctrl+Z once per property. Wrapping the sequence in
    // an edit block makes it one entry.
    //
    // The editor already groups this way internally for IME composition
    // (`keyboard.rs`) and for list nesting; these expose the same primitive
    // to external toolbars. Composites nest, so it is safe to wrap calls
    // that open one of their own.

    /// Begin grouping subsequent edits into a single undo entry.
    ///
    /// Must be paired with [`end_edit_block`](Self::end_edit_block). Prefer
    /// [`edit_block`](Self::edit_block), which pairs them for you.
    pub fn begin_edit_block(&self) {
        self.state.borrow().cursor.begin_edit_block();
    }

    /// Close the group opened by [`begin_edit_block`](Self::begin_edit_block).
    pub fn end_edit_block(&self) {
        self.state.borrow().cursor.end_edit_block();
    }

    /// Run `edits` as one undo entry.
    ///
    /// The scoped form of [`begin_edit_block`](Self::begin_edit_block) — the
    /// block is closed even if `edits` returns early, which hand-pairing gets
    /// wrong eventually.
    pub fn edit_block<R>(&self, edits: impl FnOnce() -> R) -> R {
        self.begin_edit_block();
        let result = edits();
        self.end_edit_block();
        result
    }

    /// Set the document-wide default language (ISO 639-1 code, e.g. "en",
    /// "fr", "de"). Blocks that don't set their own language inherit it
    /// for hyphenation. Forces a full re-layout so the change takes effect
    /// on the next frame. No-op-safe if the document rejects the update.
    pub fn set_default_language(&self, language: &str) {
        let _ = self.state.borrow().document.set_default_language(language);
        self.state.borrow_mut().needs_full_layout = true;
    }

    /// The document-wide default language (ISO 639-1 code). Defaults to
    /// `"en"` when never set.
    pub fn default_language(&self) -> String {
        self.state.borrow().document.default_language()
    }

    // --- External handle -------------------------------------------------

    /// Cheap clone-able handle for external toolbars / palettes — see
    /// [`EditorHandle`]. The handle shares the editor's internal
    /// state (same `Rc<RefCell<…>>`), so mutations through the handle
    /// are immediately observable through the editor's reactive
    /// signals (and vice versa).
    ///
    /// Use this when the caller needs to invoke editor commands from
    /// `on_activate_fn` / `ctx.effect` closures that outlive the
    /// borrow of `&editor`: `RichTextEditor` itself is move-only
    /// (the optional context-menu factory holds a `Box<dyn Fn>`,
    /// which prevents `Clone`).
    pub fn handle(&self) -> EditorHandle {
        EditorHandle {
            state: self.state.clone(),
        }
    }

    // --- Clipboard (programmatic) -----------------------------------------
    //
    // Direct programmatic counterparts of Ctrl+C / Ctrl+X / Ctrl+V /
    // Ctrl+Shift+V. The `ctx` argument is the active
    // [`EventContext`](bastyde_core::widget::EventContext) — the clipboard
    // lookup flows through `ctx.app_state::<ClipboardHandle>()` which
    // only has a value during event dispatch. Callers outside that
    // scope (e.g. ambient "restore from file" flows) should operate on
    // the `TextDocument` and the app-level clipboard directly.

    /// Copy the current selection to the system clipboard (plain +
    /// HTML payloads). No-op when there is no selection.
    ///
    /// All clipboard methods take `&EventContext` because they only
    /// need read access — the clipboard handle is looked up via
    /// `ctx.app_state::<ClipboardHandle>()`. A call site that holds
    /// `&mut EventContext` can pass `&ctx` directly; Rust reborrows
    /// automatically.
    pub fn copy(&self, ctx: &bastyde_core::widget::EventContext) {
        let mut st = self.state.borrow_mut();
        clipboard::copy(&mut st, ctx);
    }

    /// Cut the current selection: copy first, then remove.
    pub fn cut(&self, ctx: &bastyde_core::widget::EventContext) {
        {
            let mut st = self.state.borrow_mut();
            clipboard::cut(&mut st, ctx);
        }
        sync_cursor_signals(&self.state);
    }

    /// Paste from the system clipboard. Prefers an in-process fragment
    /// over HTML over plain text — see
    /// `rich_text/clipboard.rs`.
    pub fn paste(&self, ctx: &bastyde_core::widget::EventContext) {
        {
            let mut st = self.state.borrow_mut();
            clipboard::paste(&mut st, ctx);
        }
        sync_cursor_signals(&self.state);
    }

    /// Paste plain text only, stripping any rich payload.
    pub fn paste_unformatted(&self, ctx: &bastyde_core::widget::EventContext) {
        {
            let mut st = self.state.borrow_mut();
            clipboard::paste_unformatted(&mut st, ctx);
        }
        sync_cursor_signals(&self.state);
    }

    /// Whether a paste would insert anything — `true` iff the system
    /// clipboard carries text **or** an HTML payload (the shapes
    /// [`paste`](Self::paste) can consume; an HTML-only clipboard pastes
    /// fine, so probing plain text alone would under-report).
    ///
    /// Clipboard contents are not reactively observable, so this is a
    /// **point-in-time query** rather than a `Signal`: pass the active
    /// [`EventContext`](bastyde_core::widget::EventContext). It probes
    /// the clipboard (an X11 HTML probe can round-trip to the selection
    /// owner), so a menu / toolbar builder should re-query when the menu
    /// opens, not per frame. Returns `false` when no clipboard backend
    /// is installed (headless or feature-off builds) — the same
    /// "silently no-op" degradation the paste path itself uses.
    pub fn can_paste(&self, ctx: &bastyde_core::widget::EventContext) -> bool {
        clipboard::can_paste(ctx)
    }

    /// Set the per-editor logical font-size multiplier (`1.0` = 100 %).
    /// Composed with accessibility text scale at paint; forces relayout.
    /// See [`font_size_scale`](Self::font_size_scale).
    pub fn set_font_size_scale(&self, scale: f32) {
        let mut st = self.state.borrow_mut();
        let scale = scale.clamp(0.1, 10.0);
        if (st.font_size_scale - scale).abs() <= f32::EPSILON {
            return;
        }
        st.font_size_scale = scale;
        // Force the paint pass to re-push engine font_scale (it compares
        // against `last_font_scale` only).
        st.last_font_scale = f32::NAN;
        st.needs_full_layout = true;
        st.content_dirty = true;
        if let Some(handle) = &st.frame_request {
            handle.set(true);
        }
    }

    /// Current per-editor font-size scale (`1.0` = 100 %).
    pub fn get_font_size_scale(&self) -> f32 {
        self.state.borrow().font_size_scale
    }

    /// Set the non-destructive default typography at runtime. Re-lays out and
    /// schedules a repaint. Never mutates the document.
    pub fn set_typography_defaults(&self, defaults: EditorTypographyDefaults) {
        let mut st = self.state.borrow_mut();
        st.engine.set_typography_defaults(defaults);
        st.needs_full_layout = true;
        st.content_dirty = true;
        if let Some(handle) = &st.frame_request {
            handle.set(true);
        }
    }

    /// Current default typography (see [`typography_defaults`](Self::typography_defaults)).
    pub fn get_typography_defaults(&self) -> EditorTypographyDefaults {
        self.state.borrow().engine.typography_defaults().clone()
    }

    /// Set the typewriter-scrolling anchor at runtime — see
    /// [`typewriter`](Self::typewriter). `None` turns pinning off.
    ///
    /// Takes effect on the next caret move rather than scrolling immediately: a
    /// pin is a follow rule, and re-anchoring the page the instant a setting
    /// changes would jump the view under a reader who is not even typing.
    pub fn set_typewriter(&self, anchor: Option<f32>) {
        let mut st = self.state.borrow_mut();
        st.typewriter = anchor.map(|f| f.clamp(0.0, 1.0));
        // Drop the pin's dedup memory: the *next* caret move must re-pin even if
        // it lands where the last chase already was.
        st.last_chase_y = None;
    }

    /// Current typewriter anchor (see [`typewriter`](Self::typewriter)).
    pub fn get_typewriter(&self) -> Option<f32> {
        self.state.borrow().typewriter
    }

    /// Draw an ambient band behind the sentence — or paragraph — the caret is in.
    ///
    /// `None` (the default) draws nothing and registers no session on the document. The band
    /// shows only while **this** editor has focus, so two panes over one document never band
    /// twice, and it disappears when focus leaves the editor entirely.
    ///
    /// The band is registered below every other highlight layer, so a find match or a spell
    /// squiggle always paints over it. Give it a paint-only `format` — a background colour —
    /// or it will force a reshape on every caret move.
    pub fn set_caret_highlight(&self, highlight: Option<caret_highlight::CaretHighlight>) {
        set_caret_highlight(&self.state, highlight);
    }

    /// What this editor's caret band is currently configured to draw.
    pub fn get_caret_highlight(&self) -> Option<caret_highlight::CaretHighlight> {
        self.state
            .borrow()
            .caret_highlight
            .as_ref()
            .and_then(|s| s.config())
    }

    /// The caret's rectangle in **absolute window (tree) coordinates**, or
    /// `None` when the editor is unfocused or has not been laid out yet.
    ///
    /// The same rect the OS-IME reporting and the caret follow use, exposed for
    /// hosts that need to position something against the caret (and for tests
    /// that need to assert where a pin actually put it).
    pub fn caret_window_rect(&self) -> Option<bastyde_canvas::Rect> {
        self::keyboard::caret_window_rect(&self.state.borrow())
    }

    // --- Observability: reactive version counters -------------------------

    /// Signal that bumps on every format-only document event (bold /
    /// italic / heading / alignment / list style changes …).
    /// Distinct from [`document_version`](Self::document_version),
    /// which also bumps on content changes. Useful for toolbar
    /// observers that want to refresh button state on format changes
    /// without flickering during plain typing.
    pub fn format_version(&self) -> Signal<u64> {
        self.state.borrow().format_version.clone()
    }

    /// Signal that bumps once per document-loaded event (fires when
    /// an async `set_html` / `set_markdown` import completes). Starts
    /// at 0; observers see a new value each time a long import
    /// finishes.
    pub fn document_loaded_count(&self) -> Signal<u64> {
        self.state.borrow().document_loaded_count.clone()
    }

    // --- Link / image click callbacks -------------------------------------
    //
    // Installed via builder methods (below). The widget fires these
    // on a Primary PointerDown whose hit lands on a `HitRegion::Link`
    // or `HitRegion::Image`, before any caret placement.

    /// Install a callback fired when the user Primary-clicks a link
    /// (an element with an anchor `href`). The callback receives the
    /// href string and the active `EventContext`.
    ///
    /// The callback replaces any prior link-click callback on this
    /// builder chain. To stop observing, reconstruct the editor
    /// without the setter.
    pub fn on_link_activated(
        self,
        handler: impl Fn(&str, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.state.borrow_mut().on_link_activated = Some(std::rc::Rc::new(handler));
        self
    }

    /// Install a callback fired when the user Primary-clicks an inline
    /// image. The callback receives the image's resource name and the
    /// active `EventContext`.
    pub fn on_image_activated(
        self,
        handler: impl Fn(&str, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.state.borrow_mut().on_image_activated = Some(std::rc::Rc::new(handler));
        self
    }
}

// =============================================================================
// EditorHandle — external toolbar / palette handle
// =============================================================================

/// A clone-able, `'static` handle to a [`RichTextEditor`]'s shared
/// state.
///
/// Use this when a toolbar, palette, command panel, or other external
/// widget needs to invoke editor commands from `on_activate_fn` /
/// `ctx.effect` closures that outlive the borrow of `&editor`.
/// [`RichTextEditor`] itself is move-only (the optional
/// `custom_context_menu` factory holds a `Box<dyn Fn>`, which prevents
/// `Clone`), so a closure cannot just capture `editor.clone()`.
/// Obtain a handle via [`RichTextEditor::handle()`] and clone it into
/// each closure that needs to issue commands.
///
/// `EditorHandle` mirrors the toolbar-relevant subset of the editor's
/// public API:
///
/// * Inline character formatting — [`set_bold`](Self::set_bold) /
///   [`toggle_bold`](Self::toggle_bold) / [`is_bold`](Self::is_bold)
///   and the italic / underline / strikethrough variants.
/// * Block-level formatting — [`set_alignment`](Self::set_alignment),
///   [`set_heading_level`](Self::set_heading_level),
///   [`apply_block_format`](Self::apply_block_format),
///   [`insert_list`](Self::insert_list),
///   [`indent`](Self::indent) / [`outdent`](Self::outdent).
/// * Tables — [`insert_table`](Self::insert_table) and the per-row /
///   per-column / remove operations, plus [`is_in_table`](Self::is_in_table)
///   for contextual UI enable state.
/// * History — [`undo`](Self::undo) / [`redo`](Self::redo).
/// * Clipboard — [`copy`](Self::copy) / [`cut`](Self::cut) /
///   [`paste`](Self::paste) /
///   [`paste_unformatted`](Self::paste_unformatted), plus
///   [`can_paste`](Self::can_paste) for Paste enable-state — so a
///   context-menu factory (which can only capture a handle, never the
///   editor that owns it) can rebuild Cut / Copy / Paste /
///   Paste-Unformatted.
/// * Selection — [`select_all`](Self::select_all) /
///   [`delete_selection`](Self::delete_selection).
/// * Reactive signal accessors —
///   [`format_version`](Self::format_version),
///   [`cursor_position_signal`](Self::cursor_position_signal),
///   [`cursor_anchor_signal`](Self::cursor_anchor_signal),
///   [`has_selection`](Self::has_selection),
///   [`can_undo`](Self::can_undo) / [`can_redo`](Self::can_redo) — so
///   callers that hold only an `EditorHandle` can derive bound signals
///   without keeping a separate `RichTextEditor` reference.
///
/// Cloning is cheap (an `Rc` clone). All clones share the same
/// underlying state — mutations through any clone, through other
/// clones, or through the originating `RichTextEditor` are all
/// immediately observable through the same signals.
#[derive(Clone)]
pub struct EditorHandle {
    state: SharedState,
}

impl std::fmt::Debug for EditorHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorHandle").finish_non_exhaustive()
    }
}

impl EditorHandle {
    // --- Search / find-banner support (B3, handle mirror) ------------------
    //
    // These mirror the same-named [`RichTextEditor`] methods (which operate on
    // the same `state`), so a per-editor find banner built *above* the editor
    // can drive selection / scroll-into-view on the current match through the
    // handle it captured — the widget itself is long gone into the tree by then.

    /// Reactive signal — `true` while **this** editor holds keyboard focus.
    /// See [`RichTextEditor::focused_signal`].
    pub fn focused_signal(&self) -> Signal<bool> {
        self.state.borrow().focus_signal.clone()
    }

    /// Select the character range `[start, end)` without collapsing (anchor at
    /// `start`, caret at `end`). See [`RichTextEditor::select_range`].
    pub fn select_range(&self, start: usize, end: usize) {
        {
            let mut st = self.state.borrow_mut();
            st.cursor.set_position(start, MoveMode::MoveAnchor);
            st.cursor.set_position(end, MoveMode::KeepAnchor);
            st.cursor_affinity = bastyde_text::CursorAffinity::Downstream;
        }
        sync_cursor_signals(&self.state);
    }

    /// Replace the character range `[start, end)` with `text`, leaving the caret
    /// after the inserted text.
    ///
    /// The counterpart to [`select_range`](Self::select_range) for callers that
    /// must *rewrite* a span rather than merely reveal it — a spell-check
    /// correction picked from a context menu, an autocorrect, a
    /// replace-this-occurrence action. It goes through the widget's **internal**
    /// cursor, so the edit behaves exactly like typed text: it lands on the
    /// editor's undo stack as one entry (the replacement is a single
    /// insert-over-selection), fires the document's change notifications, and
    /// leaves the caret where the user would expect it.
    ///
    /// Offsets are **character** positions, the same space
    /// [`cursor_position`](Self::cursor_position) and `select_range` use. The
    /// inserted text inherits the character format at `start`, so correcting a
    /// word inside italic prose stays italic.
    ///
    /// Reaching through [`TextDocument::cursor`](bastyde_text::text_document::TextDocument::cursor)
    /// instead would mutate the document behind the widget's back, leaving the
    /// caret decoupled from the edit — use this.
    pub fn replace_range(&self, start: usize, end: usize, text: &str) {
        // Select, then insert over the selection — each step in its own borrow
        // scope, mirroring `select_range` / `RichTextEditor::insert_text`. The
        // insert must not run while a `borrow_mut` is held: it notifies document
        // observers, which are free to read the state back.
        self.select_range(start, end);
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_text(text);
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert plain text at the caret, replacing any selection. The
    /// [`EditorHandle`] counterpart of
    /// [`RichTextEditor::insert_text`](RichTextEditor::insert_text), for callers
    /// that hold only a handle — a toolbar button or a global menu command.
    pub fn insert_text(&self, text: &str) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_text(text);
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert a fragment parsed from djot at the caret, replacing any selection.
    ///
    /// Unlike [`insert_text`](Self::insert_text), which drops its bytes into the
    /// current block verbatim (a `\n` becomes literal content, not a new
    /// paragraph), this parses block-level djot into a `DocumentFragment`, so
    /// inserting a standalone paragraph really does create one.
    pub fn insert_djot(&self, djot: &str) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_djot(djot);
        }
        sync_cursor_signals(&self.state);
    }

    /// Split the current block at the caret, as pressing Enter does.
    pub fn insert_block(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_block();
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert `text` as a **paragraph of its own** at the caret: split here, fill
    /// the new block, split again, so whatever followed the caret continues in a
    /// third block.
    ///
    /// Deliberately one call rather than three. Composing
    /// `insert_block` + `insert_text` + `insert_block` from outside re-enters the
    /// widget three times, and an application that rebuilds its editor in
    /// response to the first change notification is left driving a handle that
    /// no longer points at the mounted widget — the split lands and the text
    /// silently does not. Doing the whole edit under a single borrow, with one
    /// signal sync at the end, makes it atomic from the caller's side.
    /// Returns `false` if any step failed, leaving the document as far as it
    /// got. Steps are **not** attempted after a failure: filling and re-splitting
    /// on top of a split that did not happen produces a mangled paragraph rather
    /// than a partial one, and the caller has no way to tell.
    pub fn insert_paragraph(&self, text: &str) -> bool {
        let ok = {
            let st = self.state.borrow();
            st.cursor.insert_block().is_ok()
                && st.cursor.insert_text(text).is_ok()
                && st.cursor.insert_block().is_ok()
        };
        sync_cursor_signals(&self.state);
        ok
    }

    /// The live selection as `(anchor, position)`, unordered — `anchor` is where the
    /// selection started, `position` is where the caret is, so a backwards drag
    /// reports `anchor > position`. Equal values mean no selection.
    ///
    /// Both ends are read under a **single** borrow, so the pair cannot tear. That is
    /// the reason to prefer this over pairing [`cursor_position`](Self::cursor_position)
    /// with [`cursor_anchor_signal`](Self::cursor_anchor_signal): the former is a live
    /// read of the cursor while the latter is a mirror refreshed on sync, so combining
    /// them mixes two different moments in time and can invent — or miss — a selection
    /// if the mirror lags. A caller deciding *"is there a selection, and over what"*
    /// wants one consistent answer.
    pub fn selection(&self) -> (usize, usize) {
        let st = self.state.borrow();
        (st.cursor.anchor(), st.cursor.position())
    }

    /// Hit-test a point — **in window coordinates**, as a
    /// [`context_menu`](RichTextEditor::context_menu) factory receives it — to a
    /// document character offset. `None` when the point resolves to no text
    /// (past the last glyph on an empty line, outside the body, etc.).
    ///
    /// Lets a custom context-menu factory resolve "the word under the pointer"
    /// from the right-click position, since a bare right-click does not move the
    /// caret on its own.
    pub fn offset_at_point(&self, window_point: Point) -> Option<usize> {
        mouse::offset_at_window_point(&self.state, window_point)
    }

    /// Reposition the caret to a right-click point (**window coordinates**)
    /// unless the click lands inside the current selection (then the selection
    /// is preserved). Call this at the top of a custom
    /// [`context_menu`](RichTextEditor::context_menu) factory so the menu's Paste
    /// — and any caret-relative action — operates where the user clicked, exactly
    /// as the built-in menu and the single-line field do.
    pub fn reposition_caret_for_context_menu(&self, window_point: Point) {
        mouse::reposition_caret_for_context_menu(&self.state, window_point);
    }

    /// Scroll the character range `[start, end)` into view. A no-op until the
    /// editor has a full layout. See [`RichTextEditor::reveal_range`].
    pub fn reveal_range(
        &self,
        ctx: &mut bastyde_core::widget::EventContext,
        start: usize,
        end: usize,
    ) {
        reveal_range_impl(&self.state, ctx, start, end);
    }

    /// Move keyboard focus onto the editor. Lets a control built *above* the
    /// editor — a find banner returning focus to the prose on Escape — put the
    /// caret back where the user expects. A no-op until the editor has built at
    /// least once (its wrapper id is stashed then).
    pub fn focus(&self, ctx: &mut bastyde_core::widget::EventContext) {
        if let Some(id) = self.state.borrow().self_id {
            ctx.request_focus(id);
        }
    }

    // --- Character-format query / apply ------------------------------------

    /// Read the current character format at the caret. When a selection
    /// is active, reads from `selection_start()` rather than
    /// `position()` so toolbar bistate stays stable across selection
    /// extension (same rule as
    /// [`RichTextEditor::caret_char_format`]).
    pub fn caret_char_format(&self) -> TextFormat {
        let st = self.state.borrow();
        let probe_pos = if st.cursor.has_selection() {
            st.cursor.selection_start()
        } else {
            st.cursor.position()
        };
        let probe = st.document.cursor();
        probe.set_position(probe_pos, MoveMode::MoveAnchor);
        probe.char_format().unwrap_or_default()
    }

    fn apply_char_format(&self, fmt: TextFormat) {
        let st = self.state.borrow();
        let _ = st.cursor.merge_char_format(&fmt);
    }

    /// Apply **bold** to the current selection.
    pub fn set_bold(&self, enabled: bool) {
        self.apply_char_format(TextFormat {
            font_bold: Some(enabled),
            ..Default::default()
        });
    }

    /// Apply *italic* to the current selection.
    pub fn set_italic(&self, enabled: bool) {
        self.apply_char_format(TextFormat {
            font_italic: Some(enabled),
            ..Default::default()
        });
    }

    /// Apply underline to the current selection.
    pub fn set_underline(&self, enabled: bool) {
        self.apply_char_format(TextFormat {
            font_underline: Some(enabled),
            ..Default::default()
        });
    }

    /// Apply strikethrough to the current selection.
    pub fn set_strikethrough(&self, enabled: bool) {
        self.apply_char_format(TextFormat {
            font_strikeout: Some(enabled),
            ..Default::default()
        });
    }

    /// Set the font family for the current selection (a character-format
    /// change applied over the selected range). Like the other char-format
    /// setters (`set_bold`, …), this is a **no-op when there is no
    /// selection** — the document model has no typing/pending format, so a
    /// bare caret has no range to format. `family` must be a name resolvable
    /// by the shared typesetter's font registrar — e.g. a value chosen from
    /// a [`FontPicker`](crate::font_picker::FontPicker).
    pub fn set_font_family(&self, family: impl Into<String>) {
        self.apply_char_format(TextFormat {
            font_family: Some(family.into()),
            ..Default::default()
        });
    }

    /// Set the font size (in points) for the current selection.
    pub fn set_font_size(&self, size: u32) {
        self.apply_char_format(TextFormat {
            font_point_size: Some(size),
            ..Default::default()
        });
    }

    // --- Default typography / font size (non-destructive, whole editor) ---

    /// Set the non-destructive default typography (font family / line height /
    /// first-line indent) filled onto runs and blocks with no explicit
    /// override. Unlike [`set_font_family`](Self::set_font_family) /
    /// [`set_font_size`](Self::set_font_size) — which mutate the selected text —
    /// this is a display-time default: it never touches the document, undo
    /// stack, or `modified` flag. Schedules a relayout + repaint.
    pub fn set_typography_defaults(&self, defaults: EditorTypographyDefaults) {
        let mut st = self.state.borrow_mut();
        st.engine.set_typography_defaults(defaults);
        st.needs_full_layout = true;
        st.content_dirty = true;
        if let Some(handle) = &st.frame_request {
            handle.set(true);
        }
    }

    /// Current default typography.
    pub fn get_typography_defaults(&self) -> EditorTypographyDefaults {
        self.state.borrow().engine.typography_defaults().clone()
    }

    /// Set the per-editor logical font-size multiplier. See
    /// [`RichTextEditor::set_font_size_scale`].
    pub fn set_font_size_scale(&self, scale: f32) {
        let mut st = self.state.borrow_mut();
        let scale = scale.clamp(0.1, 10.0);
        if (st.font_size_scale - scale).abs() <= f32::EPSILON {
            return;
        }
        st.font_size_scale = scale;
        st.last_font_scale = f32::NAN;
        st.needs_full_layout = true;
        st.content_dirty = true;
        if let Some(handle) = &st.frame_request {
            handle.set(true);
        }
    }

    /// Current per-editor font-size scale (`1.0` = 100 %).
    pub fn get_font_size_scale(&self) -> f32 {
        self.state.borrow().font_size_scale
    }

    /// Set the typewriter-scrolling anchor — the [`EditorHandle`] counterpart of
    /// [`RichTextEditor::set_typewriter`]. `None` turns pinning off.
    ///
    /// This is the door a host uses to keep the pin following a live setting,
    /// the same way [`set_typography_defaults`](Self::set_typography_defaults)
    /// keeps typography following one.
    pub fn set_typewriter(&self, anchor: Option<f32>) {
        let mut st = self.state.borrow_mut();
        st.typewriter = anchor.map(|f| f.clamp(0.0, 1.0));
        st.last_chase_y = None;
    }

    /// Current typewriter anchor.
    pub fn get_typewriter(&self) -> Option<f32> {
        self.state.borrow().typewriter
    }

    /// Draw an ambient band behind the caret's sentence or paragraph — the [`EditorHandle`]
    /// counterpart of [`RichTextEditor::set_caret_highlight`], for hosts that re-push it from a
    /// settings or theme effect after the editor is mounted.
    pub fn set_caret_highlight(&self, highlight: Option<caret_highlight::CaretHighlight>) {
        set_caret_highlight(&self.state, highlight);
    }

    /// What this editor's caret band is currently configured to draw.
    pub fn get_caret_highlight(&self) -> Option<caret_highlight::CaretHighlight> {
        self.state
            .borrow()
            .caret_highlight
            .as_ref()
            .and_then(|s| s.config())
    }

    /// The caret's rectangle in **absolute window (tree) coordinates** — the
    /// [`EditorHandle`] counterpart of [`RichTextEditor::caret_window_rect`].
    /// `None` when unfocused or not yet laid out.
    pub fn caret_window_rect(&self) -> Option<bastyde_canvas::Rect> {
        self::keyboard::caret_window_rect(&self.state.borrow())
    }

    /// Apply an arbitrary [`TextFormat`] (escape hatch for fields not
    /// covered by the dedicated setters: `letter_spacing`,
    /// `foreground_color`, …).
    pub fn apply_text_format(&self, fmt: TextFormat) {
        self.apply_char_format(fmt);
    }

    /// Toggle bold on the current selection.
    pub fn toggle_bold(&self) {
        let current = self.caret_char_format().font_bold.unwrap_or(false);
        self.set_bold(!current);
    }

    /// Toggle italic on the current selection.
    pub fn toggle_italic(&self) {
        let current = self.caret_char_format().font_italic.unwrap_or(false);
        self.set_italic(!current);
    }

    /// Toggle underline on the current selection.
    pub fn toggle_underline(&self) {
        let current = self.caret_char_format().font_underline.unwrap_or(false);
        self.set_underline(!current);
    }

    /// Toggle strikethrough on the current selection.
    pub fn toggle_strikethrough(&self) {
        let current = self.caret_char_format().font_strikeout.unwrap_or(false);
        self.set_strikethrough(!current);
    }

    /// Whether the selection / typing position is bold.
    pub fn is_bold(&self) -> bool {
        self.caret_char_format().font_bold.unwrap_or(false)
    }

    /// Whether italic.
    pub fn is_italic(&self) -> bool {
        self.caret_char_format().font_italic.unwrap_or(false)
    }

    /// Whether underline.
    pub fn is_underline(&self) -> bool {
        self.caret_char_format().font_underline.unwrap_or(false)
    }

    /// Whether strikethrough.
    pub fn is_strikethrough(&self) -> bool {
        self.caret_char_format().font_strikeout.unwrap_or(false)
    }

    // --- Vertical alignment (super / subscript) ----------------------------
    //
    // See [`RichTextEditor::set_superscript`]: one tri-state property shown as
    // two toggles, because a run cannot be both raised and lowered.

    /// Raise the selection to superscript, or return it to the baseline.
    pub fn set_superscript(&self, enabled: bool) {
        self.set_vertical_alignment(if enabled {
            CharVerticalAlignment::SuperScript
        } else {
            CharVerticalAlignment::Normal
        });
    }

    /// Lower the selection to subscript, or return it to the baseline.
    pub fn set_subscript(&self, enabled: bool) {
        self.set_vertical_alignment(if enabled {
            CharVerticalAlignment::SubScript
        } else {
            CharVerticalAlignment::Normal
        });
    }

    /// Set the selection's vertical alignment directly.
    pub fn set_vertical_alignment(&self, alignment: CharVerticalAlignment) {
        self.apply_char_format(TextFormat {
            vertical_alignment: Some(alignment),
            ..Default::default()
        });
    }

    /// The caret's vertical alignment, `Normal` when unset.
    pub fn get_vertical_alignment(&self) -> CharVerticalAlignment {
        self.caret_char_format()
            .vertical_alignment
            .unwrap_or(CharVerticalAlignment::Normal)
    }

    /// True while the caret sits in superscript text.
    pub fn is_superscript(&self) -> bool {
        self.get_vertical_alignment() == CharVerticalAlignment::SuperScript
    }

    /// True while the caret sits in subscript text.
    pub fn is_subscript(&self) -> bool {
        self.get_vertical_alignment() == CharVerticalAlignment::SubScript
    }

    /// Flip superscript on the selection. Turning it on replaces subscript.
    pub fn toggle_superscript(&self) {
        self.set_superscript(!self.is_superscript());
    }

    /// Flip subscript on the selection. Turning it on replaces superscript.
    pub fn toggle_subscript(&self) {
        self.set_subscript(!self.is_subscript());
    }

    // --- Block-format query / apply ----------------------------------------

    /// Apply an arbitrary [`BlockFormat`] to the caret's block.
    pub fn apply_block_format(&self, fmt: BlockFormat) {
        let st = self.state.borrow();
        let _ = st.cursor.set_block_format(&fmt);
    }

    /// Set paragraph alignment for the caret's block.
    pub fn set_alignment(&self, alignment: Alignment) {
        self.apply_block_format(BlockFormat {
            alignment: Some(alignment),
            ..Default::default()
        });
    }

    /// Unset the block's direction, handing the paragraph back to
    /// automatic detection.
    ///
    /// Not the same as setting left-to-right. An explicit direction
    /// *pins* the paragraph and overrides the bidi algorithm, so
    /// "clearing" a direction by writing `LeftToRight` would force
    /// Arabic and Hebrew prose to lay out backwards. Only an unset
    /// direction lets the text speak for itself.
    pub fn clear_direction(&self) {
        self.apply_block_format(BlockFormat {
            clear_direction: true,
            ..Default::default()
        });
    }

    /// Set the base reading direction of the caret's block. See
    /// [`RichTextEditor::set_direction`].
    pub fn set_direction(&self, direction: TextDirection) {
        self.apply_block_format(BlockFormat {
            direction: Some(direction),
            ..Default::default()
        });
    }

    /// Set heading level for the caret's block. `0` = plain paragraph,
    /// `1..=6` follow the HTML `<h1>..<h6>` convention.
    pub fn set_heading_level(&self, level: u8) {
        self.apply_block_format(BlockFormat {
            heading_level: Some(level),
            ..Default::default()
        });
    }

    /// Current block alignment.
    pub fn get_alignment(&self) -> Alignment {
        self.state
            .borrow()
            .cursor
            .block_format()
            .ok()
            .and_then(|f| f.alignment)
            .unwrap_or(Alignment::Left)
    }

    /// The block's explicitly-set reading direction, if it has one.
    ///
    /// `None` means the writer never chose — the bidi algorithm decides
    /// from the text. That is a genuinely different state from an
    /// explicit left-to-right, so it is reported rather than defaulted:
    /// a toggle needs to show "auto" as its own setting.
    pub fn get_direction(&self) -> Option<TextDirection> {
        self.state
            .borrow()
            .cursor
            .block_format()
            .ok()
            .and_then(|f| f.direction)
    }

    /// Current heading level (0 = plain paragraph).
    pub fn get_heading_level(&self) -> u8 {
        self.state
            .borrow()
            .cursor
            .block_format()
            .ok()
            .and_then(|f| f.heading_level)
            .unwrap_or(0)
    }

    // --- Lists -------------------------------------------------------------

    /// Wrap the caret's block in a list. `ordered = true` uses decimal
    /// numbering, `false` uses bullet discs.
    pub fn insert_list(&self, ordered: bool) {
        let style = if ordered {
            ListStyle::Decimal
        } else {
            ListStyle::Disc
        };
        self.create_list(style);
    }

    /// Wrap the caret's block in a list with an explicit
    /// [`ListStyle`].
    pub fn create_list(&self, style: ListStyle) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.create_list(style);
        }
        sync_cursor_signals(&self.state);
    }

    /// Indent the caret's current list item by one nesting level.
    /// No-op when the caret is not inside a list. Equivalent to Tab.
    pub fn indent(&self) {
        keyboard::indent_current_block(&mut self.state.borrow_mut());
        sync_cursor_signals(&self.state);
    }

    /// Outdent the caret's current list item by one nesting level.
    /// No-op at depth 0. Equivalent to Shift+Tab.
    pub fn outdent(&self) {
        keyboard::dedent_current_block(&mut self.state.borrow_mut());
        sync_cursor_signals(&self.state);
    }

    /// Take the caret's block out of its list entirely, leaving a plain
    /// paragraph. No-op when the caret is not inside a list.
    ///
    /// See [`RichTextEditor::remove_from_list`] for why this is separate from
    /// [`outdent`](Self::outdent), which stops at depth 0 by design.
    pub fn remove_from_list(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.remove_current_block_from_list();
        }
        sync_cursor_signals(&self.state);
    }

    // --- Blockquotes -------------------------------------------------------

    /// True iff the caret currently sits inside a blockquote frame at
    /// any nesting depth.
    pub fn is_in_blockquote(&self) -> bool {
        let st = self.state.borrow();
        st.cursor.is_in_blockquote()
    }

    /// True iff the selection spans more than one frame — the
    /// "Toggle blockquote" affordance should be disabled in this case.
    pub fn selection_spans_multiple_frames(&self) -> bool {
        let st = self.state.borrow();
        st.cursor.selection_spans_multiple_frames()
    }

    /// Wrap the current block/selection in a blockquote, or unwrap the
    /// innermost enclosing blockquote if already inside one. Toolbar
    /// counterpart for a Ctrl+Shift+Q-style toggle.
    pub fn toggle_blockquote(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.toggle_blockquote();
        }
        sync_cursor_signals(&self.state);
    }

    /// Wrap the current block in a deeper nested quote. Equivalent to
    /// Tab inside a blockquote.
    pub fn increase_blockquote_depth(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.increase_blockquote_depth();
        }
        sync_cursor_signals(&self.state);
    }

    /// Pop the caret out of one blockquote nesting level. Equivalent to
    /// Shift+Tab inside a blockquote.
    pub fn decrease_blockquote_depth(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.decrease_blockquote_depth();
        }
        sync_cursor_signals(&self.state);
    }

    // --- Tables ------------------------------------------------------------

    /// Insert a fresh `rows × columns` table at the caret.
    pub fn insert_table(&self, rows: usize, columns: usize) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_table(rows, columns);
        }
        sync_cursor_signals(&self.state);
    }

    /// Remove the table containing the caret. No-op outside a table.
    pub fn remove_current_table(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.remove_current_table();
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert a row above the caret's current table row.
    pub fn insert_row_above(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_row_above();
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert a row below the caret's current table row.
    pub fn insert_row_below(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_row_below();
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert a column before the caret's current table column.
    pub fn insert_column_before(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_column_before();
        }
        sync_cursor_signals(&self.state);
    }

    /// Insert a column after the caret's current table column.
    pub fn insert_column_after(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.insert_column_after();
        }
        sync_cursor_signals(&self.state);
    }

    /// Remove the caret's current table row.
    pub fn remove_current_row(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.remove_current_row();
        }
        sync_cursor_signals(&self.state);
    }

    /// Remove the caret's current table column.
    pub fn remove_current_column(&self) {
        {
            let st = self.state.borrow();
            let _ = st.cursor.remove_current_column();
        }
        sync_cursor_signals(&self.state);
    }

    /// Whether the caret is currently inside a table cell.
    pub fn is_in_table(&self) -> bool {
        self.state.borrow().cursor.current_table().is_some()
    }

    // --- History -----------------------------------------------------------

    /// Undo the most recent edit. No-op when the undo stack is empty.
    pub fn undo(&self) {
        let _ = self.state.borrow().document.undo();
        sync_cursor_signals(&self.state);
    }

    /// Redo the most recently undone edit. No-op when the redo stack
    /// is empty.
    pub fn redo(&self) {
        let _ = self.state.borrow().document.redo();
        sync_cursor_signals(&self.state);
    }

    // --- Edit blocks (composite undo) --------------------------------------
    //
    // See [`RichTextEditor::begin_edit_block`] for the rationale: a toolbar
    // action composed of several commands should cost one Ctrl+Z, not one per
    // property it touched.

    /// Begin grouping subsequent edits into a single undo entry. Pair with
    /// [`end_edit_block`](Self::end_edit_block), or prefer the scoped
    /// [`edit_block`](Self::edit_block).
    pub fn begin_edit_block(&self) {
        self.state.borrow().cursor.begin_edit_block();
    }

    /// Close the group opened by [`begin_edit_block`](Self::begin_edit_block).
    pub fn end_edit_block(&self) {
        self.state.borrow().cursor.end_edit_block();
    }

    /// Run `edits` as one undo entry — the pairing-safe form.
    pub fn edit_block<R>(&self, edits: impl FnOnce() -> R) -> R {
        self.begin_edit_block();
        let result = edits();
        self.end_edit_block();
        result
    }

    // --- Clipboard ---------------------------------------------------------
    //
    // Programmatic counterparts of Ctrl+C / Ctrl+X / Ctrl+V /
    // Ctrl+Shift+V, mirroring [`RichTextEditor::copy`] / `cut` / `paste` /
    // `paste_unformatted` body-for-body. Each takes the active
    // [`EventContext`](bastyde_core::widget::EventContext) because the
    // clipboard handle is looked up via `ctx.app_state::<ClipboardHandle>()`,
    // which only has a value during event dispatch — so these are callable
    // from an `on_activate_fn` / context-menu closure that captured just a
    // handle. A call site holding `&mut EventContext` can pass `&ctx`
    // directly; Rust reborrows automatically.

    /// Copy the current selection to the system clipboard (plain + HTML
    /// payloads). No-op when there is no selection. See
    /// [`RichTextEditor::copy`].
    pub fn copy(&self, ctx: &bastyde_core::widget::EventContext) {
        let mut st = self.state.borrow_mut();
        clipboard::copy(&mut st, ctx);
    }

    /// Cut the current selection: copy first, then remove. See
    /// [`RichTextEditor::cut`].
    pub fn cut(&self, ctx: &bastyde_core::widget::EventContext) {
        {
            let mut st = self.state.borrow_mut();
            clipboard::cut(&mut st, ctx);
        }
        sync_cursor_signals(&self.state);
    }

    /// Paste from the system clipboard. Prefers an in-process fragment
    /// over HTML over plain text. See [`RichTextEditor::paste`].
    pub fn paste(&self, ctx: &bastyde_core::widget::EventContext) {
        {
            let mut st = self.state.borrow_mut();
            clipboard::paste(&mut st, ctx);
        }
        sync_cursor_signals(&self.state);
    }

    /// Paste plain text only, stripping any rich payload. See
    /// [`RichTextEditor::paste_unformatted`].
    pub fn paste_unformatted(&self, ctx: &bastyde_core::widget::EventContext) {
        {
            let mut st = self.state.borrow_mut();
            clipboard::paste_unformatted(&mut st, ctx);
        }
        sync_cursor_signals(&self.state);
    }

    /// Whether a paste would insert anything — `true` iff the system
    /// clipboard carries text **or** an HTML payload. A point-in-time
    /// query (clipboard contents are not reactively observable), taking
    /// the active [`EventContext`](bastyde_core::widget::EventContext).
    /// Use it to drive a context-menu / toolbar Paste enable-state,
    /// re-querying on menu-open. Mirrors [`RichTextEditor::can_paste`].
    pub fn can_paste(&self, ctx: &bastyde_core::widget::EventContext) -> bool {
        clipboard::can_paste(ctx)
    }

    // --- Selection ---------------------------------------------------------

    /// Select the entire document programmatically. Resets the Ctrl+A
    /// ladder so a subsequent Ctrl+A starts fresh at level 1. Mirrors
    /// [`RichTextEditor::select_all`].
    pub fn select_all(&self) {
        {
            let mut st = self.state.borrow_mut();
            st.cursor.select(SelectionType::Document);
            st.select_all_level = 0;
            st.select_all_anchor_cell = None;
        }
        sync_cursor_signals(&self.state);
    }

    /// Delete the current selection. No-op when nothing is selected.
    /// Mirrors [`RichTextEditor::delete_selection`].
    pub fn delete_selection(&self) {
        {
            let st = self.state.borrow();
            if st.cursor.has_selection() {
                let _ = st.cursor.remove_selected_text();
            }
        }
        sync_cursor_signals(&self.state);
    }

    // --- Reactive signal accessors -----------------------------------------

    /// Bumps on every format-only document event (bold / italic /
    /// heading / alignment / list-style changes). See
    /// [`RichTextEditor::format_version`].
    pub fn format_version(&self) -> Signal<u64> {
        self.state.borrow().format_version.clone()
    }

    /// The **live** caret offset — reads `cursor.position()` directly, unbatched. Unlike
    /// [`cursor_position_signal`](Self::cursor_position_signal), whose stored value lags one frame
    /// behind a just-typed printable character (the insert is deferred to the frame loop and the
    /// signal is only re-synced on the *next* caret event), this always reflects the true caret —
    /// what a host that recomputes highlights on a frame tick must read. Mirrors
    /// [`RichTextEditor::cursor_position`].
    pub fn cursor_position(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    /// `true` while an IME composition is actively in progress. Mirrors
    /// [`RichTextEditor::is_composing`].
    pub fn is_composing(&self) -> bool {
        self.state.borrow().ime_preedit.is_some()
    }

    /// Reactive caret position signal.
    pub fn cursor_position_signal(&self) -> Signal<usize> {
        self.state.borrow().cursor_position.clone()
    }

    /// Reactive selection anchor signal.
    pub fn cursor_anchor_signal(&self) -> Signal<usize> {
        self.state.borrow().cursor_anchor.clone()
    }

    /// Reactive selection-non-empty signal.
    pub fn has_selection(&self) -> Signal<bool> {
        self.state.borrow().has_selection.clone()
    }

    /// Reactive undo-availability signal (toolbar enable-state source).
    pub fn can_undo(&self) -> Signal<bool> {
        self.state.borrow().can_undo.clone()
    }

    /// Reactive redo-availability signal.
    pub fn can_redo(&self) -> Signal<bool> {
        self.state.borrow().can_redo.clone()
    }
}

/// Private leaf body for [`RichTextEditor`].
///
/// Pure rendering surface: layout (intrinsic / greedy via
/// `min_lines` / `max_lines`), `place_children` (records the
/// viewport on `state`), `paint` (glyph runs, caret, selection),
/// `accessibility` (Role::MultilineTextInput / Role::Document plus
/// the flow-snapshot walk that emits paragraph + text-run children).
///
/// Handlers, focus, the context-menu factory, and per-frame ticking
/// all live on the composing outer [`RichTextEditor`]; the body
/// itself is non-focusable and has no event handlers. The shared
/// `state` is what links them — both widgets hold an `Rc` to the
/// same [`EditorState`], so a key event on the wrapper mutates the
/// state and the body re-paints on the next frame.
pub(crate) struct RichTextEditorBody {
    state: SharedState,
    min_lines: Option<u32>,
    max_lines: Option<u32>,
}

impl std::fmt::Debug for RichTextEditorBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichTextEditorBody")
            .field("policy", &self.state.borrow().policy)
            .finish_non_exhaustive()
    }
}

impl Widget for RichTextEditorBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Bind `caret_visible` to the framework's repaint tracker so
        // that every toggle in the frame-tick effect marks **this
        // body** widget `needs_paint` — the caret is painted in
        // `RichTextEditorBody::paint`. Skipped for `CaretPolicy::Hidden`.
        {
            let st = self.state.borrow();
            let caret_policy = st.policy.caret_policy;
            let caret_visible = st.caret_visible.clone();
            drop(st);
            if caret_policy != CaretPolicy::Hidden {
                let self_id = ctx.self_id();
                caret_visible.bind_to(
                    self_id,
                    ctx.binding_registry(),
                    bastyde_core::binding::BindingLevel::RepaintOnly,
                );
            }
        }

        // Bind document_version at `BindingLevel::AccessibilityOnly` so
        // text / format edits flip the tree's `a11y_dirty` flag through
        // **this body** — its `accessibility()` is the one that emits
        // the editor's Role::MultilineTextInput / Role::Document and
        // walks the flow snapshot.
        //
        // ALSO bind at `RepaintOnly` so the widget's needs_paint flips
        // on every text / format change. Without this, paint() only
        // ran on caret-blink (the only other RepaintOnly binding), and
        // the post-fix dispatch's `last_relayout_block_id.take()` was
        // consumed on the wrong tick — leaving text edits invisible
        // until a resize forced a full re-layout.
        {
            let st = self.state.borrow();
            let document_version = st.document_version.clone();
            drop(st);
            let self_id = ctx.self_id();
            document_version.bind_to(
                self_id,
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::AccessibilityOnly,
            );
            document_version.bind_to(
                self_id,
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // Bind `scroll_y`, `scroll_x`, `cursor_position`, `cursor_anchor`,
        // and `has_selection` at RepaintOnly so the widget marks
        // needs_paint immediately on scroll, cursor move, and selection
        // change. Without these, paint() only ran on caret-blink and
        // text-version bumps — so scroll/selection changes appeared
        // delayed by up to 500ms (in sync with the next caret toggle).
        //
        // The cursor_only render path inside text-typeset falls back to
        // a full render automatically when scroll drifted since
        // the last full render, so this binding is correctness-safe.
        {
            let st = self.state.borrow();
            let scroll_y = st.scroll_y.clone();
            let scroll_x = st.scroll_x.clone();
            let cursor_position = st.cursor_position.clone();
            let cursor_anchor = st.cursor_anchor.clone();
            let has_selection = st.has_selection.clone();
            drop(st);
            let self_id = ctx.self_id();
            for signal in [&scroll_y, &scroll_x] {
                signal.bind_to(
                    self_id,
                    ctx.binding_registry(),
                    bastyde_core::binding::BindingLevel::RepaintOnly,
                );
            }
            // Caret and anchor are repaint-only for geometry, but they ALSO
            // change what the a11y walk reports via `set_text_selection_to`. A
            // caret-only move (arrow key, click, drag-select) emits no document
            // event, so `document_version` never bumps; without an
            // `AccessibilityOnly` binding here `a11y_dirty` never flips and a
            // screen reader hears the caret frozen at the last edit. Bind both
            // levels — the two-level pattern `document_version` uses. Selecting
            // moves the caret and/or anchor, so `has_selection` (derived from
            // them) needs no separate a11y binding.
            for signal in [&cursor_position, &cursor_anchor] {
                signal.bind_to(
                    self_id,
                    ctx.binding_registry(),
                    bastyde_core::binding::BindingLevel::RepaintOnly,
                );
                signal.bind_to(
                    self_id,
                    ctx.binding_registry(),
                    bastyde_core::binding::BindingLevel::AccessibilityOnly,
                );
            }
            has_selection.bind_to(
                self_id,
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::RepaintOnly,
            );
        }

        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let w = proposal.width.unwrap_or(200.0).max(0.0);

        // Greedy mode (default, behaviour unchanged): both knobs
        // unset → consume the proposal exactly as before.
        if self.min_lines.is_none() && self.max_lines.is_none() {
            let h = proposal.height.unwrap_or(100.0).max(0.0);
            return (Size::new(w, h)).into();
        }

        // Intrinsic mode: clamp content height to `[min_h, max_h]`
        // where each bound is `n * line_height`. The clamp is a
        // hard cap — we ignore the proposal's height and let the
        // vertical scroll bar take over past `max_lines`.
        let st = self.state.borrow();
        // `default_line_height()` is the *unscaled* line height (its standalone
        // shaper path uses font_scale = 1.0), but `content_height()` carries the
        // engine's font_scale. Scale the per-line bound to match, or a
        // text-scaled editor would clip at `max_lines` / under-size at
        // `min_lines`.
        let line_scale = st.effective_font_scale(ctx.text_scale);
        let line_h = st.engine.default_line_height() * line_scale;
        let content_h = st.engine.content_height();
        drop(st);

        let min_h = self.min_lines.map(|n| n as f32 * line_h).unwrap_or(0.0);
        let max_h = self
            .max_lines
            .map(|n| n as f32 * line_h)
            .unwrap_or(f32::INFINITY);
        let intrinsic_h = content_h.clamp(min_h, max_h);
        Size::new(w, intrinsic_h.max(0.0)).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // The body is a leaf, but the layout walker hands every widget its final
        // bounds here — and layout runs before paint, so this is the earliest
        // (hence authoritative) point at which the viewport can be adopted.
        // `sync_viewport` owns the whole handoff, including `engine.set_viewport`
        // and the relayout flag; paint calls it again as an idempotent echo. See
        // its docs for why the writes must not be split.
        self.state.borrow_mut().sync_viewport(bounds);
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let mut st = self.state.borrow_mut();

        // Sync the engine's default text color with the active theme
        // so dark / light mode swaps reach the rendered glyphs. The
        // engine reads `text_color` fresh on every `render()` and
        // does not bake it into a glyph cache, so a per-paint write
        // is cheap. Skipped when the app pinned a color via
        // `RichTextEditor::text_color(...)`.
        //
        // The render frame DOES cache colors baked into glyph quads,
        // though — the cursor-only and block-only render paths reuse
        // those cached quads. So when the theme colour actually
        // changes, we must dispatch a full render this frame, or the
        // visible glyphs keep painting in the old colour until the
        // next typing / scroll event happens to bump up to a Full
        // path on its own.
        // An app-set `text_color` (Color / role / Signal) is resolved against
        // the active theme each paint; otherwise track the theme's `editor_fg`.
        {
            let new_color = match &st.text_color_prop {
                Some(prop) => prop.resolve(ctx.theme, true).to_array(),
                None => ctx.theme.colors.editor_fg.to_array(),
            };
            st.engine.set_text_color(new_color);
            if st.last_text_color != Some(new_color) {
                st.last_text_color = Some(new_color);
                st.pending_full_render = true;
            }
        }

        // Caret colour: app override resolved each paint, else the theme's
        // `editor_caret` role. The engine defaults the cursor to opaque black,
        // so without this the blinking caret stays black under a dark theme.
        // Cursor decorations are regenerated on every render (the cursor-only
        // path included), so a colour change only needs a render this frame —
        // force one so a swap doesn't wait for the next blink toggle.
        {
            let new_caret = match &st.caret_color_prop {
                Some(prop) => prop.resolve(ctx.theme, true).to_array(),
                None => ctx.theme.colors.editor_caret.to_array(),
            };
            st.engine.set_cursor_color(new_caret);
            if st.last_cursor_color != Some(new_caret) {
                st.last_cursor_color = Some(new_caret);
                st.pending_full_render = true;
            }
        }

        // Selection highlight. A custom colour (set via `.selection_color`) is
        // used as-is and is NOT auto-desaturated when the window goes inactive
        // — matching macOS, where an explicit selection colour opts out of
        // system management. Otherwise the theme drives it, window-aware: the
        // vivid `editor_selection_bg` while the window is active, the muted
        // `selection_bg_inactive` while it is not. Resolved each paint and
        // cached, so a change (theme, custom colour, or window-active flip)
        // just needs a render this frame.
        let new_sel = if let Some(prop) = st.selection_color_prop.as_ref() {
            prop.resolve(ctx.theme, true).to_array()
        } else if ctx.window_active {
            ctx.theme.colors.editor_selection_bg.to_array()
        } else {
            ctx.theme.colors.selection_bg_inactive.to_array()
        };
        if st.last_selection_color != Some(new_sel) {
            st.engine.set_selection_color(new_sel);
            st.last_selection_color = Some(new_sel);
            st.pending_full_render = true;
        }

        // Code block surface colours come from the same theme path
        // (`editor_code_block_bg` / `editor_code_block_fg`). Unlike
        // `text_color`, these are baked into the converted
        // `BlockLayoutParams` at `layout_full` / `relayout_block`
        // time, so the typesetter does NOT pick them up on a render
        // pass — we need a full re-layout when they change. Setting
        // `needs_full_layout = true` schedules that for the same
        // frame; `pending_full_render` covers the render side.
        let new_code_bg = ctx.theme.colors.editor_code_block_bg.to_array();
        let new_code_fg = Some(ctx.theme.colors.editor_code_block_fg.to_array());
        st.engine.set_code_block_background(new_code_bg);
        st.engine.set_code_block_foreground(new_code_fg);
        if st.last_code_block_bg != Some(new_code_bg) || st.last_code_block_fg != new_code_fg {
            st.last_code_block_bg = Some(new_code_bg);
            st.last_code_block_fg = new_code_fg;
            st.needs_full_layout = true;
            st.pending_full_render = true;
        }

        // The engine reads the HiDPI display scale factor from the
        // shared `TypesetterBridge` on every `layout_full`, exactly
        // like `TextWidget` does internally. No widget-side plumbing
        // — this is a render-pipeline concern, invisible to the
        // widget author.

        // Logical font scale: a11y text scale (if followed) × per-editor
        // `font_size_scale`. Baked at `layout_full`, so a change forces a
        // relayout + render this frame.
        {
            let target = st.effective_font_scale(ctx.text_scale);
            if st.last_font_scale.is_nan()
                || (st.last_font_scale - target).abs() > f32::EPSILON
            {
                st.last_font_scale = target;
                st.engine.set_font_scale(target);
                st.needs_full_layout = true;
                st.pending_full_render = true;
            }
        }

        // Idempotent echo — `place_children` already adopted these exact bounds
        // during layout, so this is normally a no-op. It stays so that any path
        // which paints without a preceding layout still sizes the engine.
        st.sync_viewport(bounds);

        // First-frame guard + viewport-change guard: (re)run the
        // full layout so the render call produces glyphs sized
        // for the current bounds. With per-widget `DocumentFlow`
        // state inside the engine, `has_full_layout()` only
        // reports `false` when this widget has never laid out
        // or when the shared service's HiDPI scale factor has
        // changed since the last layout — there is no
        // cross-widget trampling left to guard against.
        //
        // `did_full_layout` is true on this paint iff we just ran
        // `layout_full` above — which means the render frame must
        // be rebuilt from scratch via `with_render_frame`. The
        // incremental render paths (`with_render_block_only`,
        // `with_render_cursor_only`) assume a valid prior full
        // render exists.
        let did_full_layout = st.needs_full_layout || !st.engine.has_full_layout();
        if did_full_layout {
            let flow = st.flow_snapshot();
            st.engine.layout_full(&flow);
            st.needs_full_layout = false;
            st.content_dirty = true;
        }

        // Update the cursor display every paint so selection
        // highlights follow the caret without needing a frame tick.
        // The caret is suppressed in an inactive window for every policy — the
        // authoritative final gate, covering the one frame between a
        // window-active flip and the build-time effect running.
        let caret_on_now = match st.policy.caret_policy {
            CaretPolicy::Hidden => false,
            CaretPolicy::StaticVisible => st.has_focus && st.window_active,
            CaretPolicy::Blinking => st.caret_visible.get() && st.has_focus && st.window_active,
        };
        let cursor_display = bastyde_text::CursorDisplay {
            position: st.cursor.position(),
            anchor: st.cursor.anchor(),
            affinity: st.cursor_affinity,
            visible: caret_on_now,
            selected_cells: Vec::new(),
        };
        st.engine.set_cursor(&cursor_display);

        // Forward the widget's scroll state to the typesetter so
        // viewport culling knows where the visible window is. text-
        // typeset's `render()` only emits glyphs whose flow Y falls
        // inside `[scroll_offset, scroll_offset + viewport_height]`,
        // and the emitted screen coordinates already have
        // `scroll_offset` subtracted — so the paint walker doesn't
        // apply any further offset beyond the widget origin.
        let scroll_y_logical = st.scroll_y.get();
        st.engine.set_scroll_offset(scroll_y_logical);

        // Window the render to the visible clip when opted in (dubious mode).
        // The editor is laid out at its full document height inside an outer
        // ScrollArea, so its own viewport spans the whole document and the
        // viewport-derived cull keeps everything. `ctx.clip_bounds` is the
        // accumulated ancestor clip — the intersection of every clipping
        // ancestor, so this is correct under nested ScrollAreas — mapped into
        // the editor's content space to the band actually on screen. A
        // half-viewport margin each side pre-renders content just off-screen so
        // scrolling never flashes a blank edge. Positioning and hit-testing are
        // untouched: `set_render_window` overrides culling only, and
        // `scroll_offset` stays as set above.
        let render_window = if st.window_to_clip {
            ctx.clip_bounds.map(|clip| {
                // `clip` and `bounds` are screen-space; the render cull works in
                // content space. The visible band's top is the editor's own scroll
                // offset plus however far its top sits above the clip: in dubious
                // mode `scroll_offset` is pinned to 0, but including it keeps the
                // window correct (rather than mis-culling) even for a self-scrolling
                // editor, so this can't silently render the wrong rows.
                let vis_top = (scroll_y_logical + (clip.y - bounds.y)).max(0.0);
                let vis_h = clip.height.max(0.0);
                let margin = vis_h * 0.5;
                ((vis_top - margin).max(0.0), vis_h + 2.0 * margin)
            })
        } else {
            None
        };
        st.engine.set_render_window(render_window);

        // Captured before the split-borrow below (which holds `st` mutably
        // for the rest of the method) so the preedit underline pass can
        // still see them. `cursor_affinity` matches what `caret_rect`
        // queries elsewhere.
        let scroll_x_logical = st.scroll_x.get();
        let ime_preedit_range = st.ime_preedit_range.clone();
        let ime_affinity = st.cursor_affinity;

        // Clip to bounds so overflowing glyphs don't bleed into siblings.
        canvas.set_clip(bounds);

        // Choose the cheapest render path that produces a correct
        // frame for this paint:
        // - Full render: we just rebuilt the layout (no prior frame
        //   to incrementally update), so emit everything from scratch.
        // - Block-only: the frame_loop relayed out exactly one block
        //   since the last paint (single-block edit). Reuse cached
        //   glyphs for the other N-1 blocks.
        // - Cursor-only: nothing structural changed since last
        //   paint — only the cursor blink or selection updated.
        //   Reuses every cached glyph and just refreshes cursor /
        //   selection decorations. Falls back to full render
        //   internally if scroll drifted.
        //
        // Pre-fix, paint() unconditionally called `with_render_frame`,
        // which walked every block on every paint — visible as a
        // ~17% chunk in `rasterize_glyph` / `render_run_glyphs` on
        // the flamegraph because caret blinks and signal updates
        // were forcing a full re-render at ~60 Hz.
        let block_relayout = st.last_relayout_block_id.take();
        let pending_full = std::mem::replace(&mut st.pending_full_render, false);
        enum RenderChoice {
            Full,
            Block(usize),
            CursorOnly,
        }
        // `pending_full` covers the case where `frame_loop::tick`
        // already ran `layout_full` this frame (e.g. on FormatChanged
        // or FlowElementsInserted events from a list-indent edit or
        // Enter key) but cleared `needs_full_layout` before paint ran.
        // Without it, paint would fall through to CursorOnly and the
        // new layout wouldn't render until something else forced a
        // Full pass (resize, scroll out and back into view).
        let choice = if did_full_layout || pending_full {
            RenderChoice::Full
        } else if let Some(bid) = block_relayout {
            RenderChoice::Block(bid)
        } else {
            RenderChoice::CursorOnly
        };

        // Split-borrow the state fields so the paint walker can hold
        // `&engine.with_render_frame(...)`, `&document`, and
        // `&mut image_cache` simultaneously.
        let state_ref: &mut EditorState = &mut st;
        let EditorState {
            ref mut engine,
            ref document,
            ref mut image_cache,
            ..
        } = *state_ref;
        let paint_closure = |frame: &bastyde_text::RenderFrame| {
            paint_frame(
                canvas,
                PaintParams {
                    frame,
                    origin: Point::new(bounds.x, bounds.y),
                    document,
                    image_cache,
                    draw_caret: caret_on_now,
                },
            );
        };
        match choice {
            RenderChoice::Full => engine.with_render_frame(paint_closure),
            RenderChoice::Block(bid) => engine.with_render_block_only(bid, paint_closure),
            RenderChoice::CursorOnly => engine.with_render_cursor_only(paint_closure),
        };

        // IME preedit underline. Walk the composing range char-by-char,
        // emitting one underline segment per visual line so a wrapped
        // composition underlines correctly. Engine coords are content-
        // space; screen = bounds + content − scroll (matches the glyphs).
        // On a read-only viewer there is never a preedit, so this is inert.
        if let Some(range) = ime_preedit_range
            && engine.has_full_layout()
            && range.start < range.end
        {
            let color = ctx.theme.colors.text_primary;
            let underline = |canvas: &mut Canvas, x0: f32, x1: f32, y: f32, h: f32| {
                let uy = y + h - 1.0;
                canvas.draw_line(
                    Point::new(x0, uy),
                    Point::new(x1, uy),
                    color,
                    bastyde_canvas::StrokeStyle::solid(1.0),
                );
            };
            let mut seg_x0: Option<f32> = None;
            let (mut seg_y, mut seg_h, mut last_x) = (0.0_f32, 0.0_f32, 0.0_f32);
            for p in range.start..=range.end {
                let c = engine.caret_rect(p, ime_affinity);
                let x = bounds.x + c[0] - scroll_x_logical;
                let y = bounds.y + c[1] - scroll_y_logical;
                match seg_x0 {
                    None => {
                        seg_x0 = Some(x);
                        seg_y = y;
                        seg_h = c[3];
                        last_x = x;
                    }
                    Some(x0) => {
                        if (y - seg_y).abs() > 0.5 {
                            underline(canvas, x0, last_x, seg_y, seg_h);
                            seg_x0 = Some(x);
                            seg_y = y;
                            seg_h = c[3];
                        }
                        last_x = x;
                    }
                }
            }
            if let Some(x0) = seg_x0 {
                underline(canvas, x0, last_x, seg_y, seg_h);
            }
        }

        canvas.clear_clip();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use self::policy::AccessibilityRole;
        use self::state::SyntheticElementRef;
        use bastyde_core::accesskit::{Action, NodeId, Role};
        use bastyde_text::text_document::{FlowElementSnapshot, FragmentContent};

        let st = self.state.borrow();

        let role = match st.policy.access_role {
            AccessibilityRole::Editor => Role::MultilineTextInput,
            AccessibilityRole::Document => Role::Document,
        };
        builder.set_role(role);
        if st.policy.is_read_only() {
            builder.set_read_only();
        }

        // Walk the cached flow snapshot (or rebuild it if the last
        // edit cleared the cache). For each block we emit a
        // Role::Paragraph child (or Role::Heading when the block's
        // heading_level is set), then for each text fragment we
        // emit a Role::TextRun child carrying value,
        // character_lengths, word_starts, and per-character
        // geometry from text-typeset. Widget-local
        // synthetic_to_element map is populated so the on-access
        // handler can convert AccessKit TextSelection back into
        // document-absolute cursor positions.
        let snap = {
            let mut cache = st.accessibility_flow_snapshot.borrow_mut();
            if cache.is_none() {
                // A bare view (show_highlights=false) builds its AT tree from a
                // clean snapshot too, so screen readers never hear highlight-
                // driven formatting that no sighted user sees. The paint-only
                // overlay is skipped: the AT walk reads fragments, never the
                // overlay, so computing a paint span per spell/find range here
                // would be pure waste (it dominated the a11y rebuild on a large
                // spell-checked document).
                *cache = Some(st.flow_snapshot_for_a11y());
            }
            cache.as_ref().cloned()
        };

        // While composing (IME preedit active), expose the composition as
        // the AT selection so screen readers / braille track the tentative
        // text — the composing characters are already in the runs / value.
        // Falls back to the live cursor/selection otherwise.
        let (user_anchor, user_pos) = match st.ime_preedit_range.clone() {
            Some(range) => (range.start, range.end),
            None => (st.cursor.anchor(), st.cursor.position()),
        };
        let mut caret_pair: Option<(NodeId, usize)> = None;
        let mut anchor_pair: Option<(NodeId, usize)> = None;
        let mut syn_map: std::collections::HashMap<NodeId, SyntheticElementRef> =
            std::collections::HashMap::new();

        if let Some(snap) = snap {
            for elem in &snap.elements {
                if let FlowElementSnapshot::Block(block) = elem {
                    let para_id = builder.push_paragraph_child(block.block_id as u64);
                    if let Some(level) = block.block_format.heading_level {
                        builder.set_paragraph_as_heading(para_id, level);
                    }
                    for frag in &block.fragments {
                        if let FragmentContent::Text {
                            text,
                            offset,
                            length,
                            element_id,
                            word_starts,
                            format,
                            ..
                        } = frag
                        {
                            // Text attributes for AT (WCAG 1.3.1 / EN 301 549
                            // 11.5.2.9): bold / italic / underline / strikethrough
                            // per formatting run. AccessKit has no bold flag, so
                            // an explicit weight wins, else bold folds to 700.
                            let attrs = bastyde_core::accessibility::TextRunAttributes {
                                font_weight: format.font_weight.map(|w| w as u16),
                                bold: format.font_bold.unwrap_or(false),
                                italic: format.font_italic.unwrap_or(false),
                                underline: format.font_underline.unwrap_or(false),
                                strikethrough: format.font_strikeout.unwrap_or(false),
                            };
                            // character_lengths: UTF-8 byte length of each char.
                            // AccessKit indexes by char, each entry is byte count.
                            let char_lengths: Vec<u8> =
                                text.chars().map(|c| c.len_utf8() as u8).collect();

                            // Per-character geometry from text-typeset. char_start
                            // / char_end are block-relative character offsets
                            // (matches LayoutLine::char_range's coordinate space).
                            let char_start = *offset;
                            let char_end = char_start + *length;
                            let geom =
                                st.engine
                                    .character_geometry(block.block_id, char_start, char_end);
                            let char_positions: Vec<f32> =
                                geom.iter().map(|g| g.position).collect();
                            let char_widths: Vec<f32> = geom.iter().map(|g| g.width).collect();

                            let node_id = builder.push_text_run_child(
                                para_id,
                                *element_id,
                                *offset,
                                text.clone(),
                                char_lengths,
                                Some(word_starts.clone()),
                                if char_positions.is_empty() {
                                    None
                                } else {
                                    Some(char_positions)
                                },
                                if char_widths.is_empty() {
                                    None
                                } else {
                                    Some(char_widths)
                                },
                                attrs,
                            );

                            // Remember where this run lives in the document so
                            // the on-access handler can resolve
                            // SetTextSelection(TextRun NodeId, char_index).
                            let absolute_start = block.position + *offset;
                            syn_map.insert(
                                node_id,
                                SyntheticElementRef {
                                    element_id: *element_id,
                                    absolute_start,
                                    text: text.clone(),
                                },
                            );

                            // Resolve user cursor / anchor to this run if they
                            // fall within its absolute character range
                            // [absolute_start, absolute_start + length].
                            let absolute_end = absolute_start + *length;
                            if user_pos >= absolute_start && user_pos <= absolute_end {
                                let char_idx = char_index_in_text(text, user_pos - absolute_start);
                                caret_pair = Some((node_id, char_idx));
                            }
                            if user_anchor >= absolute_start && user_anchor <= absolute_end {
                                let char_idx =
                                    char_index_in_text(text, user_anchor - absolute_start);
                                anchor_pair = Some((node_id, char_idx));
                            }
                        }
                    }
                }
            }
        }

        // Attach the text selection on the editor itself, referencing
        // the appropriate TextRun children. If we couldn't resolve
        // either endpoint (empty document, cursor in no fragment),
        // fall back to a self-targeted selection so screen readers
        // still see *something*.
        if let (Some(a), Some(c)) = (anchor_pair, caret_pair) {
            builder.set_text_selection_to(a, c);
        } else {
            builder.set_text_selection_on_self(user_anchor, user_pos);
        }

        *st.synthetic_to_element.borrow_mut() = syn_map;

        builder.add_action(Action::Focus);
        builder.add_action(Action::ScrollIntoView);
        builder.add_action(Action::SetTextSelection);
        if matches!(st.policy.access_role, AccessibilityRole::Editor) {
            builder.add_action(Action::SetValue);
            builder.add_action(Action::ReplaceSelectedText);
        }
    }

    fn clips_children(&self) -> bool {
        true
    }
}

impl Widget for RichTextEditor {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Engine swap: replace the private fallback with one sharing
        // the application's `SharedTypesetter` so rendered glyphs end
        // up in the atlas bastyde-render uploads to the GPU. Headless
        // tests without a `SharedTypesetter` keep the private engine
        // untouched. Lives on the wrapper because state mutation
        // doesn't depend on `ctx.self_id()`.
        if let Some(shared) = ctx.app_state::<SharedTypesetter>() {
            let mut st = self.state.borrow_mut();
            let wrap = st.wrap_mode;
            // Carry over builder-set engine config that the swap would otherwise
            // drop — `.typography_defaults()`, `.echo_char()` are set on the
            // private engine before mount, and this runs on every rebuild.
            // (Theme colours / font-scale re-derive themselves in `paint()`.)
            let typography = st.engine.typography_defaults().clone();
            let echo = st.engine.echo_char();
            let mut engine = RichTextEngine::from_shared(shared.clone());
            engine.set_wrap_mode(wrap);
            engine.set_hyphenate_justified(true);
            engine.set_typography_defaults(typography);
            engine.set_echo_char(echo);
            st.engine = engine;
            st.needs_full_layout = true;
        }

        // Stash the tree's frame-request handle on the state so the
        // frame-tick effect can self-chain (caret blink, drag
        // auto-scroll) without mutable access to the tree.
        {
            let mut st = self.state.borrow_mut();
            st.frame_request = Some(ctx.frame_request_handle());
            st.frame_wake_at = Some(ctx.wake_at_handle());
            // Remember this build's wrapper id — the `.focusable(true)` node — so a
            // held handle can request focus back onto the editor.
            st.self_id = Some(ctx.self_id());
        }

        // Kick off the first frame so the initial layout/paint runs
        // through the tick path and populates max_scroll / content
        // metrics. Gated by activation: a tab content pane parked in a
        // non-selected `Switcher` branch must not keep the event loop
        // awake just because it was built (TabWidget pre-mounts every
        // open tab).
        let activation = ctx.activation_signal(ctx.self_id());
        if activation.get() {
            ctx.request_frame();
        }

        // When this editor is parked dormant (tab switch, collapsed
        // pane, …) clear local focus state synchronously. The tree may
        // also dispatch FocusLost via revalidate, but a race between
        // selection change and pointer focus — or a programmatic
        // selection change that never moves focus — used to leave
        // `has_focus = true` on every visited tab. Each stuck editor
        // kept scheduling caret `wake_at`s, and every open tab's
        // frame-tick effect still ran on those wakes (observers are
        // not dormancy-gated). Rapid tab switching made CPU climb.
        {
            let state = self.state.clone();
            ctx.effect(&activation, move |&active| {
                if active {
                    return;
                }
                let mut st = state.borrow_mut();
                if st.has_focus {
                    st.has_focus = false;
                    st.focus_signal.set(false);
                }
                if st.caret_visible.get() {
                    st.caret_visible.set(false);
                }
                st.blink.reset();
                // Retire the caret band here too. Only `frame_loop::tick` pushes the band's
                // focus state through to the document, and the tick effect below is skipped
                // entirely while dormant — so a parked editor would keep its last band
                // registered on a document its siblings are still showing, and a split pane
                // over the same document would show two. Clearing `has_focus` above is not
                // enough; nothing would ever act on it.
                if let Some(band) = &st.caret_highlight {
                    band.set_focused(false);
                }
                st.caret_highlight_focused = false;
                // Do not re-arm frame_request here: a dormant editor has
                // nothing to paint, and re-arming is exactly the leak
                // this gate exists to stop.
            });
        }

        // Frame-tick effect — drains document events, blinks the
        // caret, runs drag auto-scroll. Re-arms the tree's
        // frame-request flag while there's still pending work.
        // Skipped entirely while dormant so a multi-tab TabWidget does
        // not pay O(open tabs) per wake for editors nobody can see.
        {
            let state = self.state.clone();
            let active = activation.clone();
            let tick_signal = ctx.frame_tick();
            ctx.effect(&tick_signal, move |delta| {
                if !active.get() {
                    return;
                }
                let mut st = state.borrow_mut();
                let more = frame_loop::tick(&mut st, *delta);
                // Signal::set is unconditional (clones+invokes every
                // observer even when value unchanged), so only call it
                // when the bool actually flipped. Avoids per-tick fanout
                // to chrome widgets that watch the selection state.
                let new_has_selection = st.cursor.has_selection();
                if st.has_selection.get() != new_has_selection {
                    st.has_selection.set(new_has_selection);
                }
                if more && let Some(handle) = &st.frame_request {
                    handle.set(true);
                }
                drop(st);
            });
        }

        // Window-active effect — mirror the tree's window-active state onto the
        // editor state so the frame loop (which has no context) can gate the
        // caret. The frame loop may not tick while the window is inactive (the
        // animation scheduler is parked), so on deactivation we hide the caret
        // *synchronously* here rather than waiting for a tick, and request a
        // frame so the change reaches a paint pass — but only while this
        // editor is itself active. A dormant tab must not re-arm the frame
        // loop just because the host window blinked.
        {
            let state = self.state.clone();
            let active = activation.clone();
            let wa_signal = ctx.window_active_signal();
            ctx.effect(&wa_signal, move |&window_active| {
                let mut st = state.borrow_mut();
                st.window_active = window_active;
                if window_active {
                    // Reactivated: if the editor still holds focus, show the
                    // caret immediately (restart the blink phase) rather than
                    // waiting up to one blink interval. `Hidden` policy stays
                    // hidden — the paint gate suppresses it anyway.
                    let show =
                        st.has_focus && !matches!(st.policy.caret_policy, CaretPolicy::Hidden);
                    if show && !st.caret_visible.get() {
                        st.caret_visible.set(true);
                    }
                    st.blink.reset();
                } else {
                    // Deactivated: hide the caret synchronously (the frame loop
                    // may not tick while the window is inactive).
                    if st.caret_visible.get() {
                        st.caret_visible.set(false);
                    }
                    st.blink.reset();
                }
                if active.get()
                    && let Some(handle) = &st.frame_request
                {
                    handle.set(true);
                }
                drop(st);
            });
        }

        // Attach handlers on the WRAPPER — making the composing
        // widget itself the focus + event target. The body is a
        // pure leaf so users can wrap it in arbitrary chrome via
        // `RichTextEditorStyle::make_body` without losing focus
        // semantics.
        let mut handlers = HandlerSet::new();
        // Editable editors are text-input surfaces — enable the OS IME
        // while focused. Read-only viewers stay focusable for selection but
        // accept no text input, so they leave the IME descriptor unset.
        if !self.state.borrow().policy.is_read_only() {
            handlers = handlers.ime_input(bastyde_core::ime::ImeContext::text());
        }
        handlers = handlers
            .focusable(true)
            .cursor(CursorIcon::Text)
            .on_focus({
                let state = self.state.clone();
                move |gained, ctx| {
                    let mut st = state.borrow_mut();
                    st.has_focus = gained;
                    // Mirror onto the reactive signal so chrome
                    // installed by `RichTextEditorStyle::make_body`
                    // (focus-aware border / ring) re-renders.
                    st.focus_signal.set(gained);
                    if gained && matches!(st.policy.caret_policy, CaretPolicy::Blinking) {
                        st.blink.restart();
                        st.caret_visible.set(true);
                    }
                    drop(st);
                    if gained {
                        // Seed the OS IME candidate area at the caret.
                        self::keyboard::report_ime_cursor_area(&state, ctx);
                    } else {
                        // Abandon any in-progress composition on blur, and drop
                        // the IME-area / caret-chase caches. The OS IME candidate
                        // area is a single *per-window* resource a sibling field
                        // may have re-pointed while we were unfocused; clearing
                        // `last_ime_area` forces the next focus-gain report to
                        // re-seed it (the dedup must not swallow that re-seed).
                        // Clearing `last_chase_pos` lets a refocus re-reveal the
                        // caret even if it has not moved since we lost focus.
                        self::keyboard::clear_ime_preedit(&state);
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
                    self::mouse::handle_pointer_event(&state, &v_sb, &h_sb, event, ctx)
                }
            })
            .on_scroll({
                let state = self.state.clone();
                let overscroll = self.overscroll_behavior;
                move |event, ctx| self::mouse::handle_scroll(&state, overscroll, event, ctx)
            })
            .on_key({
                let state = self.state.clone();
                move |event, ctx| self::keyboard::handle_key(&state, event, ctx)
            })
            .on_double_tap({
                let state = self.state.clone();
                move |event, ctx| self::mouse::handle_double_tap(&state, event.position, ctx)
            })
            .on_triple_tap({
                let state = self.state.clone();
                move |event, ctx| self::mouse::handle_triple_tap(&state, event.position, ctx)
            })
            .on_access_action_request({
                let state = self.state.clone();
                move |action, target_node, data, ctx| {
                    handle_access_action_request(&state, action, target_node, data, ctx)
                }
            });

        // Context-menu factory — same shape as before, just hosted on
        // the wrapper.
        let policy_snapshot = self.state.borrow().policy;
        if let Some(factory) = context_menu::resolve_factory(
            self.custom_context_menu.take(),
            self.default_context_menu_enabled,
            self.state.clone(),
            policy_snapshot,
        ) {
            handlers = handlers.context_menu(move |pos, ctx| factory(pos, ctx));
        }

        ctx.apply_self_handlers(handlers);

        // Build the pure-paint leaf body. The body carries
        // layout/paint/accessibility (using its own `self_id()` for
        // `caret_visible` + `document_version` bindings); the shared
        // `state` propagates handler-driven mutations into it.
        let body = RichTextEditorBody {
            state: self.state.clone(),
            min_lines: self.min_lines,
            max_lines: self.max_lines,
        };
        let viewport_id = ctx.add(body);

        // Reactive colour overrides: a signal/role-bound `ColorProp` must
        // repaint the body (the leaf that resolves + applies them in `paint`)
        // when it changes. Bind to `viewport_id`, not the wrapper — the painter
        // owns its prop bindings (the `RectWidget` pattern). Theme-role changes
        // already dirty every node via the reactive theme; this covers
        // `Signal`-bound props. The background prop is reactive through the
        // `RectWidget` the style builds, so it isn't registered here.
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
                prop.register_if_bound(
                    viewport_id,
                    registry,
                    bastyde_core::binding::BindingLevel::RepaintOnly,
                );
            }
        }

        // Snapshot focus + read-only state for the chrome. `is_focused`
        // is the reactive mirror updated by `on_focus`; `is_read_only`
        // is sampled from the policy bundle.
        let (is_focused, is_read_only) = {
            let st = self.state.borrow();
            (st.focus_signal.clone(), st.policy.is_read_only())
        };

        let style: SharedRichTextEditorStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.rich_text_editor.clone())
            .unwrap_or_else(|| Rc::new(RecipeRichTextEditorStyle));
        let cfg = RichTextEditorStyleConfig {
            viewport: viewport_id,
            is_focused,
            is_read_only,
            content_padding: self.content_padding,
            background: self.state.borrow().background_prop.clone(),
        };
        let root = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root);

        // Overlay scrollbars — floated on top of the chrome at the
        // right / bottom edges. Driven by the same signals the frame
        // loop publishes (`scroll_*`, `max_scroll_*`, `viewport_ratio_*`).
        // ScrollPolicy::AlwaysOff suppresses the widget entirely so it
        // doesn't sit in the children list as a zero-sized stub.
        let (scroll_x, scroll_y, max_scroll_x, max_scroll_y, vr_x, vr_y) = {
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

        let mut children = vec![root];
        if self.v_scroll_policy != ScrollPolicy::AlwaysOff {
            let v_sb = ScrollBar::new(
                ScrollBarOrientation::Vertical,
                scroll_y,
                max_scroll_y.clone(),
                vr_y,
            )
            .visual(ScrollBarVariant::Overlay);
            let v_id = ctx.add(v_sb);
            self.v_scrollbar_id = Some(v_id);
            children.push(v_id);
        }
        if self.h_scroll_policy != ScrollPolicy::AlwaysOff {
            let h_sb = ScrollBar::new(
                ScrollBarOrientation::Horizontal,
                scroll_x,
                max_scroll_x.clone(),
                vr_x,
            )
            .visual(ScrollBarVariant::Overlay);
            let h_id = ctx.add(h_sb);
            self.h_scrollbar_id = Some(h_id);
            children.push(h_id);
        }

        // `place_children` reads `max_scroll_y` / `max_scroll_x`
        // synchronously to decide whether to give the overlay
        // scrollbars a non-zero rect under `ScrollPolicy::Auto`. The
        // frame loop publishes those values from `Step 7` on every
        // tick — without a Relayout binding the wrapper wouldn't
        // re-place its children when the values cross zero, so the
        // bars would stay sized 0×0 until something else (scroll
        // wheel, resize) forced a layout pass.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        max_scroll_y.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::Relayout,
        );
        max_scroll_x.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::Relayout,
        );

        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.root_child_id
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
        // Chrome (first child) fills the entire bounds. Overlay
        // scrollbars float on top at the right (vertical) and
        // bottom (horizontal) edges — collapsed to zero when the
        // axis policy is `Auto` and there's nothing to scroll.
        let sb_thickness = self::frame_loop::SCROLLBAR_THICKNESS;
        {
            // Record the wrapper node's window-space origin so the pointer
            // handlers can reconstruct window coords from the now
            // wrapper-node-local positions (see `State::node_origin`).
            let mut st = self.state.borrow_mut();
            st.node_origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
        }
        let st = self.state.borrow();
        let max_y = st.max_scroll_y.get();
        let max_x = st.max_scroll_x.get();
        drop(st);
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
        for (idx, child) in children.iter_mut().enumerate() {
            if idx == 0 {
                child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
                child.size = Size::new(bounds.width, bounds.height);
            } else if Some(child.id) == self.v_scrollbar_id {
                if show_v {
                    let h = if show_h {
                        (bounds.height - sb_thickness).max(0.0)
                    } else {
                        bounds.height
                    };
                    child.origin = bastyde_canvas::Point::new(
                        bounds.x + bounds.width - sb_thickness,
                        bounds.y,
                    );
                    child.size = Size::new(sb_thickness, h);
                    // Widget-local: pointer events arrive widget-local, so
                    // the published bounds the press-bypass test compares
                    // against must be local too (subtract the widget origin).
                    v_rect = Rect::new(
                        child.origin.x - bounds.x,
                        child.origin.y - bounds.y,
                        sb_thickness,
                        h,
                    );
                } else {
                    child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
                    child.size = Size::ZERO;
                }
            } else if Some(child.id) == self.h_scrollbar_id {
                if show_h {
                    let w = if show_v {
                        (bounds.width - sb_thickness).max(0.0)
                    } else {
                        bounds.width
                    };
                    child.origin = bastyde_canvas::Point::new(
                        bounds.x,
                        bounds.y + bounds.height - sb_thickness,
                    );
                    child.size = Size::new(w, sb_thickness);
                    // Widget-local (see the v_scrollbar branch).
                    h_rect = Rect::new(
                        child.origin.x - bounds.x,
                        child.origin.y - bounds.y,
                        w,
                        sb_thickness,
                    );
                } else {
                    child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
                    child.size = Size::ZERO;
                }
            }
        }
        // Published to the wrapper's `on_pointer_event` so a press over
        // an overlay scrollbar bypasses the drag-select latch — see
        // [`v_scrollbar_bounds`](Self::v_scrollbar_bounds).
        self.v_scrollbar_bounds.set(v_rect);
        self.h_scrollbar_bounds.set(h_rect);
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids = Vec::with_capacity(3);
        if let Some(id) = self.root_child_id {
            ids.push(id);
        }
        if let Some(id) = self.v_scrollbar_id {
            ids.push(id);
        }
        if let Some(id) = self.h_scrollbar_id {
            ids.push(id);
        }
        ids
    }

    fn clips_children(&self) -> bool {
        // Mirror the body's clipping so chrome around the editor
        // doesn't leak the body's overflow.
        true
    }

    fn focus_reveal_rect(&self, _bounds: Rect) -> Option<Rect> {
        // On focus gain the framework reveals the focused widget into any
        // enclosing ScrollArea. Reveal the caret *line*, not the (potentially
        // page-tall, own-scroll-suppressed) whole editor: a click that only
        // placed the caret near the top must not jump the page to the editor's
        // bottom. Returns the exact absolute caret rect the in-page caret-follow
        // uses (viewport_origin + caret − scroll); `scroll_rect_into_view`
        // excludes the editor itself, so this targets the enclosing ScrollArea
        // with no double-scroll. `None` (→ reveal whole bounds) before the first
        // layout or while unfocused.
        self::keyboard::caret_window_rect(&self.state.borrow())
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Transparent container in the AT tree — the inner
        // `RichTextEditorBody` carries the real role
        // (`MultilineTextInput` / `Document`) plus the paragraph and
        // text-run children. Without this method the wrapper would
        // emit a `Role::Unknown` node (the `AccessNodeBuilder`
        // default), which screen readers can't classify. Same
        // pattern as [`TextInput`](crate::TextInput), which also
        // wraps a focusable inner field.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }
}

// ---------------------------------------------------------------------------
// Event handlers — take `&SharedState` so they can be boxed into handler
// closures without borrowing `self`.
// ---------------------------------------------------------------------------

/// Shared body of [`RichTextEditor::reveal_range`] and its
/// [`EditorHandle`] twin — one implementation so the two can never drift on the
/// typewriter-pin rule.
///
/// The pin applies here even though the range is not the caret: with typewriter
/// scrolling on, walking search hits should bring each one to the same height
/// the writer works at. Unlike the caret chase, a pointer anchor does *not*
/// suppress it — the user asked for this jump explicitly by pressing Find Next,
/// so there is no gesture to fight.
fn reveal_range_impl(
    state: &SharedState,
    ctx: &mut bastyde_core::widget::EventContext,
    start: usize,
    end: usize,
) {
    let (area, pin) = {
        let st = state.borrow();
        match self::keyboard::range_window_rect(&st, start, end) {
            Some(a) => (a, st.typewriter),
            None => return,
        }
    };
    match pin {
        Some(fraction) => {
            ctx.ensure_visible_aligned(area, fraction, bastyde_core::event::ScrollMotion::Smooth)
        }
        None => ctx.ensure_visible(area),
    }
}

/// Set (or clear) an editor's ambient caret band. Shared by
/// [`RichTextEditor::set_caret_highlight`] and its [`EditorHandle`] mirror.
///
/// The session is created on first use and torn down when the band is cleared, so an editor
/// that never asks for one registers nothing on the document at all — which matters, since
/// every read-only preview pane shares the documents the writing panes are editing.
fn set_caret_highlight(state: &SharedState, highlight: Option<caret_highlight::CaretHighlight>) {
    let mut st = state.borrow_mut();
    match (&st.caret_highlight, &highlight) {
        (None, None) => return,
        (None, Some(_)) => {
            let session = caret_highlight::CaretHighlightSession::new(&st.document);
            session.set_config(highlight);
            // The frame loop hands it the focus state and the caret on the next tick, so a band
            // switched on mid-session appears without the editor having to be touched.
            session.set_focused(st.has_focus);
            st.caret_highlight_focused = st.has_focus;
            st.caret_highlight = Some(session);
        }
        (Some(_), None) => {
            // Dropping the session retires its highlight layer.
            st.caret_highlight = None;
            st.caret_highlight_focused = false;
        }
        (Some(session), Some(_)) => {
            session.set_config(highlight);
        }
    }
    // A band that appeared, vanished or changed colour needs a frame to draw it — and the
    // resolve-and-push itself only happens in `frame_loop::tick`, so without waking the tree an
    // idle editor stays configured-but-unbanded until some unrelated interaction pumps a frame.
    // Same poke `set_typography_defaults` / `set_font_size_scale` make, for the same reason:
    // these are the ctx-less setters a host calls from a settings or theme effect.
    st.content_dirty = true;
    if let Some(handle) = &st.frame_request {
        handle.set(true);
    }
}

/// Push the current cursor position / anchor / selection flag into
/// the state's reactive signals. Called after every cursor mutation
/// so external observers (status bars, tests) see the change on the
/// next signal propagation. Exported to `keyboard` and `mouse`
/// because every event handler ends with a signal publish.
pub(super) fn sync_cursor_signals(state: &SharedState) {
    let mut st = state.borrow_mut();
    let pos = st.cursor.position();
    let anc = st.cursor.anchor();
    let has_sel = st.cursor.has_selection();
    let pos_sig = st.cursor_position.clone();
    let anc_sig = st.cursor_anchor.clone();
    let sel_sig = st.has_selection.clone();
    let caret_vis_sig = st.caret_visible.clone();
    // Restart the blink phase on every cursor mutation: a steady-visible
    // caret while typing or holding an arrow key, blinking only
    // resumes after the user stops moving. Mirrors focus-gain behavior
    // (see the FocusChanged handler around rich_text.rs:2041). The frame
    // loop only toggles once a full interval has elapsed since the phase
    // start, so restarting here delays the next toggle by a full interval.
    let blink_reset = st.has_focus && matches!(st.policy.caret_policy, CaretPolicy::Blinking);
    if blink_reset {
        st.blink.restart();
    }
    drop(st);
    pos_sig.set(pos);
    anc_sig.set(anc);
    sel_sig.set(has_sel);
    if blink_reset && !caret_vis_sig.get() {
        caret_vis_sig.set(true);
    }
}

/// Dispatch an AccessKit `ActionRequest` payload for the rich text
/// editor. Handles `SetTextSelection` (screen-reader-initiated
/// caret moves), `SetValue` (programmatic text replacement), and
/// `ScrollIntoView` (scroll so the caret is visible).
fn handle_access_action_request(
    state: &SharedState,
    action: bastyde_core::accesskit::Action,
    _target_node: bastyde_core::accesskit::NodeId,
    data: Option<bastyde_core::accesskit::ActionData>,
    ctx: &mut bastyde_core::widget::EventContext,
) -> bastyde_core::event::EventResponse {
    use self::policy::EditCommandKind;
    use bastyde_core::accesskit::{Action, ActionData};
    use bastyde_core::event::EventResponse;
    use bastyde_text::text_document::{MoveMode, SelectionType};

    match (action, data) {
        (Action::SetTextSelection, Some(ActionData::SetTextSelection(sel))) => {
            let filter = state.borrow().policy.command_filter;
            // Screen-reader-initiated caret moves are "navigation",
            // filtered under the same rule as arrow keys.
            if !filter.accepts(EditCommandKind::MoveLeft) {
                return EventResponse::Ignored;
            }
            let resolve = |pos: bastyde_core::accesskit::TextPosition| -> Option<usize> {
                let st = state.borrow();
                let map = st.synthetic_to_element.borrow();
                let er = map.get(&pos.node)?.clone();
                // Convert character_index (char units within the run)
                // to a byte offset within the run's text, then add
                // absolute_start to get the document position.
                let byte_off = er
                    .text
                    .char_indices()
                    .nth(pos.character_index)
                    .map(|(i, _)| i)
                    .unwrap_or(er.text.len());
                Some(er.absolute_start + byte_off)
            };
            if let (Some(a), Some(f)) = (resolve(sel.anchor), resolve(sel.focus)) {
                let st = state.borrow();
                st.cursor.set_position(a, MoveMode::MoveAnchor);
                st.cursor.set_position(f, MoveMode::KeepAnchor);
                drop(st);
                sync_cursor_signals(state);
                ctx.request_frame();
                EventResponse::Handled
            } else {
                EventResponse::Ignored
            }
        }
        (Action::SetValue, Some(ActionData::Value(value))) => {
            let filter = state.borrow().policy.command_filter;
            if !filter.accepts(EditCommandKind::InsertChar) {
                return EventResponse::Ignored;
            }
            let st = state.borrow();
            st.cursor.select(SelectionType::Document);
            let _ = st.cursor.insert_text(value.as_ref());
            drop(st);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        (Action::ReplaceSelectedText, Some(ActionData::Value(value))) => {
            // Insert at the caret, replacing the active selection (if
            // any) — NOT the whole document like `SetValue`. The AT-SPI
            // (Linux) / UIA (Windows) braille-keyboard & dictation
            // insertion path; macOS routes insertion through `SetValue`.
            // We advertise the action in `accessibility()`, so service it.
            let filter = state.borrow().policy.command_filter;
            if !filter.accepts(EditCommandKind::InsertChar) {
                return EventResponse::Ignored;
            }
            let st = state.borrow();
            let _ = st.cursor.insert_text(value.as_ref());
            drop(st);
            sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        (Action::ScrollIntoView, _) => {
            let mut st = state.borrow_mut();
            if let Some(new_y) = st.engine.ensure_caret_visible() {
                st.scroll_y.set(new_y);
            }
            drop(st);
            ctx.request_frame();
            EventResponse::Handled
        }
        _ => EventResponse::Ignored,
    }
}

/// Convert an intra-fragment byte offset into a character index.
/// Used by `accessibility()` to map the user's document-absolute
/// cursor position into AccessKit's `TextPosition.character_index`
/// (which indexes into the target TextRun's `character_lengths`,
/// i.e., one entry per Rust `char`).
fn char_index_in_text(text: &str, byte_offset: usize) -> usize {
    // Walk char_indices until we pass byte_offset; the count at
    // that point is the character index. Fall back to the char
    // count when byte_offset >= text.len().
    if byte_offset >= text.len() {
        return text.chars().count();
    }
    let mut count = 0usize;
    for (i, _) in text.char_indices() {
        if i >= byte_offset {
            return count;
        }
        count += 1;
    }
    count
}
