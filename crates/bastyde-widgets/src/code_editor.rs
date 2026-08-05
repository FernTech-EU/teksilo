// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Multi-line plain-text and code editing surfaces.
//!
//! Three faces over one core:
//!
//! - `CodeEditor` — a source editor: gutter, current-line highlight,
//!   indentation, bracket handling, multiple carets.
//! - `PlainTextEditor` — the same core with the code affordances off and
//!   wrapping on: a notes field, a commit message, a description box.
//! - `LogView` — read-only, append-only, tail-following.
//!
//! They are one implementation because they differ in *configuration*, not in
//! kind. All three are a monospaced-or-not run of lines with a caret in it; a
//! separate widget per face would triplicate the caret, selection, IME,
//! clipboard, scrolling, and accessibility and let them drift.
//!
//! # Why not `RichTextEditor`
//!
//! `RichTextEditor` already edits multi-line text, and this deliberately does
//! not build on it. Its command vocabulary is tables, lists, blockquotes, and
//! bold — reusing it would put Tab-navigates-a-table-cell and
//! Ctrl+B-emboldens into a source file, where the first is wrong and the second
//! is meaningless. Its state carries a table-aware Ctrl+A ladder and a rich
//! clipboard fragment; this one carries an indent policy and a caret vector.
//! The overlap is real but it is the *clock* — the caret blink, the debounce
//! window, the scroll arithmetic — and that lives in the crate-internal
//! `common::editor_runtime`, shared by both.
//!
//! # Language-agnostic by construction
//!
//! There is no `Language` enum here. Comment tokens, bracket pairs, indent
//! width, and highlighting are [`CodeConfig`] values the application supplies:
//! the editor knows how to toggle a line comment, not that Rust uses `//`.
//! Guessing would be worse than not knowing — inserting `//` into a Python file
//! corrupts it silently.

mod a11y;
mod clipboard;
mod completion;
mod config;
mod frame_loop;
mod gutter;
mod keyboard;
mod log_stream;
mod log_view;
mod mouse;
mod policy;
mod semantics;
mod state;
mod widget;

#[cfg(test)]
mod tests;

pub use completion::{CompletionContext, CompletionItem, CompletionKind};
pub use config::{BracketPair, COMMON_BRACKETS, CodeConfig, IndentStyle};
pub use log_view::{LogView, LogViewHandle};
pub use policy::{CODE_EDITOR_PRESET, CODE_READ_ONLY_PRESET, CodeCommand};
pub use widget::{CodeEditor, PlainTextEditor};

use std::rc::Rc;

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_text::text_document::TextDocument;
use bastyde_text::{RichTextEngine, SharedTypesetter, WrapMode};

use self::state::{CodeEditorState, SharedState};
use crate::common::editor_runtime::PolicyBundle;
use crate::rich_text::paint::{PaintParams, paint_frame};

/// The paint-only leaf that renders the document.
///
/// Split from the wrapper for the same reason the rich text editor is: the
/// wrapper owns focus, handlers, and style-supplied chrome, so the body can be
/// a pure leaf that an application's custom style may place anywhere inside its
/// decoration without the focus semantics moving with it. The two are joined
/// only by the shared state — neither holds a reference to the other.
pub(crate) struct CodeEditorBody {
    state: SharedState,
    min_lines: Option<u32>,
    max_lines: Option<u32>,
}

impl std::fmt::Debug for CodeEditorBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeEditorBody")
            .field("policy", &self.state.borrow().policy)
            .finish_non_exhaustive()
    }
}

impl Widget for CodeEditorBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use bastyde_core::binding::BindingLevel;

        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();

        let st = self.state.borrow();

        // The caret is painted here, so its every toggle must mark *this* node
        // for repaint. Skipped when the policy never draws one.
        if st.policy.caret_policy != crate::common::editor_runtime::CaretPolicy::Hidden {
            st.caret_visible
                .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        }

        // An edit must both repaint and re-walk the accessibility tree — this
        // body is the node carrying the role and the paragraph/run children.
        st.document_version
            .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        st.document_version
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);

        // The completion popup's open/selection state rides on this node's a11y
        // (expanded / controls / active_descendant), so re-walk when it changes.
        st.completion
            .open
            .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        st.completion
            .selected
            .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);

        // Scroll never changes the AT tree, so it is repaint-only.
        for sig in [&st.scroll_x, &st.scroll_y] {
            sig.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        }
        // Caret and selection are repaint-only for geometry — they never change
        // this widget's size — but they ALSO change what the a11y walk reports
        // via `set_text_selection_to`. A caret-only move (arrow key, click,
        // drag-select) emits no document event, so `document_version` never
        // bumps; without an `AccessibilityOnly` binding here `a11y_dirty` would
        // never flip and a screen reader would hear the caret frozen at the last
        // edit. Binding one signal at two levels is the same pattern
        // `document_version` uses above. `has_selection` is derived from the
        // caret and anchor, so binding those two covers every selection change.
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
        // A caret added or removed changes what is drawn but not the layout.
        st.caret_count
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);

        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let w = proposal.width.unwrap_or(200.0).max(0.0);

        // Greedy: fill whatever we are given. The editor is normally the
        // scrollable region of a pane, so it takes the space and scrolls.
        if self.min_lines.is_none() && self.max_lines.is_none() {
            let h = proposal.height.unwrap_or(100.0).max(0.0);
            return Size::new(w, h).into();
        }

        // Intrinsic: size to content, clamped to [min_lines, max_lines] — the
        // composer pattern, where the field grows with what is typed until it
        // is allowed to grow no further and starts scrolling.
        let st = self.state.borrow();
        let line_scale = if st.follow_text_scale {
            ctx.text_scale
        } else {
            1.0
        };
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
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // A leaf, but layout runs before paint, so this is the earliest — and
        // therefore authoritative — point at which the viewport is known.
        self.state.borrow_mut().sync_viewport(bounds);
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        use crate::common::editor_runtime::CaretPolicy;

        let mut st = self.state.borrow_mut();

        // Resolve the app's colour overrides against the live theme each paint,
        // so a theme swap or a Signal-bound colour reaches the glyphs. A
        // changed colour forces a full render because the cached glyph quads
        // have the old colour baked in.
        let new_text = match &st.text_color_prop {
            Some(p) => p.resolve(ctx.theme, true).to_array(),
            None => ctx.theme.colors.editor_fg.to_array(),
        };
        st.engine.set_text_color(new_text);
        if st.last_text_color != Some(new_text) {
            st.last_text_color = Some(new_text);
            st.pending_full_render = true;
        }

        let new_caret = match &st.caret_color_prop {
            Some(p) => p.resolve(ctx.theme, true).to_array(),
            None => ctx.theme.colors.editor_caret.to_array(),
        };
        st.engine.set_cursor_color(new_caret);
        if st.last_cursor_color != Some(new_caret) {
            st.last_cursor_color = Some(new_caret);
            st.pending_full_render = true;
        }

        // Selection desaturates in an inactive window unless the app pinned a
        // colour — the same convention every desktop selection follows.
        let new_sel = if let Some(p) = st.selection_color_prop.as_ref() {
            p.resolve(ctx.theme, true).to_array()
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

        // Logical font scale: a11y × per-editor `font_size_scale`. Changes
        // glyph advances, so it forces a relayout, not just a re-render.
        let target_scale = st.effective_font_scale(ctx.text_scale);
        if st.last_font_scale.is_nan() || (st.last_font_scale - target_scale).abs() > f32::EPSILON {
            st.last_font_scale = target_scale;
            st.engine.set_font_scale(target_scale);
            st.needs_full_layout = true;
            st.pending_full_render = true;
        }

        // Idempotent echo of place_children, which already adopted these exact
        // bounds. Kept because paint is reachable on a first frame where layout
        // has run but the frame loop has not yet ticked.
        st.sync_viewport(bounds);

        let did_full_layout = st.needs_full_layout || !st.engine.has_full_layout();
        if did_full_layout {
            let flow = st.document.snapshot_flow();
            st.engine.layout_full(&flow);
            st.needs_full_layout = false;
            st.content_dirty = true;
        }

        let caret_on = match st.policy.caret_policy {
            CaretPolicy::Hidden => false,
            CaretPolicy::StaticVisible => st.has_focus && st.window_active,
            CaretPolicy::Blinking => st.caret_visible.get() && st.has_focus && st.window_active,
        };

        // Publish every caret to the engine in one call. A single-caret editor
        // is just the one-element case, so there is no second code path to keep
        // in step.
        let cursors: Vec<bastyde_text::CursorDisplay> = st
            .all_carets()
            .map(|c| bastyde_text::CursorDisplay {
                position: c.position(),
                anchor: c.anchor(),
                affinity: st.cursor_affinity,
                visible: caret_on,
                selected_cells: Vec::new(),
            })
            .collect();
        st.engine.set_cursors(&cursors);

        let scroll_y = st.scroll_y.get();
        st.engine.set_scroll_offset(scroll_y);

        // Cull the render to the visible clip band when the editor is laid out at
        // full document height inside an outer scroller (opt-in, off by default).
        // `clip_bounds` is the on-screen slice an ancestor clip leaves visible; we
        // map it into content space and render only that band (plus a half-viewport
        // margin), the same window the rich text editor uses. Never moves glyph
        // positions or hit-testing — it only limits what is emitted.
        let render_window = if st.window_to_clip {
            ctx.clip_bounds.map(|clip| {
                let vis_top = (scroll_y + (clip.y - bounds.y)).max(0.0);
                let vis_h = clip.height.max(0.0);
                let margin = vis_h * 0.5;
                ((vis_top - margin).max(0.0), vis_h + 2.0 * margin)
            })
        } else {
            None
        };
        st.engine.set_render_window(render_window);

        canvas.set_clip(bounds);

        let pending_full = std::mem::replace(&mut st.pending_full_render, false);
        let block_relayout = st.last_relayout_block_id.take();

        let state_ref: &mut CodeEditorState = &mut st;
        let CodeEditorState {
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
                    // No inline images on this surface, so none can be missing.
                    image_resolver: None,
                    selection: None,
                    selection_color: [0.0; 4],
                    selected_image_out: None,
                    resize_preview: None,
                    draw_caret: caret_on,
                },
            );
        };
        if did_full_layout || pending_full {
            engine.with_render_frame(paint_closure);
        } else if let Some(bid) = block_relayout {
            engine.with_render_block_only(bid, paint_closure);
        } else {
            engine.with_render_cursor_only(paint_closure);
        }

        canvas.clear_clip();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        let st = self.state.borrow();

        // Role, read-only, the paragraph/run tree, selection reporting, and the
        // text actions — the walk shared with the log view (full-document here).
        a11y::build_editor_a11y(&st, builder);

        // Completion popup — the ARIA combobox-with-listbox pattern (as ComboBox
        // and SearchField): the editor keeps focus and carries has-popup +
        // autocomplete, announces expanded (both branches, so it never sticks
        // open), and — only while shown, or a stale reference can crash a screen
        // reader — points controls at the listbox and active-descendant at the
        // highlighted row.
        if st.completion.has_provider() {
            use bastyde_core::accessibility::widget_id_to_node_id;
            use bastyde_core::accesskit::{AutoComplete, HasPopup};

            let inner = builder.inner_mut();
            inner.set_has_popup(HasPopup::Listbox);
            inner.set_auto_complete(AutoComplete::List);
            let open = st.completion.is_open();
            inner.set_expanded(open);
            if open {
                if let Some(pid) = st.completion.panel_id {
                    inner.push_controlled(widget_id_to_node_id(pid));
                }
                if let Some(row) = st.completion.active_row.get() {
                    inner.set_active_descendant(widget_id_to_node_id(row));
                }
            }
        }
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// Shared construction for every face of the editor.
///
/// Returns the state handle; the public builders wrap it. Keeping this one
/// function is what makes `CodeEditor` / `PlainTextEditor` / `LogView`
/// genuinely the same core rather than three that merely look alike.
pub(crate) fn construct(
    document: TextDocument,
    policy: PolicyBundle,
    config: CodeConfig,
    wrap_mode: WrapMode,
) -> SharedState {
    // A private engine to begin with. `build()` swaps in one sharing the
    // application's typesetter when there is one, so glyphs land in the atlas
    // the renderer uploads; headless tests have no typesetter and the private
    // engine is then exactly right, since no renderer is ever invoked.
    let mut engine = RichTextEngine::private_default();
    engine.set_wrap_mode(wrap_mode);
    // No hyphenation, ever: it is a prose affordance, and hyphenating source
    // code would break identifiers across lines.
    CodeEditorState::new(document, engine, policy, config, wrap_mode)
}

/// Swap the private engine for one sharing the application's typesetter.
///
/// Called from the wrapper's `build`. Outside a windowed app there is no
/// typesetter and the private engine stays, which is why the headless tests
/// exercise the same paths.
pub(crate) fn adopt_shared_typesetter(state: &SharedState, ctx: &mut BuildContext) {
    let Some(shared) = ctx.app_state::<SharedTypesetter>() else {
        return;
    };
    let mut st = state.borrow_mut();
    let wrap = st.wrap_mode;
    let typography = st.engine.typography_defaults().clone();
    let mut engine = RichTextEngine::from_shared(shared.clone());
    engine.set_wrap_mode(wrap);
    engine.set_typography_defaults(typography);
    st.engine = engine;
    st.needs_full_layout = true;
}

/// Build the paint-only body for a state handle.
pub(crate) fn body_for(
    state: &SharedState,
    min_lines: Option<u32>,
    max_lines: Option<u32>,
) -> CodeEditorBody {
    CodeEditorBody {
        state: state.clone(),
        min_lines,
        max_lines,
    }
}

/// Publish cursor state onto the reactive signals.
///
/// Every mutating path ends here. The signals are written *after* the state
/// borrow is dropped: `Signal::set` fans out to observers synchronously, and an
/// observer that reaches back into the widget would panic on a live borrow.
pub(crate) fn sync_cursor_signals(state: &SharedState) {
    let mut st = state.borrow_mut();
    let pos = st.cursor.position();
    let anchor = st.cursor.anchor();
    let has_sel = st.all_carets().any(|c| c.has_selection());
    let count = 1 + st.extra_carets.len();

    let pos_sig = st.cursor_position.clone();
    let anchor_sig = st.cursor_anchor.clone();
    let sel_sig = st.has_selection.clone();
    let count_sig = st.caret_count.clone();
    let caret_vis = st.caret_visible.clone();

    // Recompute the bracket match at the single choke point every caret move
    // passes through — but only when the app asked for it, so a plain-text
    // editor or a document with no configured pairs pays nothing. The scan reads
    // the document while it is borrowed here; the resulting signal is written
    // after the borrow drops, with the rest.
    let bracket_sig = st.bracket_match.clone();
    let bracket_val = if st.config.match_brackets {
        semantics::current_bracket_match(&st)
    } else {
        None
    };

    // Restart the blink so the caret stays lit through a held arrow key rather
    // than toggling mid-motion. `restart` deliberately does not write the
    // signal — see its docs — so the caller does, below, outside the borrow.
    let blink_reset = st.has_focus
        && matches!(
            st.policy.caret_policy,
            crate::common::editor_runtime::CaretPolicy::Blinking
        );
    if blink_reset {
        st.blink.restart();
    }
    drop(st);

    pos_sig.set_if_changed(pos);
    anchor_sig.set_if_changed(anchor);
    sel_sig.set_if_changed(has_sel);
    count_sig.set_if_changed(count);
    bracket_sig.set_if_changed(bracket_val);
    if blink_reset {
        caret_vis.set_if_changed(true);
    }
}

/// A handle onto a live editor, cloneable and detachable from the widget.
///
/// The `EditorHandle` pattern: an app keeps one to drive the editor from a
/// toolbar, a shortcut, or a test without holding the widget itself.
#[derive(Clone)]
pub struct CodeEditorHandle {
    state: SharedState,
}

impl std::fmt::Debug for CodeEditorHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeEditorHandle").finish_non_exhaustive()
    }
}

impl CodeEditorHandle {
    pub(crate) fn new(state: SharedState) -> Self {
        Self { state }
    }

    /// The caret's document position.
    pub fn cursor_position(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    /// The primary caret's document position — a character offset into the whole
    /// document, not a line or column — as a reactive signal. Bind it in a status
    /// bar to show a caret position that tracks every caret move, not only edits.
    pub fn cursor_position_signal(&self) -> bastyde_core::Signal<usize> {
        self.state.borrow().cursor_position.clone()
    }

    /// Live caret count — `1` unless multi-caret editing is active.
    pub fn caret_count(&self) -> bastyde_core::Signal<usize> {
        self.state.borrow().caret_count.clone()
    }

    /// The bracket next to the caret and its match, as document positions, or
    /// `None`. Populated only when the editor was configured with
    /// `match_brackets` and bracket pairs; a status surface can bind it, or an
    /// app can read it to drive its own overlay.
    pub fn bracket_match(&self) -> bastyde_core::Signal<Option<(usize, usize)>> {
        self.state.borrow().bracket_match.clone()
    }

    pub fn has_selection(&self) -> bastyde_core::Signal<bool> {
        self.state.borrow().has_selection.clone()
    }

    pub fn can_undo(&self) -> bastyde_core::Signal<bool> {
        self.state.borrow().can_undo.clone()
    }

    pub fn can_redo(&self) -> bastyde_core::Signal<bool> {
        self.state.borrow().can_redo.clone()
    }

    /// Bumps on every content or format change.
    pub fn document_version(&self) -> bastyde_core::Signal<u64> {
        self.state.borrow().document_version.clone()
    }

    pub fn scroll_y(&self) -> bastyde_core::Signal<f32> {
        self.state.borrow().scroll_y.clone()
    }

    #[cfg(test)]
    pub(crate) fn state_handle(&self) -> SharedState {
        self.state.clone()
    }
}

/// Keep `Rc` in the import set for the state alias.
const _: () = {
    fn _assert_shared(_: &Rc<std::cell::RefCell<CodeEditorState>>) {}
};
