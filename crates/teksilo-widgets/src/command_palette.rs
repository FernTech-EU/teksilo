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
//! # Presenting it
//!
//! [`CommandPalette::present`] shows it centered, dismissed by Escape or a click
//! outside:
//!
//! ```ignore
//! ctx.register_action_global(Action::new("app.command_palette").on_invoke(|_, ctx| {
//!     CommandPalette::new().present(ctx);
//! }));
//! ```
//!
//! Presenting it as a **window-level** modal is deliberate, not incidental: a palette
//! is routinely opened from a menu, and a menu is itself a transient overlay.
//! Anchoring to the invoking widget would render the palette inside the menu that
//! opened it, positioned against a surface that is about to disappear.
//!
//! # Matching
//!
//! Typing filters by subsequence, not substring, so `ndw` finds "New Window" and
//! `expdoc` finds "Export document". Matches score higher when the typed letters land
//! consecutively and on word starts, so the most literal reading of a query sorts
//! first. An empty query lists everything in the registry's own deterministic
//! `(category, id)` order. The category takes part in matching, so `file new` finds
//! the New command filed under File.
//!
//! # Keyboard
//!
//! Focus stays in the search field throughout — that is what makes a palette feel
//! like one. Arrow keys are not editing keys for the field, so they bubble to the
//! palette's own handler, which moves the highlight and scrolls it into view. Enter
//! runs the highlighted command; Escape dismisses.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::Role;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::intent::Intent;
use teksilo_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{ListModel, SelectionMode, SelectionModel};
use teksilo_i18n::{LocalizedString, lit, tr_widget};
use teksilo_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};

use crate::dialog::ModalContainer;
use crate::keystroke_format::format_keystroke;
use crate::list_view::ListView;
use crate::primitives::{
    Expand, FixedSize, HStack, Padding, RectWidget, Spacer, TextWidget, VStack, ZStack,
};
use crate::search_field::SearchField;

/// Presented size. Wide enough for a command name plus its chord without either
/// having to ellipsize in the common case.
const PALETTE_WIDTH: u32 = 560;
const PALETTE_HEIGHT: u32 = 420;
/// Row height fed to the list's own metrics; two lines of text plus padding.
const ROW_HEIGHT: f32 = 44.0;
/// Width of the leading bar marking the highlighted row — the non-colour half
/// of the highlight. 3 dp matches the selection edge `StandardListItem` draws.
const SELECTION_MARKER_WIDTH: f32 = 3.0;
/// How many rows the presented palette shows at once.
///
/// Derived from [`PALETTE_HEIGHT`] rather than measured, which is what lets the
/// keyboard scroll be computed without waiting on layout — the palette presents
/// itself at a fixed size, so the number is known. A caller embedding the palette in
/// a taller surface still scrolls correctly by pointer and still selects correctly by
/// keyboard; only the auto-scroll may leave the highlight a row from the edge.
const VISIBLE_ROWS: usize = 7;

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
    /// The grouping label, if the command declared one.
    pub category: Option<&'static str>,
    /// The longer explanation, if the command declared one.
    pub description: Option<String>,
    /// The effective primary chord — user rebinds merged in — or `None` when the
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
    /// `file new` finds a New command filed under File.
    fn haystack(&self) -> String {
        match self.category {
            Some(cat) => format!("{cat} {}", self.name),
            None => self.name.clone(),
        }
    }
}

type IncludeFn = Rc<dyn Fn(&PaletteCommand) -> bool>;
type DismissFn = Rc<dyn Fn(&mut EventContext)>;

/// The parts of a palette its event closures need, separated from the widget so they
/// can be cloned into `'static` handlers without cloning the widget itself.
#[derive(Clone)]
struct PaletteState {
    query: Signal<String>,
    selected: Signal<usize>,
    /// The rows currently on screen. The key handler acts on exactly what the reader
    /// is looking at rather than re-deriving the list and risking a different answer.
    rows: Rc<RefCell<Vec<PaletteCommand>>>,
    /// First row currently scrolled into view.
    ///
    /// Tracked here rather than read back off the list because the list is rebuilt
    /// from scratch on every keystroke — a scroll offset living on the widget would
    /// reset to the top each time the reader typed a letter.
    top_index: Signal<usize>,
    /// The query as of the last build, so a *changed* query can reset the highlight
    /// to the best match without an effect that would fire mid-build.
    last_query: Rc<RefCell<String>>,
    on_dismiss: Rc<RefCell<Option<DismissFn>>>,

    // ── Accessibility ───────────────────────────────────────────────────
    /// The result list's selection, mirroring [`Self::selected`].
    ///
    /// The highlight is `selected`; this exists so each realized row's
    /// `Role::ListBoxOption` reports `selected` truthfully. Without it every row
    /// answered "not selected" and the arrow keys moved a highlight no
    /// assistive technology could observe. Owned by the state, not rebuilt per
    /// build, so a pointer click on a row can be routed back into `selected`.
    selection: SelectionModel,
    /// The result `ListView`'s node, published for the search field's
    /// `controls` relation.
    listbox_id: Signal<Option<WidgetId>>,
    /// The highlighted row's node, published for the search field's
    /// `active_descendant`. `None` when the list is empty, or when the
    /// highlighted row is outside the realized virtualization window.
    active_row: Signal<Option<WidgetId>>,
}

impl PaletteState {
    fn new() -> Self {
        Self {
            query: Signal::new(String::new()),
            selected: Signal::new(0),
            rows: Rc::new(RefCell::new(Vec::new())),
            top_index: Signal::new(0),
            last_query: Rc::new(RefCell::new(String::new())),
            on_dismiss: Rc::new(RefCell::new(None)),
            selection: SelectionModel::new(SelectionMode::Single),
            listbox_id: Signal::new(None),
            active_row: Signal::new(None),
        }
    }

    /// Move the highlight to `index`, keeping the AT-visible selection with it.
    ///
    /// Every write to `selected` goes through here. The two must not drift:
    /// `selected` is what Enter runs and what the row tint follows, while
    /// `selection` is what a screen reader is told, and a palette that
    /// announces one row while running another is worse than one that
    /// announces nothing.
    fn set_selected(&self, index: usize) {
        self.selected.set(index);
        self.selection.select(index);
    }

    /// Move the highlight by `delta`, clamped to the list, and scroll it into view.
    fn step_selection(&self, delta: isize) {
        let len = self.rows.borrow().len();
        if len == 0 {
            return;
        }
        let current = self.selected.get() as isize;
        let next = (current + delta).clamp(0, len as isize - 1) as usize;
        self.set_selected(next);
        self.reveal(next);
    }

    /// Move the selection to an absolute row (Home / End).
    fn select_edge(&self, last: bool) {
        let len = self.rows.borrow().len();
        if len == 0 {
            return;
        }
        let target = if last { len - 1 } else { 0 };
        self.set_selected(target);
        self.reveal(target);
    }

    /// Scroll the minimum distance that brings row `index` into view.
    fn reveal(&self, index: usize) {
        let top = self.top_index.get();
        let new_top = if index < top {
            index
        } else if index >= top + VISIBLE_ROWS {
            index + 1 - VISIBLE_ROWS
        } else {
            top
        };
        if new_top != top {
            self.top_index.set(new_top);
        }
    }

    /// Send the highlighted command's intent, then dismiss.
    ///
    /// The intent is synthesized from the command's declared name, which is what the
    /// dispatcher sends for a chord with no custom activation closure — so a command
    /// reached from the palette and the same command reached from its keystroke
    /// arrive at the identical action.
    fn activate_selected(&self, ctx: &mut EventContext) {
        let picked = {
            let rows = self.rows.borrow();
            rows.get(self.selected.get()).cloned()
        };
        let Some(cmd) = picked else { return };
        if !cmd.enabled {
            // Reachable only with `show_disabled`, where a greyed row is displayed
            // precisely to say "not now" — running it anyway would make the grey a lie.
            return;
        }
        ctx.send_intent(Intent::new(cmd.intent));
        let dismiss = self.on_dismiss.borrow().clone();
        if let Some(dismiss) = dismiss {
            dismiss(ctx);
        }
    }

    fn dismiss(&self, ctx: &mut EventContext) {
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
    root_child_id: Option<WidgetId>,
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
            root_child_id: None,
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

    /// Show the palette centered in the window, dismissed by Escape or a click
    /// outside. See the [module docs](self) on why this is window-level.
    pub fn present(self, ctx: &mut EventContext) {
        let palette = if self.state.on_dismiss.borrow().is_none() {
            self.on_dismiss(|ctx| ctx.dismiss_modal())
        } else {
            self
        };
        let mut inner = Some(palette);
        ctx.present_modal(
            ModalRequest::deferred(move |tree| {
                let palette = inner
                    .take()
                    .expect("CommandPalette present closure called twice");
                tree.add(ModalContainer::new(palette))
            })
            .presentation(ModalPresentation::InTree)
            .close_behavior(ModalCloseBehavior::EscapeOrClickOutside)
            .size(PALETTE_WIDTH, PALETTE_HEIGHT),
        );
    }

    /// Read the registry, apply `include`, match against the query, and rank.
    fn visible_rows(&self, ctx: &BuildContext) -> Vec<PaletteCommand> {
        let needle = self.state.query.get().trim().to_lowercase();
        let mut scored: Vec<(i32, PaletteCommand)> = ctx
            .shortcut_registry()
            .iter_effective()
            .map(|eff| PaletteCommand {
                id: eff.shortcut.id,
                name: eff.shortcut.name.get(),
                category: eff.shortcut.category,
                description: eff.shortcut.description.as_ref().map(|d| d.get()),
                keystroke: eff.primary,
                enabled: eff.enabled,
                intent: eff.shortcut.intent_name(),
            })
            .filter(|cmd| self.show_disabled || cmd.enabled)
            .filter(|cmd| self.include.as_ref().is_none_or(|f| f(cmd)))
            .filter_map(|cmd| Some((fuzzy_score(&needle, &cmd.haystack())?, cmd)))
            .collect();
        // Highest score first. `iter_effective` already ordered by (category, id) and
        // `sort_by` is stable, so equal scores — which is every row when the query is
        // empty — keep exactly that order.
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored.into_iter().map(|(_, cmd)| cmd).collect()
    }
}

impl std::fmt::Debug for CommandPalette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandPalette")
            .field("query", &self.state.query.get())
            .field("rows", &self.state.rows.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Widget for CommandPalette {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Any registry change — a command registered, rebound, enabled — changes what
        // the palette should be showing.
        ctx.shortcut_version().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );
        self.state
            .query
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        self.state
            .selected
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        self.state
            .top_index
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let rows = self.visible_rows(ctx);
        // A changed query means a different list: send the highlight back to the best
        // match rather than leaving it on whatever now happens to sit at that index.
        let query_now = self.state.query.get();
        if *self.state.last_query.borrow() != query_now {
            *self.state.last_query.borrow_mut() = query_now;
            self.state.set_selected(0);
            self.state.top_index.set(0);
        }
        // Clamp before rendering, not after: a query that shortened the list must not
        // leave the highlight past the end for even one frame, or Enter would run
        // whichever row happens to sit at a stale index.
        if self.state.selected.get() >= rows.len() {
            self.state.set_selected(rows.len().saturating_sub(1));
        }
        // Keep the AT-visible selection on the highlight even when neither
        // branch above fired (first build, or a rebuild driven by the shortcut
        // registry rather than by a keystroke).
        if !rows.is_empty() && !self.state.selection.is_selected(self.state.selected.get()) {
            self.state.selection.select(self.state.selected.get());
        }
        *self.state.rows.borrow_mut() = rows.clone();
        let selected_index = self.state.selected.get();

        let placeholder = self
            .placeholder
            .clone()
            .unwrap_or_else(|| tr_widget!(command_palette_placeholder()));

        let submit_state = self.state.clone();
        let field = SearchField::new(self.state.query.clone())
            .placeholder(placeholder)
            .label(tr_widget!(command_palette_title()))
            .on_submit_fn(move |ctx| submit_state.activate_selected(ctx))
            // The ARIA combobox pattern. Focus never leaves this field — that
            // is what makes a palette feel like one — so the arrow-key
            // highlight has to be announced through the field's own AT node.
            // `SearchField` forwards both down to the focusable
            // `TextInputField`, the only node whose `active_descendant`
            // assistive technology follows.
            .drives_listbox(self.state.listbox_id.clone(), self.state.active_row.clone());

        // Stale entries would otherwise survive an empty result set and point
        // `active_descendant` at a destroyed node.
        self.state.listbox_id.set(None);
        self.state.active_row.set(None);

        let body: Box<dyn Widget> = if rows.is_empty() {
            let empty = self
                .empty_text
                .clone()
                .unwrap_or_else(|| tr_widget!(command_palette_empty()));
            Box::new(
                Padding::symmetric(14.0, 12.0).child(
                    TextWidget::new(empty)
                        .style(TextStyleRole::Body)
                        .color(TextRole::Secondary),
                ),
            )
        } else {
            let list = ListView::new(
                ListModel::from_vec(rows),
                move |index, cmd: &PaletteCommand, _row_selected| {
                    Box::new(command_row(cmd, index == selected_index))
                },
            )
            .item_height(ROW_HEIGHT)
            // Makes each row's `Role::ListBoxOption` report `selected` truthfully.
            .selection(self.state.selection.clone());
            // Take the realized-row map before the view moves into the tree.
            let row_ids = list.realized_row_ids();
            // The window the reader is looking at is state this widget owns, so it
            // survives the rebuild that every keystroke causes.
            list.scroll_to_index(self.state.top_index.get());

            // `ctx.add` builds the subtree synchronously, so by the time this
            // returns the body pane has already published its realized rows and
            // the highlighted row's id is resolvable — no deferred effect, no
            // frame of silence after an arrow key.
            let list_id = ctx.add(list);
            self.state.listbox_id.set(Some(list_id));
            let active = row_ids
                .borrow()
                .iter()
                .find(|(index, _)| *index == selected_index)
                .map(|(_, id)| *id);
            self.state.active_row.set(active);

            Box::new(Expand::new().child_id(list_id))
        };

        let key_state = self.state.clone();
        // The column is pinned to the presented size rather than left to size itself.
        // `ModalContainer` sizes to its content, and the result list lives under an
        // `Expand` — with no bounded height to fill, the list measures zero and the
        // palette collapses to just its search field, which is exactly what shipped
        // the first time this was run. Same reason `AboutPanel` pins its card.
        let column = VStack::new()
            .spacing(4.0)
            .child(Padding::symmetric(8.0, 8.0).child(field))
            .add_child(ctx.add_boxed(body))
            .on_key(move |ev, ctx| match ev {
                WidgetEvent::KeyDown {
                    key: Key::ArrowDown,
                    ..
                } => {
                    key_state.step_selection(1);
                    EventResponse::Handled
                }
                WidgetEvent::KeyDown {
                    key: Key::ArrowUp, ..
                } => {
                    key_state.step_selection(-1);
                    EventResponse::Handled
                }
                // A palette is a list, and a long result set is exactly
                // where jumping to an end matters — these were the only list
                // keys it did not answer.
                WidgetEvent::KeyDown { key: Key::Home, .. } => {
                    key_state.select_edge(false);
                    EventResponse::Handled
                }
                WidgetEvent::KeyDown { key: Key::End, .. } => {
                    key_state.select_edge(true);
                    EventResponse::Handled
                }
                WidgetEvent::KeyDown {
                    key: Key::PageUp, ..
                } => {
                    key_state.step_selection(-(VISIBLE_ROWS as isize));
                    EventResponse::Handled
                }
                WidgetEvent::KeyDown {
                    key: Key::PageDown, ..
                } => {
                    key_state.step_selection(VISIBLE_ROWS as isize);
                    EventResponse::Handled
                }
                WidgetEvent::KeyDown {
                    key: Key::Escape, ..
                } => {
                    key_state.dismiss(ctx);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });

        let root = ctx.add_boxed(Box::new(
            FixedSize::new()
                .width(PALETTE_WIDTH as f32)
                .height(PALETTE_HEIGHT as f32)
                .child(column),
        ));
        self.root_child_id = Some(root);
        vec![root]
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
        // How many commands the query currently matches — the one fact a
        // sighted user reads off the list at a glance and a screen-reader user
        // otherwise has to arrow through the whole list to learn.
        let count = self.state.rows.borrow().len();
        node.set_description(
            tr_widget!(command_palette_result_count(count = count as i64)).resolve_now(),
        );
        node.set_live(teksilo_core::accesskit::Live::Polite);
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .map(LayoutResponse::from)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
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
        self.root_child_id.into_iter().collect()
    }
}

/// One rendered row: name over category on the left, chord on the right, on a
/// selection-tinted ground.
fn command_row(cmd: &PaletteCommand, selected: bool) -> impl Widget + 'static {
    let name_color = if cmd.enabled {
        TextRole::Primary
    } else {
        TextRole::Disabled
    };
    let mut left = VStack::new().spacing(1.0).child(
        TextWidget::new(lit!(cmd.name.clone()))
            .style(TextStyleRole::Body)
            .color(name_color)
            .single_line(),
    );
    // The category is the row's disambiguator — two features' "Close" read identically
    // without it — so it shows always, not only while searching.
    if let Some(cat) = cmd.category {
        left = left.child(
            TextWidget::new(lit!(cat.to_string()))
                .style(TextStyleRole::Small)
                .color(TextRole::Secondary)
                .single_line(),
        );
    }

    // An unbound command is the normal case here, not a defect, so it gets empty space
    // rather than the em-dash a settings table uses to mean "nothing bound yet".
    let chord = cmd.keystroke.map(format_keystroke).unwrap_or_default();

    let row = HStack::new()
        .spacing(10.0)
        .child(left)
        .child(Spacer::new())
        .child(
            TextWidget::new(lit!(chord))
                .style(TextStyleRole::Small)
                .color(TextRole::Secondary)
                .single_line(),
        );

    // The highlight carries two channels, not one. A background tint alone is a
    // colour-only distinction (WCAG 1.4.1) and disappears entirely under a
    // high-contrast or forced-colours setting; the leading bar is a shape, so it
    // survives both. Same reading as the selection edge `StandardListItem`
    // draws — a palette row is a list row wearing different padding.
    let bg = RectWidget::new().background(if selected {
        SurfaceRole::Selected
    } else {
        SurfaceRole::Transparent
    });
    let marker =
        FixedSize::new()
            .width(SELECTION_MARKER_WIDTH)
            .child(RectWidget::new().background(if selected {
                ColorProp::from(BorderRole::Focused)
            } else {
                ColorProp::from(SurfaceRole::Transparent)
            }));

    ZStack::new().child(bg).child(
        HStack::new()
            .child(marker)
            .child(Expand::new().child(Padding::symmetric(6.0, 10.0).child(row))),
    )
}

// ── Matching ────────────────────────────────────────────────────────────────

/// Score `haystack` against an already-lowercased `needle`, or `None` when the needle
/// is not a subsequence of it.
///
/// Higher is better. The weights encode three preferences, strongest first: a run of
/// typed letters landing consecutively beats the same letters scattered; a letter
/// landing at the start of a word beats one landing mid-word; and an early match beats
/// a late one. That is enough to put the row a person meant at the top for the queries
/// people actually type, without a general-purpose ranking library.
///
/// A space in the needle matches a space in the haystack like any other character, so
/// `file new` behaves as a two-word query against the "category name" haystack.
fn fuzzy_score(needle: &str, haystack: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    const CONSECUTIVE_BONUS: i32 = 15;
    const WORD_START_BONUS: i32 = 20;
    const GAP_PENALTY: i32 = 1;
    const MAX_GAP_PENALTY: i32 = 20;

    let hay: Vec<char> = haystack.to_lowercase().chars().collect();
    // Word starts are read off the *original* casing, so a TitleCase or camelCase
    // boundary counts even with no separator before it.
    let raw: Vec<char> = haystack.chars().collect();
    let is_word_start = |i: usize| -> bool {
        if i == 0 {
            return true;
        }
        // `hay` is the lowercased haystack and `raw` the original. Lowercasing can
        // change the character count for some scripts, so only consult `raw` when the
        // two line up; otherwise fall back to the separator test alone.
        let Some(&prev) = raw.get(i.wrapping_sub(1)) else {
            return true;
        };
        let Some(&cur) = raw.get(i) else {
            return false;
        };
        !prev.is_alphanumeric() || (prev.is_lowercase() && cur.is_uppercase())
    };

    let mut score = 0;
    let mut hay_pos = 0usize;
    let mut last_match: Option<usize> = None;
    // Length of the run of consecutive matches ending at the previous character. The
    // bonus compounds with it, which is what makes a whole word typed out beat the
    // same letters collected from the start of several words: `exp` must find
    // "Export", not "Edit XML Properties", even though the latter matches three word
    // starts and the former only one.
    let mut streak = 0;

    for want in needle.chars() {
        let found = hay[hay_pos..].iter().position(|c| *c == want)? + hay_pos;
        match last_match {
            Some(prev) if found == prev + 1 => {
                streak += 1;
                score += CONSECUTIVE_BONUS * streak;
            }
            Some(prev) => {
                streak = 0;
                score -= ((found - prev - 1) as i32 * GAP_PENALTY).min(MAX_GAP_PENALTY);
            }
            // Reward matching near the front, so `new` prefers "New Window" over a
            // command that merely contains the letters later on.
            None => {
                streak = 0;
                score -= (found as i32 * GAP_PENALTY).min(MAX_GAP_PENALTY);
            }
        }
        if is_word_start(found) {
            score += WORD_START_BONUS;
        }
        last_match = Some(found);
        hay_pos = found + 1;
    }
    Some(score)
}

#[cfg(test)]
mod tests;
