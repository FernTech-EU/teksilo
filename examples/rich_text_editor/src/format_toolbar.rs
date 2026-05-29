//! Two-row formatting toolbar for the rich_text_editor example.
//!
//! Built against [`bastyde::widgets::rich_text::EditorHandle`], which
//! exposes the editor's command and signal API in a clone-able form
//! suitable for closure captures. The toolbar drives every reactive
//! pattern a third-party formatting toolbar needs:
//!
//! * Per-button `Signal<bool>` mirroring inline format state
//!   (Bold / Italic / Underline / Strikethrough). The signal is a
//!   regular `bastyde::Signal` (not derived) — required because
//!   [`bastyde::widgets::IconButton::toggle`] writes back on click,
//!   and derived signals are read-only.
//! * A four-way mutually-exclusive alignment group. The same
//!   [`IconButton::toggle`] click-flip applies, so the activation
//!   closure re-syncs all four signals immediately after setting the
//!   alignment — that defeats the click-flip when the user clicks
//!   the already-active alignment.
//! * A two-way bound [`bastyde::widgets::ComboBox`] heading picker.
//!   Editor→picker writes go through one `ctx.effect`; picker→editor
//!   writes go through another, guarded by an explicit value check
//!   (because [`Signal::set`] does not short-circuit on equal values).
//! * Contextual enable via [`bastyde::core::build_context::BuildContext::enabled_when`]
//!   for the table-operations row, which disables when the caret is
//!   outside any table.
//!
//! Every button is `.focusable(false)` so the editor keeps focus
//! across toolbar clicks — Ctrl+B typed after a toolbar click still
//! reaches the editor.

use bastyde::canvas::svg::SvgIcon;
use bastyde::core::widget::WidgetPlacement;
use bastyde::prelude::*;
use bastyde::res;
use bastyde::text_document::Alignment;
use bastyde::widgets::rich_text::{EditorHandle, RichTextEditor};
use bastyde::widgets::{ComboBox, Divider, IconButton, IconWidget, Toolbar, VStack};

/// Heading level shown in the picker. Matches the HTML `<h1>..<h6>`
/// convention; `Normal` is the plain-paragraph option (level 0).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeadingLevel {
    Normal,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
}

impl HeadingLevel {
    fn from_u8(level: u8) -> Self {
        match level {
            1 => Self::H1,
            2 => Self::H2,
            3 => Self::H3,
            4 => Self::H4,
            5 => Self::H5,
            6 => Self::H6,
            _ => Self::Normal,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::H1 => 1,
            Self::H2 => 2,
            Self::H3 => 3,
            Self::H4 => 4,
            Self::H5 => 5,
            Self::H6 => 6,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::H1 => "Heading 1",
            Self::H2 => "Heading 2",
            Self::H3 => "Heading 3",
            Self::H4 => "Heading 4",
            Self::H5 => "Heading 5",
            Self::H6 => "Heading 6",
        }
    }
}

const ALL_HEADING_LEVELS: [HeadingLevel; 7] = [
    HeadingLevel::Normal,
    HeadingLevel::H1,
    HeadingLevel::H2,
    HeadingLevel::H3,
    HeadingLevel::H4,
    HeadingLevel::H5,
    HeadingLevel::H6,
];

/// Composing widget that wraps two [`Toolbar`]s in a [`VStack`] and
/// wires every reactive binding against the supplied
/// [`RichTextEditor`].
pub struct FormatToolbar {
    handle: EditorHandle,
    root: Option<WidgetId>,
}

impl FormatToolbar {
    pub fn new(editor: &RichTextEditor) -> Self {
        Self {
            handle: editor.handle(),
            root: None,
        }
    }
}

impl std::fmt::Debug for FormatToolbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormatToolbar").finish_non_exhaustive()
    }
}

impl Widget for FormatToolbar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // ── Inline format mirror signals (Bold / Italic / Underline / Strike) ──
        let is_bold = ctx.signal(self.handle.is_bold());
        let is_italic = ctx.signal(self.handle.is_italic());
        let is_underline = ctx.signal(self.handle.is_underline());
        let is_strike = ctx.signal(self.handle.is_strikethrough());

        // ── Alignment mirror signals (4-way mutually exclusive) ──
        let init_align = self.handle.get_alignment();
        let is_align_left = ctx.signal(init_align == Alignment::Left);
        let is_align_center = ctx.signal(init_align == Alignment::Center);
        let is_align_right = ctx.signal(init_align == Alignment::Right);
        let is_align_justify = ctx.signal(init_align == Alignment::Justify);

        // ── Table-context signal for row-2 enable state ──
        let is_in_table = ctx.signal(self.handle.is_in_table());

        // ── Heading-picker mutable signal (two-way bound to ComboBox) ──
        let heading_selected: Signal<Option<HeadingLevel>> =
            ctx.signal(Some(HeadingLevel::from_u8(self.handle.get_heading_level())));

        // ── Editor → toolbar re-sync via per-frame poll with version
        // diff. NOT direct `ctx.effect(&format_version, ...)` because
        // `format_version` and `cursor_position` are written from
        // *inside* `RichTextEditor`'s own `state.borrow_mut()` (the
        // frame-tick path calls `drain_events` which then calls
        // `format_version.set(...)`); observers fire synchronously
        // there, and re-entering the state via `handle.is_bold()`
        // would panic with `already mutably borrowed`. `frame_tick`
        // fires *outside* any state borrow — by the time the toolbar
        // sees the tick, the editor's tick closure has already
        // released its borrow.
        let fv_src = self.handle.format_version();
        let cp_src = self.handle.cursor_position_signal();
        let last_fv = std::rc::Rc::new(std::cell::Cell::new(fv_src.get()));
        let last_cp = std::rc::Rc::new(std::cell::Cell::new(cp_src.get()));
        let tick = ctx.frame_tick();
        {
            let handle = self.handle.clone();
            let is_bold = is_bold.clone();
            let is_italic = is_italic.clone();
            let is_underline = is_underline.clone();
            let is_strike = is_strike.clone();
            let is_left = is_align_left.clone();
            let is_center = is_align_center.clone();
            let is_right = is_align_right.clone();
            let is_justify = is_align_justify.clone();
            let heading = heading_selected.clone();
            let in_table = is_in_table.clone();
            let last_fv = last_fv.clone();
            let last_cp = last_cp.clone();
            ctx.effect(&tick, move |_| {
                let fv_now = fv_src.get();
                let cp_now = cp_src.get();
                if fv_now == last_fv.get() && cp_now == last_cp.get() {
                    return;
                }
                last_fv.set(fv_now);
                last_cp.set(cp_now);
                is_bold.set(handle.is_bold());
                is_italic.set(handle.is_italic());
                is_underline.set(handle.is_underline());
                is_strike.set(handle.is_strikethrough());
                let align = handle.get_alignment();
                is_left.set(align == Alignment::Left);
                is_center.set(align == Alignment::Center);
                is_right.set(align == Alignment::Right);
                is_justify.set(align == Alignment::Justify);
                let target = Some(HeadingLevel::from_u8(handle.get_heading_level()));
                if heading.get() != target {
                    heading.set(target);
                }
                in_table.set(handle.is_in_table());
            });
        }

        // ── Picker → editor effect. Guarded against re-entry because
        // Signal::set fires observers unconditionally (no equality
        // check), so the editor→picker effect feeds back into this one
        // on every FormatChanged. The guard short-circuits when the
        // editor already holds the desired level.
        {
            let handle = self.handle.clone();
            ctx.effect(&heading_selected, move |sel| {
                if let Some(level) = sel.as_ref().copied() {
                    let target = level.to_u8();
                    if handle.get_heading_level() != target {
                        handle.set_heading_level(target);
                    }
                }
            });
        }

        // ── Row 1: inline / heading / alignment / lists / indent / insert-table / undo / redo ──

        let bold_id = ctx.add(toggle_btn(
            res!("resources/icons/bold.svg"),
            "Bold (Ctrl+B)",
            is_bold.clone(),
            {
                let h = self.handle.clone();
                move |_| h.toggle_bold()
            },
        ));
        let italic_id = ctx.add(toggle_btn(
            res!("resources/icons/italic.svg"),
            "Italic (Ctrl+I)",
            is_italic.clone(),
            {
                let h = self.handle.clone();
                move |_| h.toggle_italic()
            },
        ));
        let underline_id = ctx.add(toggle_btn(
            res!("resources/icons/underline.svg"),
            "Underline (Ctrl+U)",
            is_underline.clone(),
            {
                let h = self.handle.clone();
                move |_| h.toggle_underline()
            },
        ));
        let strike_id = ctx.add(toggle_btn(
            res!("resources/icons/strikethrough.svg"),
            "Strikethrough",
            is_strike.clone(),
            {
                let h = self.handle.clone();
                move |_| h.toggle_strikethrough()
            },
        ));

        let heading_picker_id = ctx.add(
            ComboBox::from_items(ALL_HEADING_LEVELS, heading_selected.clone(), |level| {
                lit!(level.label().to_string())
            })
            .label(lit!("Heading level"))
            .max_visible_items(7),
        );

        // Alignment buttons share an activation pattern: set the
        // alignment, then re-sync all four mirror signals to defeat
        // IconButton::toggle's click-flip for the already-active case.
        let align_left_id = ctx.add(toggle_btn(
            res!("resources/icons/align-left.svg"),
            "Align Left",
            is_align_left.clone(),
            alignment_action(
                self.handle.clone(),
                Alignment::Left,
                &is_align_left,
                &is_align_center,
                &is_align_right,
                &is_align_justify,
            ),
        ));
        let align_center_id = ctx.add(toggle_btn(
            res!("resources/icons/align-center.svg"),
            "Align Center",
            is_align_center.clone(),
            alignment_action(
                self.handle.clone(),
                Alignment::Center,
                &is_align_left,
                &is_align_center,
                &is_align_right,
                &is_align_justify,
            ),
        ));
        let align_right_id = ctx.add(toggle_btn(
            res!("resources/icons/align-right.svg"),
            "Align Right",
            is_align_right.clone(),
            alignment_action(
                self.handle.clone(),
                Alignment::Right,
                &is_align_left,
                &is_align_center,
                &is_align_right,
                &is_align_justify,
            ),
        ));
        let align_justify_id = ctx.add(toggle_btn(
            res!("resources/icons/align-justify.svg"),
            "Justify",
            is_align_justify.clone(),
            alignment_action(
                self.handle.clone(),
                Alignment::Justify,
                &is_align_left,
                &is_align_center,
                &is_align_right,
                &is_align_justify,
            ),
        ));

        let bullet_id = ctx.add(plain_btn(
            res!("resources/icons/list-bulleted.svg"),
            "Bullet list",
            {
                let h = self.handle.clone();
                move |_| h.insert_list(false)
            },
        ));
        let numbered_id = ctx.add(plain_btn(
            res!("resources/icons/list-numbered.svg"),
            "Numbered list",
            {
                let h = self.handle.clone();
                move |_| h.insert_list(true)
            },
        ));
        let indent_id = ctx.add(plain_btn(
            res!("resources/icons/indent.svg"),
            "Indent (Tab)",
            {
                let h = self.handle.clone();
                move |_| h.indent()
            },
        ));
        let outdent_id = ctx.add(plain_btn(
            res!("resources/icons/outdent.svg"),
            "Outdent (Shift+Tab)",
            {
                let h = self.handle.clone();
                move |_| h.outdent()
            },
        ));

        let blockquote_id = ctx.add(plain_btn(
            res!("resources/icons/blockquote.svg"),
            "Toggle blockquote",
            {
                let h = self.handle.clone();
                move |_| h.toggle_blockquote()
            },
        ));
        // NOTE: when the selection crosses a frame boundary,
        // `EditorHandle::toggle_blockquote` returns silently — the
        // underlying use case rejects the wrap with a typed error and
        // nothing is mutated. A reactive "disable on cross-frame
        // selection" signal would be a polish improvement; for now the
        // button is always enabled and harmless on cross-frame ranges.

        let insert_table_id = ctx.add(plain_btn(
            res!("resources/icons/table.svg"),
            "Insert 3×3 table",
            {
                let h = self.handle.clone();
                move |_| h.insert_table(3, 3)
            },
        ));

        let undo_id = ctx.add(plain_btn(
            res!("resources/icons/undo.svg"),
            "Undo (Ctrl+Z)",
            {
                let h = self.handle.clone();
                move |_| h.undo()
            },
        ));
        ctx.enabled_when(undo_id, self.handle.can_undo());
        let redo_id = ctx.add(plain_btn(
            res!("resources/icons/redo.svg"),
            "Redo (Ctrl+Shift+Z)",
            {
                let h = self.handle.clone();
                move |_| h.redo()
            },
        ));
        ctx.enabled_when(redo_id, self.handle.can_redo());

        let row1 = ctx.add(
            Toolbar::new()
                .label(lit!("Formatting"))
                .add_child(bold_id)
                .add_child(italic_id)
                .add_child(underline_id)
                .add_child(strike_id)
                .child(Divider::vertical())
                .add_child(heading_picker_id)
                .child(Divider::vertical())
                .add_child(align_left_id)
                .add_child(align_center_id)
                .add_child(align_right_id)
                .add_child(align_justify_id)
                .child(Divider::vertical())
                .add_child(bullet_id)
                .add_child(numbered_id)
                .add_child(indent_id)
                .add_child(outdent_id)
                .child(Divider::vertical())
                .add_child(blockquote_id)
                .child(Divider::vertical())
                .add_child(insert_table_id)
                .child(Divider::vertical())
                .add_child(undo_id)
                .add_child(redo_id),
        );

        // ── Row 2: table operations, all 7 buttons gated on is_in_table ──

        let row_above_id = ctx.add(plain_btn(
            res!("resources/icons/table-row-above.svg"),
            "Insert row above",
            {
                let h = self.handle.clone();
                move |_| h.insert_row_above()
            },
        ));
        ctx.enabled_when(row_above_id, is_in_table.clone());

        let row_below_id = ctx.add(plain_btn(
            res!("resources/icons/table-row-below.svg"),
            "Insert row below",
            {
                let h = self.handle.clone();
                move |_| h.insert_row_below()
            },
        ));
        ctx.enabled_when(row_below_id, is_in_table.clone());

        let col_before_id = ctx.add(plain_btn(
            res!("resources/icons/table-column-before.svg"),
            "Insert column before",
            {
                let h = self.handle.clone();
                move |_| h.insert_column_before()
            },
        ));
        ctx.enabled_when(col_before_id, is_in_table.clone());

        let col_after_id = ctx.add(plain_btn(
            res!("resources/icons/table-column-after.svg"),
            "Insert column after",
            {
                let h = self.handle.clone();
                move |_| h.insert_column_after()
            },
        ));
        ctx.enabled_when(col_after_id, is_in_table.clone());

        let del_row_id = ctx.add(plain_btn(
            res!("resources/icons/table-row-delete.svg"),
            "Delete row",
            {
                let h = self.handle.clone();
                move |_| h.remove_current_row()
            },
        ));
        ctx.enabled_when(del_row_id, is_in_table.clone());

        let del_col_id = ctx.add(plain_btn(
            res!("resources/icons/table-column-delete.svg"),
            "Delete column",
            {
                let h = self.handle.clone();
                move |_| h.remove_current_column()
            },
        ));
        ctx.enabled_when(del_col_id, is_in_table.clone());

        let remove_table_id = ctx.add(plain_btn(
            res!("resources/icons/table-remove.svg"),
            "Remove table",
            {
                let h = self.handle.clone();
                move |_| h.remove_current_table()
            },
        ));
        ctx.enabled_when(remove_table_id, is_in_table.clone());

        let row2 = ctx.add(
            Toolbar::new()
                .label(lit!("Table operations"))
                .add_child(row_above_id)
                .add_child(row_below_id)
                .add_child(col_before_id)
                .add_child(col_after_id)
                .child(Divider::vertical())
                .add_child(del_row_id)
                .add_child(del_col_id)
                .add_child(remove_table_id),
        );

        let root = ctx.add(VStack::new().add_child(row1).add_child(row2));
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        if let Some(root) = self.root
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return size.into();
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

// ── helpers ────────────────────────────────────────────────────────────

fn toggle_btn(
    icon: &'static SvgIcon,
    tooltip: &'static str,
    state: Signal<bool>,
    on_click: impl Fn(&mut EventContext) + 'static,
) -> IconButton {
    IconButton::new(IconWidget::from_svg_icon(icon))
        .toolbar()
        .focusable(false)
        .tooltip(lit!(tooltip))
        .toggle(state)
        .on_activate_fn(on_click)
}

fn plain_btn(
    icon: &'static SvgIcon,
    tooltip: &'static str,
    on_click: impl Fn(&mut EventContext) + 'static,
) -> IconButton {
    IconButton::new(IconWidget::from_svg_icon(icon))
        .toolbar()
        .focusable(false)
        .tooltip(lit!(tooltip))
        .on_activate_fn(on_click)
}

/// Build the activation closure for an alignment button. After
/// setting the alignment, immediately re-sync all four mirror signals
/// so the button appears active when the user clicks the
/// already-active alignment (defeating IconButton::toggle's click
/// flip without waiting for the next FormatChanged round-trip).
fn alignment_action(
    handle: EditorHandle,
    target: Alignment,
    left: &Signal<bool>,
    center: &Signal<bool>,
    right: &Signal<bool>,
    justify: &Signal<bool>,
) -> impl Fn(&mut EventContext) + 'static {
    let left = left.clone();
    let center = center.clone();
    let right = right.clone();
    let justify = justify.clone();
    move |_| {
        // Alignment is Clone but not Copy, so re-clone on each call
        // (the closure is Fn, not FnOnce — captures must survive
        // multiple invocations).
        handle.set_alignment(target.clone());
        let actual = handle.get_alignment();
        left.set(actual == Alignment::Left);
        center.set(actual == Alignment::Center);
        right.set(actual == Alignment::Right);
        justify.set(actual == Alignment::Justify);
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde::core::widget_tree::WidgetTree;
    use bastyde::text_document::TextDocument;

    #[test]
    fn format_toolbar_builds_and_lays_out() {
        let doc = TextDocument::new();
        let editor = RichTextEditor::editor(doc);
        let toolbar = FormatToolbar::new(&editor);

        let mut tree = WidgetTree::new();
        let _id = tree.add(toolbar);
        // Wide enough to fit both rows comfortably. The smoke test
        // just asserts every ctx.add / ctx.effect / ctx.enabled_when
        // / Toolbar / Divider / IconButton chain builds without
        // panicking under a headless tree.
        tree.layout(SizeProposal::exact(1400.0, 120.0));
    }

    #[test]
    fn handle_bold_round_trip_on_selection() {
        // `is_bold()` probes the document's char format at the
        // selection start, so a selection is required for the round-
        // trip to be observable — without a selection, toggle_bold
        // changes the *typing format* only, and `is_bold` still
        // reads the un-bolded character at the caret.
        let doc = TextDocument::new();
        doc.set_markdown("Hello world")
            .expect("parse")
            .wait()
            .expect("import");
        let editor = RichTextEditor::editor(doc);
        editor.select_all();
        let handle = editor.handle();
        assert!(!handle.is_bold(), "fresh selection is not bold");
        handle.toggle_bold();
        assert!(handle.is_bold(), "toggle_bold flips selection bold on");
        handle.toggle_bold();
        assert!(!handle.is_bold(), "toggle_bold flips selection bold off");
    }

    #[test]
    fn handle_heading_round_trip() {
        // Block-format operations don't need a selection — they
        // affect the caret's current block.
        let doc = TextDocument::new();
        doc.set_markdown("Hello")
            .expect("parse")
            .wait()
            .expect("import");
        let editor = RichTextEditor::editor(doc);
        let handle = editor.handle();
        assert_eq!(handle.get_heading_level(), 0);
        handle.set_heading_level(2);
        assert_eq!(handle.get_heading_level(), 2);
        handle.set_heading_level(0);
        assert_eq!(handle.get_heading_level(), 0);
    }

    #[test]
    fn handle_alignment_round_trip() {
        let doc = TextDocument::new();
        doc.set_markdown("Hello")
            .expect("parse")
            .wait()
            .expect("import");
        let editor = RichTextEditor::editor(doc);
        let handle = editor.handle();
        assert_eq!(handle.get_alignment(), Alignment::Left);
        handle.set_alignment(Alignment::Center);
        assert_eq!(handle.get_alignment(), Alignment::Center);
        handle.set_alignment(Alignment::Right);
        assert_eq!(handle.get_alignment(), Alignment::Right);
        handle.set_alignment(Alignment::Justify);
        assert_eq!(handle.get_alignment(), Alignment::Justify);
        handle.set_alignment(Alignment::Left);
        assert_eq!(handle.get_alignment(), Alignment::Left);
    }
}
