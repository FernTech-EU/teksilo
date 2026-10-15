// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The palette's result list: the one part of the palette that rebuilds as
//! the query changes.
//!
//! It is a sibling of the search field, never its ancestor, so a keystroke
//! rebuilds the list and leaves the field (its caret, its selection, an input
//! method's composition, its undo history) alone. Moving the highlight
//! rebuilds nothing: each row follows the highlight signal by repaint.

use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_data::ListModel;
use teksilo_i18n::{LocalizedString, tr_widget};
use teksilo_tokens::{TextRole, TextStyleRole};

use crate::keystroke_format::format_keystroke;
use crate::list_view::ListView;
use crate::primitives::{Padding, TextWidget};

use super::matcher::fuzzy_match;
use super::row::{PaletteRow, ROW_HIGHLIGHT_INSET, row_height};
use super::{IncludeFn, PaletteCommand, PaletteState, acts_on_the_focused_editor};

/// How many rows the list shows before it scrolls.
pub(super) const MAX_VISIBLE_ROWS: usize = 10;
/// Space above the first row and below the last.
const LIST_PADDING_VERTICAL: f32 = 4.0;
/// With the row's own highlight inset, puts the highlight's edges under the
/// search field's.
const LIST_PADDING_HORIZONTAL: f32 = 4.0;
const MESSAGE_PADDING_VERTICAL: f32 = 10.0;
/// Lines the empty-state text up with the row labels.
const MESSAGE_PADDING_HORIZONTAL: f32 = LIST_PADDING_HORIZONTAL + ROW_HIGHLIGHT_INSET + 10.0;

/// One matched command, with everything its row draws worked out once.
#[derive(Debug)]
pub(super) struct PaletteEntry {
    pub(super) command: PaletteCommand,
    /// The category as the row shows it, `File:`, localized; `None` for a
    /// command filed under no category.
    pub(super) category_prefix: Option<String>,
    /// Matched characters, as char indices into `category_prefix`.
    pub(super) category_positions: Vec<usize>,
    /// Matched characters, as char indices into `command.name`.
    pub(super) name_positions: Vec<usize>,
    /// The effective primary chord, formatted for display.
    pub(super) chord: Option<String>,
    /// What a screen reader is told the row is: the visible line.
    pub(super) accessible_name: String,
}

/// What the result list needs from the palette's builder options.
pub(super) struct ResultsConfig {
    pub(super) include: Option<IncludeFn>,
    pub(super) show_disabled: bool,
    pub(super) empty_text: Option<LocalizedString>,
}

/// Read the registry, filter, match against the query and rank.
pub(super) fn collect_entries(
    ctx: &BuildContext,
    state: &PaletteState,
    config: &ResultsConfig,
) -> Vec<Rc<PaletteEntry>> {
    let needle = state.query.get().trim().to_string();
    let presented = state.presented.get();
    let mut scored: Vec<(i32, PaletteEntry)> = ctx
        .shortcut_registry()
        .iter_effective()
        .map(|eff| PaletteCommand {
            id: eff.shortcut.id,
            name: eff.shortcut.name.get(),
            category: eff.shortcut.category,
            category_text: eff.shortcut.category_text(),
            description: eff.shortcut.description.as_ref().map(|d| d.get()),
            keystroke: eff.primary,
            enabled: eff.enabled,
            intent: eff.shortcut.intent_name(),
        })
        .filter(|cmd| config.show_disabled || cmd.enabled)
        // A presented palette runs a command from the focus it hands back,
        // so cut, copy, bold and the rest reach the editor it was opened
        // over. Embedded, the palette keeps the focus, and they would act on
        // its own search field.
        .filter(|cmd| presented || !acts_on_the_focused_editor(cmd.id))
        .filter(|cmd| config.include.as_ref().is_none_or(|f| f(cmd)))
        .filter_map(|cmd| {
            let found = fuzzy_match(&needle, &cmd.haystack())?;
            Some((found.score, entry_for(cmd, &found.positions)))
        })
        .collect();
    // Highest score first. `iter_effective` already ordered by (category, id)
    // and `sort_by_key` is stable, so equal scores (every row when the query
    // is empty) keep exactly that order.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, e)| Rc::new(e)).collect()
}

/// Split the haystack's matched positions between the category and the name,
/// and resolve what the row shows.
fn entry_for(command: PaletteCommand, positions: &[usize]) -> PaletteEntry {
    // The haystack is `"{category} {name}"`, see `PaletteCommand::haystack`.
    let category_len = command
        .category_text
        .as_ref()
        .map(|c| c.chars().count() + 1);
    let (mut in_category, mut in_name) = (Vec::new(), Vec::new());
    for &p in positions {
        match category_len {
            Some(len) if p + 1 < len => in_category.push(p),
            Some(len) if p + 1 == len => {} // the separating space
            Some(len) => in_name.push(p - len),
            None => in_name.push(p),
        }
    }
    let category_prefix = command.category_text.as_ref().map(|category| {
        tr_widget!(command_palette_category(category = category.clone()))
            .resolve_now()
            .to_string()
    });
    // Highlight inside the prefix only where the translation starts with the
    // category itself, so the positions still land on its letters.
    let category_positions = match (&category_prefix, &command.category_text) {
        (Some(prefix), Some(category)) if prefix.starts_with(category.as_str()) => in_category,
        _ => Vec::new(),
    };
    let accessible_name = match &category_prefix {
        Some(prefix) => format!("{prefix} {}", command.name),
        None => command.name.clone(),
    };
    PaletteEntry {
        chord: command.keystroke.map(format_keystroke),
        category_prefix,
        category_positions,
        name_positions: in_name,
        accessible_name,
        command,
    }
}

/// The list under the search field, or the line saying nothing matched.
pub(super) struct PaletteResults {
    pub(super) state: PaletteState,
    pub(super) config: Rc<ResultsConfig>,
    /// The padded list, or the empty-state message.
    body_id: Option<WidgetId>,
    row_count: usize,
}

impl PaletteResults {
    pub(super) fn new(state: PaletteState, config: Rc<ResultsConfig>) -> Self {
        Self {
            state,
            config,
            body_id: None,
            row_count: 0,
        }
    }
}

impl std::fmt::Debug for PaletteResults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaletteResults")
            .field("rows", &self.row_count)
            .finish_non_exhaustive()
    }
}

impl Widget for PaletteResults {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // A registry change (a command registered, rebound, enabled) changes
        // the rows as surely as the query does.
        ctx.shortcut_version()
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        self.state
            .query
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);

        let entries = collect_entries(ctx, &self.state, &self.config);
        let len = entries.len();
        self.row_count = len;

        // A changed query is a different list: the highlight goes back to the
        // best match. A rebuild the registry drove keeps the reader's place,
        // clamped to the new length.
        let query = self.state.query.get();
        let changed = self.state.last_query.borrow().as_deref() != Some(query.as_str());
        if changed {
            *self.state.last_query.borrow_mut() = Some(query);
        }
        let highlight = if changed {
            0
        } else {
            self.state.highlight.get().min(len.saturating_sub(1))
        };
        *self.state.rows.borrow_mut() = entries.clone();
        self.state.set_highlight(highlight);
        if self.state.count.get() != len {
            self.state.count.set(len);
        }

        if entries.is_empty() {
            // Stale entries would point the field's relations at destroyed
            // nodes.
            self.state.listbox_id.set(None);
            self.state.active_row.set(None);
            *self.state.list.borrow_mut() = None;
            let empty = self
                .config
                .empty_text
                .clone()
                .unwrap_or_else(|| tr_widget!(command_palette_empty()));
            // Hidden from assistive technology: the palette's description
            // already says no command matched.
            self.body_id = Some(
                ctx.add(
                    Padding::symmetric(MESSAGE_PADDING_VERTICAL, MESSAGE_PADDING_HORIZONTAL).child(
                        TextWidget::new(empty)
                            .style(TextStyleRole::Body)
                            .color(TextRole::Secondary)
                            .a11y_hidden(),
                    ),
                ),
            );
            return self.body_id.into_iter().collect();
        }

        if self.state.listbox_id.get() != Some(self_id) {
            self.state.listbox_id.set(Some(self_id));
        }
        let row_state = self.state.clone();
        // Presentational: this widget is the `Role::ListBox` and the rows
        // are its options. The view only realizes the rows in view.
        let list = ListView::new(
            ListModel::from_vec(entries),
            move |index, entry: &Rc<PaletteEntry>, _selected| {
                Box::new(PaletteRow {
                    index,
                    entry: entry.clone(),
                    state: row_state.clone(),
                    root_child_id: None,
                })
            },
        )
        .presentational(true)
        .auto_item_height(row_height(ctx.theme()));
        let handle = list.handle();
        if highlight > 0 {
            handle.ensure_index_visible(highlight);
        }
        *self.state.list.borrow_mut() = Some(handle);
        let list_id = ctx.add(list);
        self.body_id = Some(ctx.add(
            Padding::symmetric(LIST_PADDING_VERTICAL, LIST_PADDING_HORIZONTAL).child(list_id),
        ));

        self.body_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let width = proposal.width;
        if self.row_count == 0 {
            return self
                .body_id
                .and_then(|id| {
                    ctx.child_size(
                        id,
                        SizeProposal {
                            width,
                            height: None,
                        },
                    )
                })
                .unwrap_or_default()
                .into();
        }
        let row = row_height(ctx.theme);
        let padding = 2.0 * LIST_PADDING_VERTICAL;
        let rows = self.row_count.min(MAX_VISIBLE_ROWS) as f32;
        let w = width.unwrap_or(0.0);
        let ideal = Size::new(w, rows * row + padding);
        // Squeezed by a short window, the list gives up rows down to one and
        // scrolls.
        let min = Size::new(w, (row + padding).min(ideal.height));
        LayoutResponse::shrinkable(ideal, min, 1.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let row = row_height(ctx.theme);
        let page = ((bounds.height - 2.0 * LIST_PADDING_VERTICAL) / row).floor();
        self.state.page_rows.set((page as usize).max(1));
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.row_count == 0 {
            builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
            return;
        }
        builder.set_role(teksilo_core::accesskit::Role::ListBox);
        builder.set_size_of_set(self.row_count);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.body_id.into_iter().collect()
    }
}
