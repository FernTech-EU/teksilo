// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! CommandPalette — type-to-run access to every command an app has registered.
//!
//! The palette is **application-agnostic**: it holds no list of its own and knows
//! nothing about any particular app. Its content is the tree's
//! [`ShortcutRegistry`](teksilo_core::shortcut::ShortcutRegistry), which already
//! carries everything a palette row needs — a localized
//! [`name`](teksilo_core::shortcut::Shortcut), an optional `category` to group by, an
//! optional `description`, the effective keystroke (user rebinds merged in), and a
//! live `enabled` verdict. Activating a row sends the command's intent, which is the
//! same path a menu row or the chord itself takes.
//!
//! That has a consequence worth stating plainly, because it is the whole design:
//! **a command does not need a keystroke to appear here.** `iter_effective()` yields
//! every registered entry, bound or not, so an app makes a command searchable by
//! registering it with a name and no chord:
//!
//! ```ignore
//! // Reachable from the palette, and rebindable by the user later, without
//! // occupying a keystroke today.
//! ctx.register_shortcut_global(
//!     Shortcut::new("document.export")
//!         .name("Export…")
//!         .category("File")
//!         .build(),
//! );
//! ```
//!
//! # Running a command
//!
//! Activating a row (Enter, or a click) closes the palette, which hands focus
//! back to whatever the user was working in, and then sends the command's
//! intent from there, as its keystroke would have been sent
//! ([`EventContext::send_intent_from_focus`]). So a command answers wherever
//! its action is registered: on the editor the palette was opened over, on
//! one of that editor's ancestors, or window-wide. Opened with nothing
//! focused, it sends from the window's root.
//!
//! That is also why a presented palette lists the commands that act on the
//! focused editor: the standard edit commands every tree registers
//! ([`StandardEdit`]: cut, copy, paste, undo, redo, select all) and the
//! editors' own ([`FormatShortcut`](crate::editor_shortcuts::FormatShortcut),
//! [`CodeShortcut`](crate::editor_shortcuts::CodeShortcut)). A palette embedded
//! in a surface of the caller's keeps the focus in its own search field, where
//! those commands would act on the query, so it leaves them out.
//!
//! # Presenting it
//!
//! [`CommandPalette::present`] shows it as a modal near the top of the window,
//! dismissed by Escape or a click outside. On the window's root, a plain
//! `register_action` is reached from everywhere the palette is opened from: a
//! shortcut and a menu bar's row both send from the focused widget, whose walk
//! ends at the root:
//!
//! ```ignore
//! ctx.register_action(Action::new("app.command_palette").on_invoke(|_, ctx| {
//!     CommandPalette::new().present(ctx);
//! }));
//! ```
//!
//! Presenting it as a **window-level** modal is deliberate, not incidental: a palette
//! is routinely opened from a menu, and a menu is itself a transient overlay.
//! Anchoring to the invoking widget would render the palette inside the menu that
//! opened it, positioned against a surface that is about to disappear. It is
//! anchored by its top edge ([`ModalPlacement::Top`]) rather than centred: the
//! palette is as tall as its list, and a centred one would move its search field
//! with every keystroke that changed the number of matches.
//!
//! # What a row shows
//!
//! One line: the category, muted (`File:`), then the command's name, with the
//! letters the query matched in bold, then its description when it has one,
//! muted and the first to give way to a long name, and the effective shortcut,
//! user rebinds merged in, drawn as a key at the trailing edge. Rows grow with
//! the global text scale.
//!
//! # Matching
//!
//! Typing filters by subsequence, not substring, so `ndw` finds "New Window" and
//! `expdoc` finds "Export document". Matches score higher when the typed letters land
//! consecutively and on word starts, so the most literal reading of a query sorts
//! first. An empty query lists everything in the registry's own deterministic
//! `(category, id)` order. The category takes part in matching, so `file new` finds
//! the New command filed under File.
//! A query can usually land in a command in more than one way (`exp` in "File:
//! Export": the `e` of "File", or the `E` of "Export"); the best-scoring way is
//! kept, and its letters are the ones the row shows in bold.
//!
//! The category a reader sees and matches against is the shortcut's
//! `category_label` when it has one, and its `category` key otherwise. Give a
//! localized label (`.category_label(tr!(…))`) and keep the key stable: the key
//! orders the groups, so it must not change with the language, and the label is
//! what a French reader types (`fichier nouveau`). Settings ▸ Keymap
//! (`ShortcutSettings`) uses the same pair, and names the group of shortcuts that
//! declared no category from the widget catalogue
//! (`shortcut-settings-uncategorized`).
//!
//! # Keyboard
//!
//! Focus stays in the search field throughout: that is what makes a palette feel
//! like one. Arrow keys are not editing keys for the field, so they bubble to the
//! palette's own handler, which moves the highlight (wrapping at either end) and
//! scrolls it into view; PageUp and PageDown move it a viewport at a time. Home and
//! End stay with the field, which moves its caret with them. Enter runs the
//! highlighted command; Escape dismisses. A palette presented by a command's
//! action (Ctrl+Shift+P bound to `app.command_palette`) is dismissed by that
//! command's chord too, so the key that opened it closes it: the palette is
//! modal, and the page's own shortcut does not reach into it.
//!
//! Typing re-filters the list and nothing else: the list is a sibling of the search
//! field, not its parent, so the field keeps its caret, its selection and an input
//! method's composition from one keystroke to the next.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Live, Role};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::intent::Intent;
use teksilo_core::modal::{ModalCloseBehavior, ModalPlacement, ModalPresentation, ModalRequest};
use teksilo_core::shortcut::{KeyStroke, StandardEdit};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{LocalizedString, tr_widget};

use crate::dialog::ModalContainer;
use crate::list_view::ListViewHandle;
use crate::primitives::{Divider, Padding};
use crate::search_field::SearchField;

mod matcher;
mod results;
mod row;

use results::{PaletteEntry, PaletteResults, ResultsConfig};

/// The palette's width when nothing narrower is proposed. Room for a category,
/// a command name and its chord without any of them having to ellipsize in
/// the common case.
const PALETTE_WIDTH: f32 = 600.0;
/// How far below the top of the window a presented palette's top edge sits.
const PALETTE_TOP_OFFSET: f32 = 56.0;
/// Inset of the search field from the palette's edges.
const FIELD_PADDING: f32 = 8.0;

/// One command as the palette sees it.
///
/// A read-only projection of a registered shortcut, handed to
/// [`CommandPalette::include`] so an app can decide what belongs in its palette
/// without the widget growing knowledge of any app's command names. Deliberately
/// *not* the `Shortcut` itself: that type carries the activation closure and the
/// rebinding machinery, neither of which a filter predicate has any business
/// reaching.
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    /// The stable registry id, e.g. `"work.export"`.
    pub id: &'static str,
    /// The localized display name, already resolved for the active locale.
    pub name: String,
    /// The grouping key, if the command declared one: stable across languages,
    /// so an [`CommandPalette::include`] filter can match on it.
    pub category: Option<&'static str>,
    /// The category as the reader sees it, already resolved for the active
    /// locale: the command's `category_label`, or the key when it gave none.
    pub category_text: Option<String>,
    /// The longer explanation, if the command declared one.
    pub description: Option<String>,
    /// The effective primary chord (user rebinds merged in), or `None` when the
    /// command has no keystroke at all, which is normal for a palette-only command.
    pub keystroke: Option<KeyStroke>,
    /// Whether the command's own `enabled_when` predicate currently says yes.
    pub enabled: bool,
    /// The intent name activation sends. Falls back to [`Self::id`] when the command
    /// declared no explicit intent, exactly as the keystroke dispatcher does.
    pub intent: &'static str,
}

impl PaletteCommand {
    /// The text a query is matched against: category and name together, so
    /// `file new` finds a New command filed under File. The displayed category,
    /// not the key, because the reader types what they read: `fichier nouveau`
    /// in a French interface.
    fn haystack(&self) -> String {
        match &self.category_text {
            Some(cat) => format!("{cat} {}", self.name),
            None => self.name.clone(),
        }
    }
}

type IncludeFn = Rc<dyn Fn(&PaletteCommand) -> bool>;
type DismissFn = Rc<dyn Fn(&mut EventContext)>;

/// The parts of a palette its rows, its result list and its handlers share,
/// separated from the widget so they can be cloned into `'static` closures.
#[derive(Clone)]
struct PaletteState {
    query: Signal<String>,
    /// The highlighted row: what Enter runs, what the row tint follows, what a
    /// screen reader is told is selected. One signal, so the three cannot
    /// drift apart.
    highlight: Signal<usize>,
    /// The rows on screen. The key handler acts on exactly what the reader is
    /// looking at rather than re-deriving the list and risking a different
    /// answer.
    rows: Rc<RefCell<Vec<Rc<PaletteEntry>>>>,
    /// How many rows match, for the palette's own accessible description.
    count: Signal<usize>,
    /// The query the rows were last built for. `None` before the first build.
    last_query: Rc<RefCell<Option<String>>>,
    /// The result list, to keep the highlight scrolled into view.
    list: Rc<RefCell<Option<ListViewHandle>>>,
    /// How many rows fit in the list's viewport: one PageUp / PageDown.
    page_rows: Rc<Cell<usize>>,
    /// Whether [`CommandPalette::present`] put this palette up. A presented
    /// palette hands focus back before a command runs; an embedded one keeps
    /// it.
    presented: Rc<Cell<bool>>,
    /// The command whose action presented this palette (`app.palette` bound
    /// to Ctrl+Shift+P), if an action did: its chord, pressed in the palette,
    /// closes it again, as the toggle it is to the user. The palette is modal,
    /// so the page's own shortcut does not reach into it.
    toggle: Rc<Cell<Option<&'static str>>>,
    on_dismiss: Rc<RefCell<Option<DismissFn>>>,
    /// Whether the dismissal hook has already run.
    ///
    /// It is reachable from two directions, the palette's own Escape /
    /// activate path and the modal's `on_dismiss` for the routes the overlay
    /// system handles before the palette ever sees the key, and a caller that
    /// installed it to release a resource must not be called twice.
    dismissed: Rc<Cell<bool>>,
    /// The result list's node, published for the search field's `controls`
    /// relation. `None` while nothing matches.
    listbox_id: Signal<Option<WidgetId>>,
    /// The highlighted row's node, published for the search field's
    /// `active_descendant`. `None` when the list is empty, or while the
    /// highlighted row is outside the realized virtualization window.
    active_row: Signal<Option<WidgetId>>,
}

impl PaletteState {
    fn new() -> Self {
        Self {
            query: Signal::new(String::new()),
            highlight: Signal::new(0),
            rows: Rc::new(RefCell::new(Vec::new())),
            count: Signal::new(0),
            last_query: Rc::new(RefCell::new(None)),
            list: Rc::new(RefCell::new(None)),
            page_rows: Rc::new(Cell::new(1)),
            presented: Rc::new(Cell::new(false)),
            toggle: Rc::new(Cell::new(None)),
            on_dismiss: Rc::new(RefCell::new(None)),
            dismissed: Rc::new(Cell::new(false)),
            listbox_id: Signal::new(None),
            active_row: Signal::new(None),
        }
    }

    fn len(&self) -> usize {
        self.rows.borrow().len()
    }

    /// Move the highlight to `index`. Writes nothing when it is already
    /// there, so a rebuild that keeps the reader's place notifies no row.
    fn set_highlight(&self, index: usize) {
        if self.highlight.get() != index {
            self.highlight.set(index);
        }
    }

    /// Move the highlight to `index` and scroll it into view.
    fn move_highlight(&self, index: usize) {
        self.set_highlight(index);
        if let Some(list) = self.list.borrow().as_ref() {
            list.ensure_index_visible(index);
        }
    }

    /// One row up or down, wrapping at either end, as a menu does.
    fn step(&self, down: bool) {
        let len = self.len();
        if len == 0 {
            return;
        }
        let current = self.highlight.get().min(len - 1);
        let next = if down {
            (current + 1) % len
        } else {
            (current + len - 1) % len
        };
        self.move_highlight(next);
    }

    /// One viewport up or down, stopping at either end.
    fn page(&self, down: bool) {
        let len = self.len();
        if len == 0 {
            return;
        }
        let page = self.page_rows.get().max(1);
        let current = self.highlight.get().min(len - 1);
        let next = if down {
            (current + page).min(len - 1)
        } else {
            current.saturating_sub(page)
        };
        self.move_highlight(next);
    }

    /// Run the highlighted command and close the palette.
    ///
    /// The palette closes first and the intent is sent from the focus that
    /// closing hands back ([`EventContext::send_intent_from_focus`]), which
    /// is how the command's own keystroke would have been dispatched: the
    /// editor the palette was opened over hears it, along with its ancestors
    /// and the window-global actions. Sent from the palette, it would start
    /// from a surface that is already gone.
    fn activate_highlighted(&self, ctx: &mut EventContext) {
        let picked = self.rows.borrow().get(self.highlight.get()).cloned();
        let Some(entry) = picked else { return };
        if !entry.command.enabled {
            // Reachable only with `show_disabled`, where a greyed row is
            // displayed precisely to say "not now": running it anyway would
            // make the grey a lie.
            return;
        }
        self.dismiss(ctx);
        ctx.send_intent_from_focus(Intent::new(entry.command.intent));
    }

    fn dismiss(&self, ctx: &mut EventContext) {
        if self.dismissed.replace(true) {
            return;
        }
        let dismiss = self.on_dismiss.borrow().clone();
        if let Some(dismiss) = dismiss {
            dismiss(ctx);
        }
    }
}

/// Type-to-run access to every registered command. See the [module docs](self).
pub struct CommandPalette {
    state: PaletteState,
    placeholder: Option<LocalizedString>,
    empty_text: Option<LocalizedString>,
    include: Option<IncludeFn>,
    show_disabled: bool,
    field_id: Option<WidgetId>,
    divider_id: Option<WidgetId>,
    results_id: Option<WidgetId>,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandPalette {
    /// A palette over every command in the tree's registry.
    pub fn new() -> Self {
        Self {
            state: PaletteState::new(),
            placeholder: None,
            empty_text: None,
            include: None,
            show_disabled: false,
            field_id: None,
            divider_id: None,
            results_id: None,
        }
    }

    /// Replace the search field's placeholder text.
    pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// Replace the text shown when nothing matches the query.
    pub fn empty_text(mut self, text: impl Into<LocalizedString>) -> Self {
        self.empty_text = Some(text.into());
        self
    }

    /// Keep only the commands this predicate accepts.
    ///
    /// The usual reasons are to hide the command that opens the palette itself, and
    /// to drop registry entries that are key bindings rather than commands a person
    /// would look for by name.
    pub fn include(mut self, f: impl Fn(&PaletteCommand) -> bool + 'static) -> Self {
        self.include = Some(Rc::new(f));
        self
    }

    /// Run this after a command is activated, and when Escape is pressed.
    ///
    /// [`present`](Self::present) installs its own, so this is for callers embedding
    /// the palette in a surface they manage themselves.
    pub fn on_dismiss(self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        *self.state.on_dismiss.borrow_mut() = Some(Rc::new(f));
        self
    }

    /// Also list commands whose `enabled_when` predicate currently says no, greyed
    /// out and inert. Off by default: a palette answers "what can I do now", and a
    /// row that cannot run is a row that has to be explained.
    pub fn show_disabled(mut self, show: bool) -> Self {
        self.show_disabled = show;
        self
    }

    /// The query signal, so a caller can seed or observe what was typed.
    pub fn query_signal(&self) -> Signal<String> {
        self.state.query.clone()
    }

    /// Show the palette near the top of the window, dismissed by Escape or a
    /// click outside. See the [module docs](self) on why this is window-level.
    pub fn present(self, ctx: &mut EventContext) {
        self.state.presented.set(true);
        self.state.toggle.set(ctx.handling_intent());
        // The palette's own Escape handler never runs for a presented palette:
        // the overlay system claims Escape first and tears the modal down
        // without it, and the same is true of a press outside. So a caller's
        // `on_dismiss`, installed to release whatever opening the palette
        // reserved, only ever fired when a command was actually run. Route
        // the framework's dismissal into the same latched hook.
        //
        // Only the caller's own callback is carried here, never the default
        // `ctx.dismiss_modal()` installed below: on this path the modal is
        // already going away, and that call would set a deferred flag spent
        // on whatever modal is topmost when it is read.
        let user_dismiss = self.state.on_dismiss.borrow().clone();
        let already_dismissed = self.state.dismissed.clone();
        let palette = match user_dismiss.clone() {
            None => self.on_dismiss(|ctx| ctx.dismiss_modal()),
            // Close the modal as well as running the caller's hook: a
            // palette that ran a command and stayed up would leave the
            // command's focus target under it.
            Some(user) => self.on_dismiss(move |ctx| {
                user(ctx);
                ctx.dismiss_modal();
            }),
        };
        let mut inner = Some(palette);
        ctx.present_modal(
            ModalRequest::deferred(move |tree| {
                let palette = inner
                    .take()
                    .expect("CommandPalette present closure called twice");
                // The palette draws its own insets: the dialog padding would
                // float the search field in the middle of the panel.
                tree.add(ModalContainer::new(palette).padding(0.0))
            })
            .presentation(ModalPresentation::InTree)
            .close_behavior(ModalCloseBehavior::EscapeOrClickOutside)
            // Top-anchored, so the search field stays where it is while the
            // list under it narrows.
            .placement(ModalPlacement::Top {
                offset: PALETTE_TOP_OFFSET,
            })
            .on_dismiss(Rc::new(move |_reason, ctx: &mut EventContext| {
                if already_dismissed.replace(true) {
                    return;
                }
                if let Some(cb) = &user_dismiss {
                    cb(ctx);
                }
            })),
        );
    }
}

/// Whether `id` is a command that runs on the focused editor: a standard edit
/// command, or one of the editors' own.
fn acts_on_the_focused_editor(id: &str) -> bool {
    StandardEdit::from_id(id).is_some()
        || crate::editor_shortcuts::FormatShortcut::from_id(id).is_some()
        || crate::editor_shortcuts::CodeShortcut::from_id(id).is_some()
}

impl std::fmt::Debug for CommandPalette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandPalette")
            .field("query", &self.state.query.get())
            .field("rows", &self.state.len())
            .finish_non_exhaustive()
    }
}

impl Widget for CommandPalette {
    /// Built once. Neither typing nor moving the highlight rebuilds the
    /// palette: the query rebuilds only the result list, a sibling of the
    /// search field, and the highlight repaints the rows.
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The match count is this node's accessible description.
        self.state.count.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

        let placeholder = self
            .placeholder
            .clone()
            .unwrap_or_else(|| tr_widget!(command_palette_placeholder()));
        let submit_state = self.state.clone();
        let field = SearchField::new(self.state.query.clone())
            .placeholder(placeholder)
            .label(tr_widget!(command_palette_title()))
            .on_submit_fn(move |ctx| submit_state.activate_highlighted(ctx))
            // The ARIA combobox pattern. Focus never leaves this field (that
            // is what makes a palette feel like one), so the highlight has to
            // be announced through the field's own AT node. `SearchField`
            // forwards both down to the focusable `TextInputField`, the only
            // node whose `active_descendant` assistive technology follows.
            .drives_listbox(self.state.listbox_id.clone(), self.state.active_row.clone());

        // `Live::Off` on each part of the content. The palette's own node is
        // a polite live region, and `accesskit_consumer` hands a node's
        // politeness down to every descendant that sets none (node.rs:
        // 906-910), while all three adapters announce each named node that
        // enters the filtered tree, or is renamed there, with an inherited
        // politeness (atspi_common adapter.rs:71-77 and node.rs:610-622,
        // windows adapter.rs:255-263, macos event.rs:236-241). Without it,
        // opening the palette announced the field and every row, and each
        // keystroke announced the rows it brought in, each cutting off the
        // one before.
        self.field_id = Some(
            ctx.add(
                Padding::uniform(FIELD_PADDING)
                    .child(field)
                    .access_live(Live::Off),
            ),
        );
        self.divider_id = Some(ctx.add(Divider::horizontal()));
        let config = Rc::new(ResultsConfig {
            include: self.include.clone(),
            show_disabled: self.show_disabled,
            empty_text: self.empty_text.clone(),
        });
        self.results_id =
            Some(ctx.add(PaletteResults::new(self.state.clone(), config).access_live(Live::Off)));

        // The command that presented the palette closes it again, by its
        // chord (the page's shortcut does not apply in a modal, so the key
        // falls through to here) and by its intent (sent here when its
        // shortcut has no owner, and so applies everywhere).
        let toggle_keys: Vec<KeyStroke> = match self.state.toggle.get() {
            Some(toggle) => {
                let close_state = self.state.clone();
                ctx.register_action(
                    teksilo_core::Action::new(toggle)
                        .on_invoke(move |_, ctx| close_state.dismiss(ctx)),
                );
                ctx.shortcut_registry()
                    .iter_effective()
                    .filter(|shortcut| shortcut.shortcut.intent_name() == toggle)
                    .flat_map(|shortcut| [shortcut.primary, shortcut.secondary])
                    .flatten()
                    .collect()
            }
            None => Vec::new(),
        };

        let key_state = self.state.clone();
        ctx.apply_self_handlers(HandlerSet::new().on_key(move |ev, ctx| {
            // Home and End are not here: they belong to the search field,
            // which moves its caret with them.
            let WidgetEvent::KeyDown { key, modifiers, .. } = ev else {
                return EventResponse::Ignored;
            };
            if toggle_keys.contains(&KeyStroke::new(*key, *modifiers)) {
                key_state.dismiss(ctx);
                return EventResponse::Handled;
            }
            match key {
                Key::ArrowDown => key_state.step(true),
                Key::ArrowUp => key_state.step(false),
                Key::PageDown => key_state.page(true),
                Key::PageUp => key_state.page(false),
                Key::Escape => key_state.dismiss(ctx),
                _ => return EventResponse::Ignored,
            }
            EventResponse::Handled
        }));

        self.children()
    }

    fn accessibility(&self, node: &mut AccessNodeBuilder) {
        node.set_role(Role::Dialog);
        // An unnamed dialog is announced as "dialog" and nothing else, which
        // tells a screen-reader user that something opened but not what.
        node.set_name(
            tr_widget!(command_palette_title())
                .resolve_now()
                .to_string(),
        );
        node.set_modal();
        // How many commands the query currently matches: the one fact a
        // sighted user reads off the list at a glance and a screen-reader user
        // otherwise has to arrow through the whole list to learn.
        let count = self.state.count.get();
        node.set_description(
            tr_widget!(command_palette_result_count(count = count as i64)).resolve_now(),
        );
        node.set_live(Live::Polite);
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let width = proposal.width.unwrap_or(PALETTE_WIDTH);
        let at_width = SizeProposal::with_width(width);
        let fixed: f32 = [self.field_id, self.divider_id]
            .into_iter()
            .flatten()
            .filter_map(|id| ctx.child_size(id, at_width))
            .map(|s| s.height)
            .sum();
        let results = self
            .results_id
            .and_then(|id| ctx.child_layout_response(id, at_width));
        let (ideal, min) = match results {
            Some(r) => (fixed + r.size.height, fixed + r.min.height),
            None => (fixed, fixed),
        };
        // As tall as the content and no taller, so the panel hugs a short
        // list; squeezed by a short window, the list gives up rows first.
        let height = proposal.height.map_or(ideal, |h| h.min(ideal));
        LayoutResponse::shrinkable(
            Size::new(width, height),
            Size::new(width, min.min(height)),
            1.0,
        )
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let at_width = SizeProposal::with_width(bounds.width);
        let mut y = bounds.y;
        for child in children.iter_mut() {
            let height = if Some(child.id) == self.results_id {
                (bounds.bottom() - y).max(0.0)
            } else {
                ctx.child_size(child.id, at_width)
                    .map(|s| s.height)
                    .unwrap_or(0.0)
            };
            child.origin = Point::new(bounds.x, y);
            child.size = Size::new(bounds.width, height);
            y += height;
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        [self.field_id, self.divider_id, self.results_id]
            .into_iter()
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests;
