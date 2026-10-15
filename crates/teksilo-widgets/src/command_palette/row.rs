// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One palette row: `Category: Name` on one line, the letters the query
//! matched in bold, then the command's description, muted, and the shortcut
//! as a keycap at the trailing edge.
//!
//! The row is a `Role::ListBoxOption` named by its whole text. Its labels are
//! hidden from assistive technology so the name is read once. It is not
//! focusable: focus stays in the search field, which points at the
//! highlighted row through its active descendant.

use std::rc::Rc;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::ObserverHandle;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::density::dp;
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::lit;
use teksilo_tokens::{BorderRole, CornerRadius, SurfaceRole, TargetRole, TextRole, TextStyleRole};

use crate::primitives::{HStack, Padding, RectWidget, Shrinkable, Spacer, TextWidget, ZStack};

use super::PaletteState;
use super::matcher::highlighted_markup;
use super::results::PaletteEntry;

/// A row's height before the density floor: one line of body text plus
/// [`ROW_PADDING_VERTICAL`] above and below.
const ROW_MIN_HEIGHT: f32 = 28.0;
const ROW_PADDING_VERTICAL: f32 = 4.0;
/// Inset of the label from the highlight's edge.
const ROW_PADDING_HORIZONTAL: f32 = 10.0;
/// Inset of the highlight from the row's edge, so its rounded corners float
/// inside the list the way `StandardListItem`'s do.
pub(super) const ROW_HIGHLIGHT_INSET: f32 = 4.0;
const ROW_CORNER_RADIUS: f32 = 6.0;
/// The highlighted row's outline: the non-colour half of the highlight (WCAG
/// 1.4.1), the same edge `StandardListItem` draws round a selected row.
const ROW_SELECTION_EDGE_WIDTH: f32 = 1.0;
const ROW_GAP: f32 = 6.0;
/// The narrowest a name or category is squeezed to before the row overflows.
const LABEL_MIN_WIDTH: f32 = 40.0;
/// How much faster than the name the description gives up room.
const DESCRIPTION_SHRINK: f32 = 8.0;
const KEYCAP_PADDING_HORIZONTAL: f32 = 6.0;
const KEYCAP_PADDING_VERTICAL: f32 = 1.0;
const KEYCAP_CORNER_RADIUS: f32 = 4.0;

/// The height of every row, from the theme a pass is laid out with.
///
/// Read both by the row and by the result list sizing its viewport, so the
/// two cannot disagree. The typography in a layout pass carries the global
/// text scale, so a row grows with the text instead of clipping it.
pub(super) fn row_height(theme: &teksilo_core::Theme) -> f32 {
    let body = &theme.typography.body;
    let line = body.size * body.line_height;
    dp(ROW_MIN_HEIGHT, TargetRole::Target, &theme.input).max(line + 2.0 * ROW_PADDING_VERTICAL)
}

pub(super) struct PaletteRow {
    pub(super) index: usize,
    pub(super) entry: Rc<PaletteEntry>,
    pub(super) state: PaletteState,
    pub(super) root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for PaletteRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaletteRow")
            .field("index", &self.index)
            .field("id", &self.entry.command.id)
            .finish()
    }
}

impl Widget for PaletteRow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let index = self.index;
        let self_id = ctx.self_id();
        // `selected` is read in `accessibility`, so the node is re-walked
        // when the highlight moves. Paint follows through the role signals.
        self.state.highlight.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );
        self.publish_active_row(ctx);

        let hovered = ctx.signal(false);
        let current = self.state.highlight.map(move |h| *h == index);
        let background = current.zip(&hovered).map(|(current, hovered)| {
            if *current {
                SurfaceRole::Selected
            } else if *hovered {
                SurfaceRole::Hover
            } else {
                SurfaceRole::Transparent
            }
        });
        let edge = current.map(|current| {
            if *current {
                ROW_SELECTION_EDGE_WIDTH
            } else {
                0.0
            }
        });
        let highlight = Padding::new(0.0, ROW_HIGHLIGHT_INSET, 0.0, ROW_HIGHLIGHT_INSET).child(
            RectWidget::new()
                .background(background)
                .border_color(BorderRole::Focused)
                .border_width(edge)
                .corner_radius(CornerRadius::uniform(ROW_CORNER_RADIUS)),
        );

        let entry = &self.entry;
        let enabled = entry.command.enabled;
        let (category_role, name_role) = if enabled {
            (TextRole::Secondary, TextRole::Primary)
        } else {
            (TextRole::Disabled, TextRole::Disabled)
        };
        let label = |markup: String, role: TextRole| {
            Shrinkable::new().min_width(LABEL_MIN_WIDTH).child(
                TextWidget::new(lit!(markup))
                    .style(TextStyleRole::Body)
                    .color(role)
                    .markup(true)
                    .single_line()
                    .a11y_hidden(),
            )
        };
        let mut line = HStack::new().spacing(ROW_GAP);
        if let Some(category) = &entry.category_prefix {
            line = line.child(label(
                highlighted_markup(category, &entry.category_positions),
                category_role,
            ));
        }
        line = line.child(label(
            highlighted_markup(&entry.command.name, &entry.name_positions),
            name_role,
        ));
        if let Some(description) = &entry.command.description {
            // After the name, muted and smaller, and the first thing to give
            // way when the row is short of room: it says more about the
            // command, the name says which command it is.
            line = line.child(
                Shrinkable::new().shrink(DESCRIPTION_SHRINK).child(
                    TextWidget::new(lit!(description.clone()))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary)
                        .single_line()
                        .a11y_hidden(),
                ),
            );
        }
        line = line.child(Spacer::new());
        if let Some(chord) = &entry.chord {
            line = line.child(keycap(chord));
        }
        let content =
            Padding::symmetric(0.0, ROW_HIGHLIGHT_INSET + ROW_PADDING_HORIZONTAL).child(line);

        let root = ctx.add(ZStack::new().child(highlight).child(content));
        self.root_child_id = Some(root);

        let state = self.state.clone();
        ctx.apply_self_handlers(
            HandlerSet::new()
                .on_tap(move |_tap, ctx: &mut EventContext| {
                    state.set_highlight(index);
                    state.activate_highlighted(ctx);
                })
                .on_hover(move |entered: bool, _ctx: &mut EventContext| hovered.set(entered))
                .cursor(if enabled {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                }),
        );
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let height = row_height(ctx.theme);
        let child = self
            .root_child_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: proposal.width,
                        height: Some(height),
                    },
                )
            })
            .unwrap_or_default();
        Size::new(
            proposal.width.unwrap_or(child.width),
            height.max(child.height),
        )
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
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::ListBoxOption);
        builder.set_name(self.entry.accessible_name.clone());
        if let Some(description) = &self.entry.command.description {
            builder.set_description(description.clone());
        }
        if let Some(chord) = &self.entry.chord {
            builder.set_keyboard_shortcut(chord.clone());
        }
        if !self.entry.command.enabled {
            builder.set_disabled();
        }
        builder.set_selected(self.state.highlight.get() == self.index);
        builder.set_position_in_set(self.index + 1);
        // The "of N" half is on the result list's `Role::ListBox`: AccessKit
        // resolves an item's set size by walking up to its container.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

impl PaletteRow {
    /// Publish this row's node while it holds the highlight, for the search
    /// field's `active_descendant`, and withdraw it when the highlight leaves
    /// or the row is destroyed (scrolled out of the realized window, or
    /// rebuilt by a new query). The row does it rather than the list because
    /// only the row knows its node, and a virtualized list builds a row only
    /// once it scrolls into view, after the key that moved the highlight
    /// there.
    fn publish_active_row(&self, ctx: &mut BuildContext) {
        let self_id = ctx.self_id();
        let index = self.index;
        let node = self.state.active_row.clone();
        if self.state.highlight.get() == index {
            node.set(Some(self_id));
        }
        ctx.effect(&self.state.highlight, move |highlight| {
            if *highlight == index {
                if node.get() != Some(self_id) {
                    node.set(Some(self_id));
                }
            } else if node.get() == Some(self_id) {
                node.set(None);
            }
        });
        let node = self.state.active_row.clone();
        ctx.own_handle(ObserverHandle::new(
            Rc::new(()),
            0,
            Rc::new(move |_| {
                if node.get() == Some(self_id) {
                    node.set(None);
                }
            }),
        ));
    }
}

/// A keyboard shortcut drawn as a key: the chord in small text on a sunken,
/// outlined, rounded ground.
fn keycap(chord: &str) -> impl Widget + 'static {
    ZStack::new()
        .child(
            RectWidget::new()
                .background(SurfaceRole::Sunken)
                .border_color(BorderRole::Default)
                .border_width(1.0)
                .corner_radius(CornerRadius::uniform(KEYCAP_CORNER_RADIUS)),
        )
        .child(
            Padding::symmetric(KEYCAP_PADDING_VERTICAL, KEYCAP_PADDING_HORIZONTAL).child(
                TextWidget::new(lit!(chord.to_string()))
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary)
                    .single_line()
                    .no_shrink()
                    .a11y_hidden(),
            ),
        )
}
