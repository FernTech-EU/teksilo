//! Rich text editor widget. Feature-gated behind the `rich-text` feature.
//!
//! See [`§27.10` of the architecture doc](../../../../../docs/architecture.md)
//! for the design rationale. This crate ships `RichTextEditor` with two
//! construction presets — M8a provides [`RichTextEditor::read_only`]
//! (view documents, select/copy, click links). M8b will add
//! [`RichTextEditor::editor`] (full editing).
//!
//! The widget owns its own `bastyde_text::RichTextEngine` (per-widget
//! typesetter — see gap 5 of the plan), subscribes to document events
//! via `on_change` so multiple editors can share a `TextDocument` like
//! QTextEdit views, and drives its own scroll bars outside of
//! `ScrollArea` to break the wrap/scrollbar circular dependency of
//! §27.10.5.
//!
//! Constructors: [`RichTextEditor::read_only`] (hidden caret, filter
//! rejects mutations, accessibility role `Document`) and
//! [`RichTextEditor::editor`] (blinking caret, full command filter,
//! role `MultilineTextInput`, `SetValue` action declared). Both
//! widgets subscribe to `TextDocument::on_change` independently so
//! any number of editors / viewers can share a document and observe
//! each other's edits — see gap 10 of the plan.
//!
//! This file owns the struct, its builder methods and signal
//! accessors, `Widget` trait impl (`build` / `size_that_fits` /
//! `place_children` / `paint` / `accessibility`), and the shared
//! `sync_cursor_signals` helper used by both `keyboard` and `mouse`
//! dispatch modules. Key / pointer / gesture handlers live in
//! [`keyboard`] and [`mouse`]; the frame-tick loop lives in
//! [`frame_loop`]; clipboard actions in [`clipboard`].

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
/// (M8a) or [`RichTextEditor::editor`] (M8b, currently stubbed as
/// `unimplemented!`).
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
        Self::construct(document, READ_ONLY_PRESET)
    }

    /// Construct an editable rich text editor bound to `document`.
    /// Uses the full editor preset: every command accepted, caret
    /// blinks, `MultilineTextInput` accessibility role, full clipboard
    /// support. Multiple editors on the same document share live edits
    /// via per-widget `on_change` subscriptions — see §27.10.1 of the
    /// architecture doc.
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

    // --- Builder methods ------------------------------------------------

    pub fn wrap_mode(self, mode: WrapMode) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.wrap_mode = mode;
            st.engine.set_wrap_mode(mode);
            st.needs_full_layout = true;
        }
        self
    }

    pub fn zoom(self, zoom: f32) -> Self {
        {
            let mut st = self.state.borrow_mut();
            st.engine.set_zoom(zoom);
            st.needs_full_layout = true;
        }
        self
    }

    pub fn selection_color(self, color: Color) -> Self {
        self.state
            .borrow_mut()
            .engine
            .set_selection_color(color.to_array());
        self
    }

    pub fn caret_color(self, color: Color) -> Self {
        self.state
            .borrow_mut()
            .engine
            .set_cursor_color(color.to_array());
        self
    }

    pub fn text_color(self, color: Color) -> Self {
        let mut st = self.state.borrow_mut();
        st.engine.set_text_color(color.to_array());
        st.text_color_user_set = true;
        drop(st);
        self
    }

    pub fn v_scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.v_scroll_policy = policy;
        self
    }

    pub fn h_scroll_policy(mut self, policy: ScrollPolicy) -> Self {
        self.h_scroll_policy = policy;
        self
    }

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
            st.engine = engine;
            st.needs_full_layout = true;
        }
        self
    }

    // --- Observable signals ---------------------------------------------

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

    pub fn scroll_y(&self) -> Signal<f32> {
        self.state.borrow().scroll_y.clone()
    }

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
    /// existing selection (passes [`MoveMode::MoveAnchor`]).
    pub fn set_caret_position(&self, position: usize) {
        {
            let st = self.state.borrow();
            st.cursor.set_position(position, MoveMode::MoveAnchor);
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
    /// [`rich_text/clipboard.rs`](crate::rich_text::clipboard).
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
    /// href string and the active [`EventContext`].
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
    /// active [`EventContext`].
    pub fn on_image_activated(
        self,
        handler: impl Fn(&str, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.state.borrow_mut().on_image_activated = Some(std::rc::Rc::new(handler));
        self
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
        // is cheap and produces correct output on the next frame
        // without forcing a relayout. Skipped when the app pinned a
        // color via `RichTextEditor::text_color(...)`.
        if !st.text_color_user_set {
            st.engine
                .set_text_color(ctx.theme.colors.editor_fg.to_array());
        }

        // The engine reads the HiDPI display scale factor from the
        // shared `TypesetterBridge` on every `layout_full`, exactly
        // like `TextWidget` does internally. No widget-side plumbing
        // — this is a render-pipeline concern, invisible to the
        // widget author.

        // Bug-fix: `place_children` is only called when a widget has
        // children, so for the M8a leaf editor the viewport was never
        // recorded on the state. Pull it from the paint bounds now,
        // flag a relayout if it changed, and update the typesetter.
        // Also record the widget's origin in window space — event
        // handlers (click / hit-test) subtract this from the incoming
        // pointer position to obtain widget-local coordinates.
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
        if st.needs_full_layout || !st.engine.has_full_layout() {
            let flow = st.document.snapshot_flow();
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

        // Clip to bounds so overflowing glyphs don't bleed into siblings.
        canvas.set_clip(bounds);

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
        engine.with_render_frame(|frame| {
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
        });

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
                *cache = Some(st.document.snapshot_flow());
            }
            cache.as_ref().cloned()
        };

        let user_pos = st.cursor.position();
        let user_anchor = st.cursor.anchor();
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
                st.has_selection.set(st.cursor.has_selection());
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
                    ctx.request_frame();
                }
            })
            .on_pointer_event({
                let state = self.state.clone();
                move |event, ctx| self::mouse::handle_pointer_event(&state, event, ctx)
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
        };
        let root = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root);
        vec![root]
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
        for child in children.iter_mut() {
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
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
    let st = state.borrow();
    let pos = st.cursor.position();
    let anc = st.cursor.anchor();
    let has_sel = st.cursor.has_selection();
    let pos_sig = st.cursor_position.clone();
    let anc_sig = st.cursor_anchor.clone();
    let sel_sig = st.has_selection.clone();
    drop(st);
    pos_sig.set(pos);
    anc_sig.set(anc);
    sel_sig.set(has_sel);
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
