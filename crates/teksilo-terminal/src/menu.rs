// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The terminal's context menu: Copy / Paste / Select all / Clear.
//!
//! # Why it is written here rather than reused
//!
//! `MenuList` lives in `teksilo-widgets`, and this crate depends on
//! `teksilo-core` only — deliberately, so an app that embeds no terminal pays
//! nothing, and so the terminal can be composed into a widget set that is not
//! Teksilo's own. The alternative to these two small self-painting widgets was
//! shipping no menu at all and leaving Copy on a chord a finger cannot press.
//!
//! What that costs is stated rather than hidden: this is a flat list of rows
//! with no submenus, no separators, no icons, no shortcut column and no
//! mnemonics. It is a menu for four commands. An application that wants its own
//! richer one installs a
//! [`context_menu`](teksilo_core::widget_builder::WidgetBuilder::context_menu)
//! factory on an ancestor and this one steps aside — the terminal's factory is
//! consulted first only because the walk starts at the target.
//!
//! # Which commands, and where they come from
//!
//! The four rows are not a list held here: Copy / Paste / Select all come from
//! [`TextHitSource::clipboard_actions`],
//! the same answer that would drive a selection toolbar, so the menu cannot
//! offer a Copy with nothing selected or a Paste into a read-only terminal.
//! Clear is the terminal's own and is always offered.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::text_touch::{TextAction, TextHitSource};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, CornerRadius, SurfaceRole, TextRole, TextStyle};

use crate::state::TerminalState;
use crate::touch::TerminalHitSource;

/// Vertical padding above the first row and below the last, in dp.
const MENU_PADDING: f32 = 4.0;
/// Horizontal padding inside a row, in dp.
const ROW_PADDING_X: f32 = 12.0;
/// A row's height floor, in dp — the WCAG 2.5.8 minimum a finger needs. The
/// row grows past it for a larger text scale; it never shrinks below it.
const ROW_MIN_HEIGHT: f32 = 24.0;
/// Extra height a row takes over its label, in dp.
const ROW_LEADING: f32 = 10.0;
/// Corner radius of the menu panel, in dp.
const MENU_RADIUS: f32 = 8.0;

/// One command the terminal's context menu can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalMenuCommand {
    Copy,
    Paste,
    SelectAll,
    /// Clear the visible screen. Scrollback is retained — the destructive
    /// full reset is not offered from a menu a finger can open by accident.
    Clear,
}

impl TerminalMenuCommand {
    /// The label, in English. Not localised: this crate has no message bundle
    /// (it depends on neither `teksilo-i18n` nor `teksilo-widgets`), the same
    /// position `teksilo_core::text_touch`'s own handle labels are in. An app
    /// that ships translations installs its own context-menu factory.
    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::SelectAll => "Select all",
            Self::Clear => "Clear",
        }
    }
}

/// The commands to offer for the terminal's current state.
///
/// Copy / Paste / Select all are read off the touch contract's
/// `clipboard_actions`, so the menu and a selection toolbar could never
/// disagree about what this surface will honour. `Cut` is never offered — the
/// contract already answers `false` for it, and the mapping below would drop it
/// anyway.
pub(crate) fn commands_for(st: &mut TerminalState) -> Vec<TerminalMenuCommand> {
    let source = TerminalHitSource::new(st);
    let mut out: Vec<TerminalMenuCommand> = source
        .clipboard_actions()
        .to_actions()
        .into_iter()
        .filter_map(|action| match action {
            TextAction::Copy => Some(TerminalMenuCommand::Copy),
            TextAction::Paste => Some(TerminalMenuCommand::Paste),
            TextAction::SelectAll => Some(TerminalMenuCommand::SelectAll),
            TextAction::Cut | TextAction::Custom(_) => None,
        })
        .collect();
    out.push(TerminalMenuCommand::Clear);
    out
}

/// What a menu row does when it is chosen. Held as an `Rc` so every row shares
/// one closure rather than one per row capturing the terminal's whole state.
type RunCommand = Rc<dyn Fn(TerminalMenuCommand, &mut EventContext<'_>)>;

/// The terminal's context menu.
pub(crate) struct TerminalMenu {
    commands: Vec<TerminalMenuCommand>,
    run: RunCommand,
    /// Which row the keyboard is on. Also the AT `active_descendant`, so a
    /// screen reader follows arrow keys without a focus move per row.
    active: Signal<usize>,
    rows: Vec<WidgetId>,
    /// Row height and label width, measured in `layout_response` and read by
    /// `place_children`. `Cell`s because both methods take `&self` and both
    /// need the same two numbers from one text measurement per pass.
    row_height: Cell<f32>,
    width: Cell<f32>,
}

impl std::fmt::Debug for TerminalMenu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalMenu")
            .field("commands", &self.commands)
            .finish_non_exhaustive()
    }
}

impl TerminalMenu {
    pub(crate) fn new(commands: Vec<TerminalMenuCommand>, run: RunCommand) -> Self {
        Self {
            commands,
            run,
            active: Signal::new(0),
            rows: Vec::new(),
            row_height: Cell::new(ROW_MIN_HEIGHT),
            width: Cell::new(0.0),
        }
    }

    /// The label style, and so the row height. One place, so the measurement in
    /// `layout_response` and the paint in the row cannot disagree.
    fn label_style(theme: &teksilo_core::styles::Theme) -> TextStyle {
        theme.typography.body.clone()
    }
}

impl Widget for TerminalMenu {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.rows.clear();
        for (index, command) in self.commands.iter().copied().enumerate() {
            let id = ctx.add(TerminalMenuRow {
                command,
                index,
                count: self.commands.len(),
                active: self.active.clone(),
                run: Rc::clone(&self.run),
            });
            self.rows.push(id);
        }

        let active = self.active.clone();
        let count = self.commands.len();
        let commands = self.commands.clone();
        let run = Rc::clone(&self.run);
        let handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Default)
            .on_key(move |event, ctx| {
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                if count == 0 {
                    return EventResponse::Ignored;
                }
                match key {
                    Key::ArrowDown => {
                        active.set((active.get() + 1) % count);
                        EventResponse::Handled
                    }
                    Key::ArrowUp => {
                        active.set((active.get() + count - 1) % count);
                        EventResponse::Handled
                    }
                    Key::Home => {
                        active.set(0);
                        EventResponse::Handled
                    }
                    Key::End => {
                        active.set(count - 1);
                        EventResponse::Handled
                    }
                    Key::Enter | Key::Space => {
                        if let Some(command) = commands.get(active.get()).copied() {
                            run(command, ctx);
                            ctx.dismiss_self_overlay_chain();
                        }
                        EventResponse::Handled
                    }
                    // Escape is the overlay's own (`EscapeOrClickOutside`), so
                    // declining it is what lets the framework close the menu.
                    _ => EventResponse::Ignored,
                }
            });
        ctx.apply_self_handlers(handlers);

        // A row's highlight follows `active`, which is a repaint of a row and
        // never a relayout of the panel.
        self.active.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );

        self.rows.clone()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let style = Self::label_style(ctx.theme);
        let mut width = 0.0f32;
        let mut line = ROW_MIN_HEIGHT - ROW_LEADING;
        if let Some(backend) = ctx.text_backend {
            let mut backend = backend.borrow_mut();
            for command in &self.commands {
                let layout = backend.layout_single_line(command.label(), &style, None);
                width = width.max(layout.width);
                line = line.max(layout.height);
            }
        } else {
            for command in &self.commands {
                width = width.max(command.label().chars().count() as f32 * style.size * 0.6);
            }
        }
        let row_height = (line + ROW_LEADING).max(ROW_MIN_HEIGHT);
        self.row_height.set(row_height);
        self.width.set(width);
        let size = Size::new(
            width + ROW_PADDING_X * 2.0,
            row_height * self.commands.len() as f32 + MENU_PADDING * 2.0,
        );
        proposal.resolve(size.width, size.height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let mut y = bounds.y + MENU_PADDING;
        for placement in children.iter_mut() {
            placement.origin = Point::new(bounds.x, y);
            placement.size = Size::new(bounds.width, self.row_height.get());
            y += self.row_height.get();
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        canvas.fill_rounded_rect(
            bounds,
            CornerRadius::uniform(MENU_RADIUS),
            SurfaceRole::Raised.resolve(&ctx.theme.colors),
        );
        canvas.stroke_rounded_rect(
            bounds,
            CornerRadius::uniform(MENU_RADIUS),
            BorderRole::Default.resolve(&ctx.theme.colors),
            1.0,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(accesskit::Role::Menu);
        builder.set_orientation(accesskit::Orientation::Vertical);
        if let Some(id) = self.rows.get(self.active.get()) {
            builder.set_active_descendant(widget_id_to_node_id(*id));
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.rows.clone()
    }
}

/// One row of [`TerminalMenu`].
struct TerminalMenuRow {
    command: TerminalMenuCommand,
    index: usize,
    count: usize,
    active: Signal<usize>,
    run: RunCommand,
}

impl std::fmt::Debug for TerminalMenuRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalMenuRow")
            .field("command", &self.command)
            .finish_non_exhaustive()
    }
}

impl Widget for TerminalMenuRow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let command = self.command;
        let tap_run = Rc::clone(&self.run);
        let action_run = Rc::clone(&self.run);
        let active = self.active.clone();
        let index = self.index;
        let handlers = HandlerSet::new()
            .cursor(CursorIcon::Default)
            .on_hover(move |entered, _ctx| {
                if entered {
                    active.set(index);
                }
            })
            .on_tap(move |_tap, ctx| {
                tap_run(command, ctx);
                ctx.dismiss_self_overlay_chain();
            })
            .on_access_action(move |action, ctx| {
                if action == accesskit::Action::Click {
                    // Same two statements the tap runs: an assistive client's
                    // Click must both act and take the menu down, or the menu
                    // outlives the command that closed it everywhere else.
                    action_run(command, ctx);
                    ctx.dismiss_self_overlay_chain();
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });
        ctx.apply_self_handlers(handlers);
        self.active.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, ROW_MIN_HEIGHT).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let is_active = self.active.get() == self.index;
        if is_active {
            // `Hover`, not `Accent`: a menu row's highlight is the neutral
            // hover surface in every preset this crate can see, and it keeps
            // the label on `TextRole::Primary` — no contrast pairing to get
            // wrong against a theme whose accent this crate never chose.
            canvas.fill_rect(bounds, SurfaceRole::Hover.resolve(&ctx.theme.colors));
        }
        let style = TerminalMenu::label_style(ctx.theme);
        let color = TextRole::Primary.resolve(&ctx.theme.colors);
        canvas.draw_text(
            self.command.label(),
            Rect::new(
                bounds.x + ROW_PADDING_X,
                bounds.y,
                (bounds.width - ROW_PADDING_X * 2.0).max(0.0),
                bounds.height,
            ),
            &style,
            color,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(accesskit::Role::MenuItem);
        builder.set_name(self.command.label());
        builder.add_action(accesskit::Action::Click);
        builder.set_position_in_set(self.index + 1);
        builder.set_size_of_set(self.count);
    }
}

/// Build the menu the terminal's `context_menu` factory returns.
pub(crate) fn build_menu(state: &Rc<RefCell<TerminalState>>, run: RunCommand) -> Box<dyn Widget> {
    let commands = {
        let mut st = state.borrow_mut();
        commands_for(&mut st)
    };
    Box::new(TerminalMenu::new(commands, run))
}
