// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Rich text editor widget.
//!
//! See [`§27.10` of the architecture doc](../../../../../docs/architecture.md)
//! for the design rationale. Two construction presets:
//! [`RichTextEditor::read_only`] (view documents, select/copy, click links)
//! and [`RichTextEditor::editor`] (full editing).
//!
//! The widget owns its own `bastyde_text::RichTextEngine` (per-widget
//! typesetter), subscribes to document events via `on_change` so
//! multiple editors can share a `TextDocument` like QTextEdit views,
//! and drives its own scroll bars outside of `ScrollArea` to break the
//! wrap/scrollbar circular dependency of §27.10.5.
//!
//! Constructors: [`RichTextEditor::read_only`] (hidden caret, filter
//! rejects mutations, accessibility role `Document`) and
//! [`RichTextEditor::editor`] (blinking caret, full command filter,
//! role `MultilineTextInput`, `SetValue` action declared). Both
//! widgets subscribe to `TextDocument::on_change` independently so
//! any number of editors / viewers can share a document and observe
//! each other's edits independently.
//!
//! This file owns the struct, its builder methods and signal
//! accessors, `Widget` trait impl (`build` / `size_that_fits` /
//! `place_children` / `paint` / `accessibility`), and the shared
//! `sync_cursor_signals` helper used by both `keyboard` and `mouse`
//! dispatch modules. Key / pointer / gesture handlers live in
//! `keyboard` and `mouse`; the frame-tick loop lives in
//! `frame_loop`; clipboard actions in `clipboard`.

mod clipboard;
mod context_menu;
mod frame_loop;
mod hit_test;
pub(crate) mod image_cache;
mod keyboard;
mod mouse;
pub(crate) mod paint;
mod policy;
mod state;

#[cfg(test)]
mod tests;

pub use context_menu::{
    INTENT_COPY, INTENT_CUT, INTENT_PASTE, INTENT_PASTE_UNFORMATTED, INTENT_SELECT_ALL,
};
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
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    RichTextEditorStyle, RichTextEditorStyleConfig, SharedRichTextEditorStyle,
};
use bastyde_core::widget::{CursorIcon, LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_text::text_document::{
    Alignment, BlockFormat, ListStyle, MoveMode, SelectionType, TextDocument, TextFormat,
};
use bastyde_text::{FontRegistrar, RichTextEngine, SharedTypesetter, WrapMode};
use bastyde_tokens::Color;

use self::paint::{PaintParams, paint_frame};
use self::state::{EditorState, SharedState};
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVariant};
use crate::styles::RecipeRichTextEditorStyle;

/// Scrollbar visibility policy, applied independently per axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollPolicy {
    /// Visible only when the corresponding `max_scroll_axis > 0`.
    #[default]
    Auto,
    /// Always visible (reserves gutter width even when content fits).
    AlwaysOn,
    /// Never rendered. Useful when embedding the editor in an outer
    /// scroll container, or in tests.
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

    /// Set the initial zoom factor (`1.0` = 100 %). Applied before the first
    /// layout pass. Use [`set_zoom_level`](Self::set_zoom_level) after the
    /// widget is mounted.
    pub fn zoom(self, zoom: f32) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.engine.set_zoom(zoom);
            st.needs_full_layout = true;
        }
        self
    }

    /// Override the selection-highlight color. Defaults to the theme's
    /// selection color when not set.
    pub fn selection_color(self, color: Color) -> Self {
        self.state
            .borrow_mut()
            .engine
            .set_selection_color(color.to_array());
        self
    }

    /// Override the caret / insertion-point color.
    pub fn caret_color(self, color: Color) -> Self {
        self.state
            .borrow_mut()
            .engine
            .set_cursor_color(color.to_array());
        self
    }

    /// Pin the default text color, bypassing theme-driven updates. Once set,
    /// dark / light mode changes no longer affect glyph color for this
    /// editor. Omit to track the active theme automatically.
    pub fn text_color(self, color: Color) -> Self {
        let mut st = self.state.borrow_mut();
        st.engine.set_text_color(color.to_array());
        st.text_color_user_set = true;
        drop(st);
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

    /// Set the same scroll-bar visibility policy on both axes.
    pub fn scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.v_scroll_policy = policy;
        self.h_scroll_policy = policy;
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
    /// (origin at the widget's top-left, scroll offset and zoom handled
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

    // --- Runtime zoom -----------------------------------------------------

    /// Set the editor's zoom level. Re-lays out immediately; triggers
    /// a repaint via the engine's dirty tracking on the next frame.
    pub fn set_zoom_level(&self, zoom: f32) {
        let mut st = self.state.borrow_mut();
        st.engine.set_zoom(zoom.clamp(0.1, 10.0));
        st.needs_full_layout = true;
        st.content_dirty = true;
    }

    /// Current zoom level.
    pub fn get_zoom_level(&self) -> f32 {
        self.state.borrow().engine.zoom()
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

    // --- Reactive signal accessors -----------------------------------------

    /// Bumps on every format-only document event (bold / italic /
    /// heading / alignment / list-style changes). See
    /// [`RichTextEditor::format_version`].
    pub fn format_version(&self) -> Signal<u64> {
        self.state.borrow().format_version.clone()
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
        // a full render automatically when scroll/zoom drifted since
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
            cursor_position.bind_to(
                self_id,
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::RepaintOnly,
            );
            cursor_anchor.bind_to(
                self_id,
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::RepaintOnly,
            );
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
        _ctx: &LayoutContext,
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
        let line_h = st.engine.default_line_height();
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
        // Record the viewport for the frame loop to read next tick.
        // The rich text editor is currently a leaf — scroll bar
        // siblings (§27.10.5's "scrollbars outside ScrollArea" trick)
        // are a future addition. For now `paint()` is the only
        // place `viewport_width / height` get reliably refreshed
        // (since leaf widgets may not call `place_children`), and
        // this path is the fallback when a parent does call us.
        let mut st = self.state.borrow_mut();
        st.viewport_width = bounds.width;
        st.viewport_height = bounds.height;
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
        if !st.text_color_user_set {
            let new_color = ctx.theme.colors.editor_fg.to_array();
            st.engine.set_text_color(new_color);
            if st.last_text_color != Some(new_color) {
                st.last_text_color = Some(new_color);
                st.pending_full_render = true;
            }
        }

        // Sync the caret colour with the theme's `editor_caret` role the
        // same way. The engine defaults the cursor to opaque black, so
        // without this the blinking caret stays black under a dark theme.
        // Cursor decorations are regenerated on every render (the
        // cursor-only path included), so a colour change only needs a
        // render this frame — force one so a theme swap doesn't wait for
        // the next blink toggle to repaint the caret.
        {
            let new_caret = ctx.theme.colors.editor_caret.to_array();
            st.engine.set_cursor_color(new_caret);
            if st.last_cursor_color != Some(new_caret) {
                st.last_cursor_color = Some(new_caret);
                st.pending_full_render = true;
            }
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

        // `RichTextEditorBody` is a leaf, so the framework never calls
        // `place_children` on it. Sync the viewport from paint bounds,
        // flag a relayout if the size changed, and record the window-space
        // origin so pointer handlers can convert to widget-local coordinates.
        st.viewport_origin = Point::new(bounds.x, bounds.y);
        let viewport_changed = (st.viewport_width - bounds.width).abs() > 0.5
            || (st.viewport_height - bounds.height).abs() > 0.5;
        if viewport_changed {
            st.viewport_width = bounds.width;
            st.viewport_height = bounds.height;
            st.engine.set_viewport(bounds.width, bounds.height);
            st.needs_full_layout = true;
        }

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
        let caret_on_now = match st.policy.caret_policy {
            CaretPolicy::Hidden => false,
            CaretPolicy::StaticVisible => st.has_focus,
            CaretPolicy::Blinking => st.caret_visible.get() && st.has_focus,
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
        //   internally if scroll or zoom drifted.
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
                // driven formatting that no sighted user sees.
                *cache = Some(st.flow_snapshot());
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
                            ..
                        } = frag
                        {
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
            let mut engine = RichTextEngine::from_shared(shared.clone());
            engine.set_wrap_mode(wrap);
            engine.set_hyphenate_justified(true);
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
        }

        // Kick off the first frame so the initial layout/paint runs
        // through the tick path and populates max_scroll / content
        // metrics.
        ctx.request_frame();

        // Frame-tick effect — drains document events, blinks the
        // caret, runs drag auto-scroll. Re-arms the tree's
        // frame-request flag while there's still pending work.
        {
            let state = self.state.clone();
            let tick_signal = ctx.frame_tick();
            ctx.effect(&tick_signal, move |delta| {
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
                        st.blink_last_toggle = Some(std::time::Instant::now());
                        st.caret_visible.set(true);
                    }
                    drop(st);
                    if gained {
                        // Seed the OS IME candidate area at the caret.
                        self::keyboard::report_ime_cursor_area(&state, ctx);
                    } else {
                        // Abandon any in-progress composition on blur.
                        self::keyboard::clear_ime_preedit(&state);
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
                move |event, ctx| self::mouse::handle_scroll(&state, event, ctx)
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
    // Reset the blink phase on every cursor mutation: a steady-visible
    // caret while typing or holding an arrow key, blinking only
    // resumes after the user stops moving. Mirrors focus-gain behavior
    // (see the FocusChanged handler around rich_text.rs:2041). The
    // frame loop reads `blink_last_toggle` each tick and toggles only
    // after CARET_BLINK_INTERVAL elapses, so resetting it here delays
    // the next toggle by a full interval.
    let blink_reset = st.has_focus && matches!(st.policy.caret_policy, CaretPolicy::Blinking);
    if blink_reset {
        st.blink_last_toggle = Some(std::time::Instant::now());
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
