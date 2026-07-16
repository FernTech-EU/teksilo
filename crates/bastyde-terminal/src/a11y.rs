//! Accessibility for the terminal view.
//!
//! The terminal exposes its visible screen as a native `Role::Terminal` node
//! whose children are one `Role::Paragraph` → `Role::TextRun` per visible row
//! (so a screen reader can review the screen with its normal text-navigation
//! commands), with the VT cursor mapped to the AT caret. New output is
//! announced through a separate, small `Role::Status` live region
//! ([`LiveAnnouncer`]) rather than by re-announcing the whole screen — the way
//! screen readers actually consume ARIA live regions.

use accesskit::{Action, Live, Role};
use bastyde_canvas::SizeProposal;
use bastyde_core::accessibility::{AccessNodeBuilder, TextRunAttributes};
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget};
use bastyde_core::widget_id::WidgetId;

use crate::engine::GridSnapshot;

/// Build the `Role::Terminal` accessibility subtree for the current screen.
pub(crate) fn build_terminal_a11y(
    builder: &mut AccessNodeBuilder,
    snapshot: &GridSnapshot,
    name: &str,
) {
    builder.set_role(Role::Terminal);
    if !name.is_empty() {
        builder.set_name(name);
    }
    builder.add_action(Action::Focus);
    builder.add_action(Action::ScrollUp);
    builder.add_action(Action::ScrollDown);

    let cols = snapshot.columns;
    let mut cursor_target: Option<(accesskit::NodeId, usize)> = None;

    for row in 0..snapshot.screen_lines {
        // Concatenate the row's cells (a wide glyph contributes its text once;
        // its spacer half is skipped), then trim trailing blanks so the screen
        // reader doesn't read a line of spaces.
        let mut text = String::with_capacity(cols);
        for col in 0..cols {
            let Some(cell) = snapshot.cell(row, col) else {
                continue;
            };
            if cell.attrs.wide_spacer {
                continue;
            }
            text.push_str(&cell.text());
        }
        let row_text = text.trim_end().to_string();

        let para = builder.push_paragraph_child(row as u64);
        let character_lengths: Vec<u8> = row_text.chars().map(|c| c.len_utf8() as u8).collect();
        let run = builder.push_text_run_child(
            para,
            row as u64,
            0,
            row_text.clone(),
            character_lengths,
            None,
            None,
            None,
            TextRunAttributes::default(),
        );

        if snapshot.cursor.visible && snapshot.cursor.line == row {
            let char_count = row_text.chars().count();
            let idx = snapshot.cursor.column.min(char_count);
            cursor_target = Some((run, idx));
        }
    }

    // Map the VT cursor to the AT caret (a collapsed selection).
    if let Some((node, idx)) = cursor_target {
        builder.set_text_selection_to((node, idx), (node, idx));
    }
}

/// A zero-size child node that carries the terminal's "new output" live region.
/// The terminal updates [`Self::text`] with each newly-completed output line; a
/// polite live region announces it once (the value-diffing is done by the
/// framework's `collect_announcements`).
#[derive(Debug)]
pub(crate) struct LiveAnnouncer {
    pub(crate) text: Signal<String>,
}

impl Widget for LiveAnnouncer {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Re-walk this node's AT (only) whenever the announced text changes, so
        // a new completed line is picked up without a rebuild.
        self.text.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );
        Vec::new()
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        LayoutResponse::ZERO
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Status);
        builder.set_live(Live::Polite);
        let text = self.text.get();
        if !text.is_empty() {
            builder.set_value(text);
        }
    }
}
