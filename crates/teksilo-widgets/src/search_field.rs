// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! SearchField — a [`TextInput`] preset
//! configured for search workflows: leading magnifier glyph, default-on
//! clear-X, and an optional anchored suggestions popover with keyboard
//! navigation and the ARIA combobox-with-listbox accessibility pattern.
//! The popover is shown via `OverlayRequest` so it floats above sibling
//! content and escapes ancestor clipping (same pattern as `ComboBox`).
//!
//! ```ignore
//! let query = ctx.signal(String::new());
//! SearchField::new(query.clone())
//!     .placeholder("Search documents")
//!     .with_suggestions(|prefix| {
//!         FRUITS.iter()
//!             .filter(|f| f.to_lowercase().starts_with(&prefix.to_lowercase()))
//!             .map(|s| s.to_string())
//!             .collect()
//!     })
//!     .on_select(|value, _ctx| println!("picked: {value}"))
//!     .on_submit_fn(|ctx| ctx.send_intent(AppIntent::Search))
//! ```
//!
//! ## Design — comparison with searchable [`ComboBox`](crate::combo_box::ComboBox)
//!
//! A searchable `ComboBox` and a `SearchField` are visually similar
//! but semantically different:
//!
//! - **ComboBox** is a *value picker* — the bound state is the
//!   selected item from a known list. The text input is a transient
//!   filter, embedded inside the dropdown popup; the closed combo
//!   shows the selected value, not the user's query.
//! - **SearchField** is a *query input* — the bound state is the
//!   query string itself. The text input is always visible at the
//!   top level; suggestions are completion hints, not the source of
//!   truth. The bound `Signal<String>` keeps whatever the user
//!   typed, even if no suggestion matches.
//!
//! The two share the same dropdown-of-options machinery in spirit;
//! a future refactor could lift a common `OverlayList<T>` primitive
//! out of both. For now they're separate so each can keep a small
//! API surface tuned to its semantics.
//!
//! ## Accessibility
//!
//! A `SearchField` is one node to assistive technology: its editing field,
//! the node that holds focus, with `Role::SearchInput` and the name given by
//! [`SearchField::label`]. The `SearchField`'s own node is structure and is
//! left out of the tree, and what an application attaches to the
//! `SearchField`'s id (`access_label`, a `FormLayout`'s `labelled_by`,
//! `access_described_by`) lands on the field. Focusing the `SearchField`
//! (`ctx.focus(id)`, an `initial_focus_hint`) focuses its field.
//!
//! With suggestions ([`SearchField::with_suggestions`]) the field is also
//! the combobox: `HasPopup::Listbox`, `AutoComplete::List`, `expanded`
//! (`true` while the popup is open, `false` otherwise), `controls` naming
//! the popup's `Role::ListBox` while it is open, and `active_descendant`
//! naming the highlighted row while the popup is open, so arrowing through
//! the suggestions is announced while focus stays in the field. Without
//! suggestions it claims no popup. Each row is `Role::ListBoxOption` with
//! `set_selected(is_highlighted)` and `set_position_in_set(idx + 1)`; the
//! `Role::ListBox` container carries the matching `set_size_of_set(total)`,
//! since AccessKit resolves a set size by walking up from the item. Together
//! they let screen readers announce "Apple, 1 of 5".
//!
//! [`SearchField::drives_listbox`] gives the field `controls` and
//! `active_descendant` for a list its host owns, without a popup. With both,
//! the built-in popup's row is announced while the popup is open and the
//! host's list otherwise.

use std::cell::Cell;
use std::rc::Rc;
use teksilo_i18n::lit;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayPlacement, OverlayRequest,
};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{PopoverStyleConfig, PopoverVariant};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, HAlignment, SurfaceRole, TextRole, TextStyleRole};

use crate::EditSource;
use crate::icon_button::BuiltInIcons;
use crate::primitives::{
    Center, FixedSize, MinSize, Padding, RectWidget, TextWidget, VStack, ZStack,
};
use crate::text_input::TextInput;
use teksilo_i18n::LocalizedString;

/// Default cap on the number of suggestions rendered in the popup.
/// Lives in code (not in `SearchFieldStyle`) because it's a behavior
/// default, not a visual dimension — apps override per-instance via
/// [`SearchField::max_suggestions`].
const DEFAULT_MAX_SUGGESTIONS: usize = 8;

type SuggestionProvider = Rc<dyn Fn(&str) -> Vec<String>>;
type OnSelect = Rc<dyn Fn(&str, &mut EventContext)>;
type OnSubmit = Rc<dyn Fn(&mut EventContext)>;

fn search_glyph(glyph_size: f32, slot_width: f32) -> impl Widget + 'static {
    let icon = (BuiltInIcons::global().search)()
        .icon_size(glyph_size)
        .color(TextRole::Secondary);
    // `height` is load-bearing — without it, `Center`'s
    // `proposal.resolve(0, 0)` collapses the unspecified-height side
    // to zero and the slot disappears even though its width is set.
    FixedSize::new()
        .width(slot_width)
        .height(glyph_size)
        .child(Center::new().child(icon))
}

/// A search input with optional inline suggestions popup.
pub struct SearchField {
    text: Signal<String>,
    placeholder: Option<LocalizedString>,
    label: Option<LocalizedString>,
    /// Initial enabled-state; forwarded to the arena at build time.
    enabled: Prop<bool>,
    suggestion_provider: Option<SuggestionProvider>,
    max_suggestions: usize,
    min_chars: usize,
    on_select: Option<OnSelect>,
    on_submit: Option<OnSubmit>,
    /// Forwarded to the inner `TextInput`; see [`SearchField::on_text_inserted`].
    on_text_inserted: Option<Rc<dyn Fn(EditSource, usize)>>,
    /// Forwarded to the inner `TextInput`; see [`SearchField::writing_tools`].
    writing_tools: Prop<bool>,
    /// ARIA combobox wiring for an **externally owned** listbox — a command
    /// palette's result list, not this field's own suggestion popup — forwarded
    /// to the inner `TextInput` and from there to the focusable
    /// `TextInputField`. Independent of `row_ids_slot` / `highlighted_slot`,
    /// which serve the built-in suggestion panel.
    external_active_descendant: Option<Signal<Option<WidgetId>>>,
    external_controls: Option<Signal<Option<WidgetId>>>,
    /// Build state — populated in `build()`.
    root_child_id: Option<WidgetId>,
    /// The inner `TextInput`'s editing field, the node that holds focus and
    /// stands for this widget to assistive technology (see
    /// `accessibility_proxy`). Filled in when the `TextInput` builds.
    field_id: Rc<Cell<Option<WidgetId>>>,
    /// Pre-created suggestions panel content. Inserted as a dormant
    /// arena root in `build()` and shown as an overlay anchored to
    /// the field via `OverlayRequest`. Added detached, so the
    /// framework owns the teardown — the previous panel is reaped on
    /// rebuild and the live one when the field is destroyed.
    panel_content_id: Option<WidgetId>,
    /// Per-call style override.
    style_override: Option<teksilo_core::styles::SharedSearchFieldStyle>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl SearchField {
    /// Create a search field bound to `text`, the reactive query string.
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            placeholder: None,
            label: None,
            enabled: Prop::Static(true),
            suggestion_provider: None,
            max_suggestions: DEFAULT_MAX_SUGGESTIONS,
            min_chars: 1,
            on_select: None,
            on_submit: None,
            on_text_inserted: None,
            writing_tools: Prop::Static(true),
            external_active_descendant: None,
            external_controls: None,
            root_child_id: None,
            field_id: Rc::new(Cell::new(None)),
            panel_content_id: None,
            style_override: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Per-call SearchFieldStyle override.
    pub fn style(mut self, style: impl teksilo_core::styles::SearchFieldStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Set the placeholder text shown when the query is empty.
    pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.placeholder = Some(ls);
        self
    }

    /// Set an accessible label for the field (announced by screen readers, not visually shown).
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Wire this field to a listbox **the caller owns**, so arrow keys that
    /// move a highlight through that list are announced while focus stays here
    /// (the ARIA combobox pattern). `listbox` is the list's node, `active` the
    /// currently-highlighted row's node; both are forwarded to the inner
    /// `TextInputField`, which is the node that actually holds focus and
    /// therefore the only one whose `active_descendant` assistive technology
    /// follows.
    ///
    /// This is for a search field driving a list built by its *host* — a
    /// command palette, a filter box above a results view. The built-in
    /// suggestion popup (`suggestions`) wires itself and needs none of this.
    pub fn drives_listbox(
        mut self,
        listbox: Signal<Option<WidgetId>>,
        active: Signal<Option<WidgetId>>,
    ) -> Self {
        self.external_controls = Some(listbox);
        self.external_active_descendant = Some(active);
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to the
    /// arena at build time; a bound `Signal<bool>` updates live.
    pub fn enabled(mut self, on: impl Into<Prop<bool>>) -> Self {
        self.enabled = on.into();
        self
    }

    /// Install a callback invoked when the user presses Enter (or activates the search action).
    pub fn on_submit_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    /// Callback fired where text lands in the field, with the [`EditSource`]
    /// it came through and how many characters it was. Counts only, never
    /// content; not fired when the application writes the bound
    /// `Signal<String>`, nor when a picked suggestion replaces the query.
    /// Forwarded to the inner field; see
    /// [`TextInputField::on_text_inserted`](crate::primitives::TextInputField::on_text_inserted).
    pub fn on_text_inserted(mut self, f: impl Fn(EditSource, usize) + 'static) -> Self {
        self.on_text_inserted = Some(Rc::new(f));
        self
    }

    /// Whether the system's Writing Tools may act on this field: on macOS,
    /// whether they may read its text and replace it. Other platforms have no
    /// such feature. Defaults to `true`; static or a `Signal<bool>`. Forwarded
    /// to the inner field; see
    /// [`TextInputField::writing_tools`](crate::primitives::TextInputField::writing_tools).
    pub fn writing_tools(mut self, allowed: impl Into<Prop<bool>>) -> Self {
        self.writing_tools = allowed.into();
        self
    }

    /// Provider that returns suggestions for the current query string.
    /// When set, the popup appears below the field as soon as the
    /// user types at least [`Self::min_chars`] characters and the
    /// provider returns a non-empty list.
    pub fn with_suggestions(mut self, f: impl Fn(&str) -> Vec<String> + 'static) -> Self {
        self.suggestion_provider = Some(Rc::new(f));
        self
    }

    /// Cap the number of suggestions shown in the popup (default 8, minimum 1).
    pub fn max_suggestions(mut self, n: usize) -> Self {
        self.max_suggestions = n.max(1);
        self
    }

    /// Minimum number of characters the user must type before suggestions appear (default 1).
    pub fn min_chars(mut self, n: usize) -> Self {
        self.min_chars = n;
        self
    }

    /// Install a callback invoked when the user picks a suggestion (tap, Enter, or Space).
    pub fn on_select(mut self, f: impl Fn(&str, &mut EventContext) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Show a plain one-line tooltip after a hover delay.
    ///
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — calling this
    /// clears the other slots (last call wins).
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Show a registry-driven rich tooltip keyed by `key`.
    ///
    /// Mutually exclusive with the other tooltip setters — last call wins.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Show an inline rich tooltip with the given [`TooltipContent`](crate::tooltip::TooltipContent).
    ///
    /// Mutually exclusive with the other tooltip setters — last call wins.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Show a composite tooltip whose body is an arbitrary widget tree.
    ///
    /// Mutually exclusive with the other tooltip setters — last call wins.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl std::fmt::Debug for SearchField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchField")
            .field("placeholder", &self.placeholder)
            .field("max_suggestions", &self.max_suggestions)
            .field("min_chars", &self.min_chars)
            .finish_non_exhaustive()
    }
}

impl Widget for SearchField {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());

        // ── Reactive state ──────────────────────────────────────────
        // Suggestions list — recomputed on every text change.
        let suggestions: Signal<Vec<String>> = ctx.signal(Vec::new());
        // Currently highlighted row inside the popup. Driven by
        // ArrowUp / ArrowDown and by hover.
        let highlighted: Signal<Option<usize>> = ctx.signal::<Option<usize>>(None);
        // "User pressed Escape" / "User picked a suggestion" flag —
        // suppresses the popup until the user starts typing again or
        // refocuses the field. Reset on focus-gain and on any
        // non-Escape KeyDown.
        let dismissed: Signal<bool> = ctx.signal(false);
        // `overlay_open` mirrors the live overlay state: set to true by the
        // open helper before `ctx.show_overlay`, set to false by the dismiss
        // callback the overlay manager invokes (Escape, outside click,
        // programmatic dismiss). The field publishes it as `expanded`, so it
        // exists before the field is built. It is also the panel's reveal
        // gate: the suggestion list is built the first time the field
        // actually suggests something, not on every rebuild of the field.
        let overlay_open: Signal<bool> = ctx.signal(false);
        // What the suggestion panel publishes when it builds: its `ListBox`
        // and its rows, in display order. Each has one writer, the panel's
        // `build`; the field's relations are derived from them below.
        let panel_listbox: Signal<Option<WidgetId>> = ctx.signal::<Option<WidgetId>>(None);
        let panel_rows: Signal<Vec<WidgetId>> = ctx.signal(Vec::new());

        // ── TextInput with submit hook ──────────────────────────────
        // Built inline (matching DateEdit / TimeEdit / SpinBox) — no
        // helper method or stored Option<TextInput>, just direct
        // construction from the SearchField's own config fields.
        use crate::styles::recipe_search_field_style as sf;
        let placeholder = self
            .placeholder
            .clone()
            .unwrap_or_else(|| teksilo_i18n::tr_widget!(a11y_builtin_search()));
        let on_submit = self.on_submit.clone();
        let on_select = self.on_select.clone();
        let text_signal = self.text.clone();
        let highlighted_for_submit = highlighted.clone();
        let suggestions_for_submit = suggestions.clone();
        let dismissed_for_submit = dismissed.clone();
        let has_suggestions = self.suggestion_provider.is_some();

        let mut input = TextInput::new(self.text.clone())
            .input_purpose(crate::primitives::InputPurpose::Search)
            .placeholder(placeholder)
            .show_clear_button(true)
            .leading_slot(search_glyph(sf::GLYPH_SIZE, sf::GLYPH_SLOT_WIDTH))
            .enabled(self.enabled.clone())
            .on_submit_fn(move |ctx| {
                let idx = highlighted_for_submit.get();
                let list = suggestions_for_submit.get();
                if let Some(i) = idx {
                    if let Some(value) = list.get(i).cloned() {
                        text_signal.set(value.clone());
                        if let Some(handler) = &on_select {
                            handler(&value, ctx);
                        }
                        dismissed_for_submit.set(true);
                        // Close the popover after picking a
                        // suggestion — without this, the panel
                        // stays open showing just the picked item
                        // until the user clicks outside.
                        ctx.dismiss_all_except_hosts();
                        return;
                    }
                }
                if let Some(handler) = &on_submit {
                    handler(ctx);
                }
                dismissed_for_submit.set(true);
                // Closing overlays here exists solely to take down the
                // *suggestion panel*. With no provider there is no panel, and
                // this call is pure collateral damage: `dismiss_scope` is a
                // single slot, so it silently overwrites whatever the caller's
                // own `on_submit` handler just asked for. A `SearchField` inside
                // a popover — a "go to" palette, a picker — could therefore
                // never close its own popover on Enter, and the reason was
                // invisible from the call site.
                if has_suggestions {
                    ctx.dismiss_all_except_hosts();
                }
            });
        if let Some(label) = &self.label {
            input = input.label(label.clone());
        }
        if let Some(cb) = self.on_text_inserted.clone() {
            input = input.on_text_inserted(move |source, chars| (cb)(source, chars));
        }
        input = input.writing_tools(self.writing_tools.clone());
        // The combobox wiring goes on the field, the node that holds focus:
        // a screen reader reads a combobox's popup state and follows its
        // active descendant on the focused node, never on an ancestor.
        let external_active = self.external_active_descendant.clone();
        let external_controls = self.external_controls.clone();
        if has_suggestions {
            input = input
                .has_popup(teksilo_core::accesskit::HasPopup::Listbox)
                .auto_complete(teksilo_core::accesskit::AutoComplete::List)
                .expanded(overlay_open.clone());
            // The popup's highlighted row while it is open. The host's list,
            // if there is one, while it is closed: the arrow keys drive
            // whichever list is showing.
            let built_in = overlay_open.zip3(&highlighted, &panel_rows);
            let active = match external_active {
                Some(external) => built_in.zip(&external).map(|((open, at, rows), ext)| {
                    if *open {
                        at.and_then(|i| rows.get(i).copied())
                    } else {
                        *ext
                    }
                }),
                None => built_in.map(|(open, at, rows)| {
                    if *open {
                        at.and_then(|i| rows.get(i).copied())
                    } else {
                        None
                    }
                }),
            };
            // The popup's list, which is in the tree only while it is shown:
            // the walker drops a relation to a node it did not emit, so a
            // closed popup leaves `controls` empty.
            let controls = match external_controls {
                Some(external) => overlay_open
                    .zip3(&panel_listbox, &external)
                    .map(|(open, own, ext)| if *open { *own } else { *ext }),
                None => panel_listbox.clone(),
            };
            input = input.active_descendant(active).controls(controls);
        } else {
            if let Some(active) = external_active {
                input = input.active_descendant(active);
            }
            if let Some(listbox) = external_controls {
                input = input.controls(listbox);
            }
        }
        self.field_id = input.field_id();
        let input_id = ctx.add(input);

        // ── Suggestions provider effect ─────────────────────────────
        // Recomputes the list on every text change. The popup's
        // visibility is driven by a separate derived signal further
        // down, so this effect only mutates `suggestions` /
        // `highlighted`.
        if let Some(provider) = self.suggestion_provider.clone() {
            let suggestions = suggestions.clone();
            let highlighted = highlighted.clone();
            let max = self.max_suggestions;
            let min_chars = self.min_chars;
            ctx.effect(&self.text, move |text| {
                let len = text.chars().count();
                if len < min_chars {
                    suggestions.set(Vec::new());
                    highlighted.set(None);
                    return;
                }
                let mut matches = provider(text);
                if matches.len() > max {
                    matches.truncate(max);
                }
                suggestions.set(matches);
                highlighted.set(None);
            });
        }

        // ── Suggestions panel (overlay content, anchored to the field) ─
        // Detached rather than a child (it must not wake or paint with the
        // field), and `add_detached` rather than `ctx.add` so the framework
        // owns it: the previous panel is reaped on rebuild, and the live one
        // when the field itself is destroyed. This used to be a hand-rolled
        // `destroy_subtree` of the old id here, which covered the rebuild but
        // not the destroy — a `SearchField` that went away took nothing with
        // it.
        let panel = SuggestionPanel {
            text: self.text.clone(),
            suggestions: suggestions.clone(),
            highlighted: highlighted.clone(),
            on_select: self.on_select.clone(),
            dismissed: dismissed.clone(),
            listbox: panel_listbox.clone(),
            rows: panel_rows.clone(),
            root_child_id: None,
        };
        let panel_id = ctx.add_detached_deferred(overlay_open.clone(), panel);
        ctx.set_dormant(panel_id);
        self.panel_content_id = Some(panel_id);

        let dismiss_callback: OverlayDismissCallback = {
            let overlay_open = overlay_open.clone();
            let dismissed = dismissed.clone();
            let highlighted = highlighted.clone();
            Rc::new(move |_, _| {
                overlay_open.set(false);
                // The dismiss arrived from the framework (Escape /
                // outside click). Suppress re-opening until the user
                // resumes typing or arrows back into the list — same
                // semantics the explicit Escape handler used to have.
                dismissed.set(true);
                highlighted.set(None);
            })
        };

        let self_id = ctx.self_id();
        let open_overlay: Rc<dyn Fn(&mut EventContext)> = {
            let overlay_open = overlay_open.clone();
            let suggestions_open = suggestions.clone();
            let dismissed_open = dismissed.clone();
            let dismiss_callback = dismiss_callback.clone();
            Rc::new(move |ctx: &mut EventContext| {
                if overlay_open.get() {
                    return;
                }
                if dismissed_open.get() || suggestions_open.get().is_empty() {
                    return;
                }
                overlay_open.set(true);
                // Activate the dormant panel BEFORE queueing the
                // overlay request — `ctx.activate` enqueues a
                // `TreeMutation::Activate` which is applied by the
                // dispatch path *before* `overlay_requests` is
                // drained, so by the time layout walks the overlay
                // stack the panel is active and gets laid out.
                // Without this the panel stays dormant from `build()`
                // (we `set_dormant` it there), `show_overlay` only
                // pushes onto the stack, and `layout_impl`'s overlay
                // loop skips dormant content — popup never paints.
                // ComboBox does the same dance in its `open_overlay` helper.
                // Build the panel if this is its first open — `activate` alone
                // would wake a node whose subtree does not exist yet.
                ctx.materialize_now(panel_id);
                ctx.activate(panel_id);
                ctx.show_overlay(
                    OverlayRequest::new(
                        panel_id,
                        self_id,
                        // `NearAnchor` (rather than `BelowPreferred`)
                        // because the popover should size to the widest
                        // suggestion, not to the field's width. `BelowPreferred`
                        // exists for combo-box dropdowns that must be at
                        // least as wide as their trigger — it does
                        // `content_size.width.max(anchor.width)` in
                        // overlay.rs's `position_overlays`. `NearAnchor`
                        // keeps the same below/above flip behavior, the
                        // same horizontal viewport clamp, but takes the
                        // content's intrinsic width as-is. The
                        // `SuggestionPanel` already reports max
                        // (label_width + row_padding) + panel_padding as
                        // its natural width, so the popover ends up
                        // exactly the size of the widest item.
                        OverlayPlacement::NearAnchor {
                            offset: teksilo_canvas::Vec2::ZERO,
                        },
                        DismissBehavior::EscapeOrClickOutside,
                    )
                    .on_dismiss(dismiss_callback.clone()),
                );
            })
        };

        // ── Compose ─────────────────────────────────────────────────
        // The visible subtree is just the TextInput now; the
        // suggestions panel lives as an overlay anchored to this
        // widget's own bounds via `OverlayRequest`.
        let body_id = ctx.add(MinSize::new(0.0, 0.0).child(input_id));
        let style = crate::styles::recipe_search_field_style::resolve_search_field_style(
            &self.style_override,
            ctx,
        );
        let cfg = teksilo_core::styles::SearchFieldStyleConfig { body: body_id };
        let visible_root = style.make_body(&cfg, ctx);
        self.root_child_id = Some(visible_root);

        // ── Tooltip ────────────────────────────────────────────────
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, visible_root, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, visible_root, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, visible_root, text, delay);
        }

        // ── Handlers ───────────────────────────────────────────────
        //
        // Everything runs on the **preview** pass. The bubble pass is
        // unreachable for character input: `TextInputField.on_key`
        // returns `EventResponse::Handled`, which stops the bubble at
        // the framework level (see event_dispatch_impl.rs's bubble
        // loop). The preview pass walks strict ancestors of the focus
        // target *before* the target itself, so this handler sees
        // every KeyDown the user types into the inner field.
        let suggestions_for_keys = suggestions.clone();
        let highlighted_for_keys = highlighted.clone();
        let dismissed_for_keys = dismissed.clone();
        let overlay_open_for_keys = overlay_open.clone();
        let open_for_arrows = open_overlay.clone();
        let open_for_typing = open_overlay.clone();
        // Captures for the Space-to-select branch (a parallel of the
        // TextInput `on_submit_fn` picker, fired from the preview pass
        // when an item is highlighted so Space doesn't fall through to
        // TextInputField and insert a literal space).
        let suggestions_for_space = suggestions.clone();
        let highlighted_for_space = highlighted.clone();
        let dismissed_for_space = dismissed.clone();
        let overlay_open_for_space = overlay_open.clone();
        let text_for_space = self.text.clone();
        let on_select_for_space = self.on_select.clone();
        // Inline-provider fixup. The text signal isn't updated until
        // the next frame (TextInputField defers via
        // `deferred_text_update`), and even on subsequent keystrokes
        // the preview pass fires *before* the target's key handler,
        // so `self.text.get()` is always the pre-keystroke value here.
        // We project the post-keystroke text by appending the event's
        // `text` field and run the provider synchronously so the
        // popup opens with the right list on the very first character.
        let text_for_typing = self.text.clone();
        let provider_for_typing = self.suggestion_provider.clone();
        let min_chars_for_typing = self.min_chars;
        let max_for_typing = self.max_suggestions;

        let handlers = HandlerSet::new().on_key_preview(move |event, ctx| -> EventResponse {
            match event {
                WidgetEvent::KeyDown {
                    key: Key::ArrowDown,
                    ..
                } => {
                    let list_len = suggestions_for_keys.get().len();
                    if list_len == 0 {
                        return EventResponse::Ignored;
                    }
                    // Re-open if the user previously dismissed.
                    dismissed_for_keys.set(false);
                    let next = match highlighted_for_keys.get() {
                        None => 0,
                        Some(i) if i + 1 >= list_len => 0,
                        Some(i) => i + 1,
                    };
                    highlighted_for_keys.set(Some(next));
                    open_for_arrows(ctx);
                    EventResponse::Handled
                }
                WidgetEvent::KeyDown {
                    key: Key::Space, ..
                } if overlay_open_for_space.get() && highlighted_for_space.get().is_some() => {
                    // Space-to-select: only kicks in when the popover
                    // is open AND a row is currently highlighted (i.e.
                    // the user navigated with arrow keys). Otherwise
                    // we return Ignored so Space falls through to
                    // TextInputField and inserts a literal space in
                    // the query — Space is a valid search character.
                    let idx = highlighted_for_space.get().expect("guard above");
                    let list = suggestions_for_space.get();
                    if let Some(value) = list.get(idx).cloned() {
                        text_for_space.set(value.clone());
                        if let Some(handler) = &on_select_for_space {
                            handler(&value, ctx);
                        }
                        dismissed_for_space.set(true);
                        ctx.dismiss_all_except_hosts();
                    }
                    EventResponse::Handled
                }
                WidgetEvent::KeyDown {
                    key: Key::ArrowUp, ..
                } => {
                    let list_len = suggestions_for_keys.get().len();
                    if list_len == 0 {
                        return EventResponse::Ignored;
                    }
                    dismissed_for_keys.set(false);
                    let prev = match highlighted_for_keys.get() {
                        None => list_len - 1,
                        Some(0) => list_len - 1,
                        Some(i) => i - 1,
                    };
                    highlighted_for_keys.set(Some(prev));
                    open_for_arrows(ctx);
                    EventResponse::Handled
                }
                WidgetEvent::KeyDown { text, .. } => {
                    // Any other key — character input, Backspace,
                    // Delete, etc — clears the dismissed flag so the
                    // popup can re-appear after the user resumes typing.
                    dismissed_for_keys.set(false);
                    // For character input, project the post-keystroke
                    // text and run the provider synchronously, then
                    // open. The framework continues the preview pass
                    // (we return Ignored), so TextInputField still
                    // gets the keystroke and inserts the character.
                    //
                    // Filter control characters out of the projected
                    // text. Without this, Enter (`"\n"` / `"\r"`)
                    // appends a newline to the prefix, the provider
                    // returns no matches, and the empty-dismiss path
                    // below fires `on_dismiss` synchronously between
                    // this handler and TextInputField's submit
                    // handler — which resets `highlighted` to None,
                    // so submit can't pick the highlighted row. Same
                    // for Tab, Escape, Backspace's "\u{8}", etc.
                    let mut became_empty = false;
                    if let (Some(ch), Some(provider)) = (text, &provider_for_typing) {
                        let clean: String = ch.chars().filter(|c| !c.is_control()).collect();
                        if !clean.is_empty() {
                            let projected = format!("{}{}", text_for_typing.get(), clean);
                            if projected.chars().count() >= min_chars_for_typing {
                                let mut fresh = provider(&projected);
                                if fresh.len() > max_for_typing {
                                    fresh.truncate(max_for_typing);
                                }
                                became_empty = fresh.is_empty();
                                suggestions_for_keys.set(fresh);
                            } else {
                                became_empty = true;
                                suggestions_for_keys.set(Vec::new());
                            }
                        }
                    }
                    if became_empty && overlay_open_for_keys.get() {
                        // No matches for the new text — close the
                        // popover instead of leaving it stuck on the
                        // pre-keystroke list. The framework's dismiss
                        // path fires the on_dismiss callback we
                        // registered, which resets `overlay_open` and
                        // `dismissed` correctly. The next character
                        // that yields a non-empty list will reopen.
                        ctx.dismiss_all_except_hosts();
                    } else {
                        open_for_typing(ctx);
                    }
                    EventResponse::Ignored
                }
                _ => EventResponse::Ignored,
            }
        });

        ctx.apply_self_handlers(handlers);

        // Return BOTH the visible root AND the dormant suggestions
        // panel as children so the framework links `panel_id` under
        // this widget in the arena instead of leaving it an orphan
        // root. See popover_widget.rs for the same pattern.
        let mut out = vec![visible_root];
        if let Some(panel_id) = self.panel_content_id {
            out.push(panel_id);
        }
        out
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
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
        // The visible root fills our bounds; the suggestions panel's
        // bounds are owned by the overlay manager when shown
        // (`position_overlays`), so we zero-size it here.
        for child in children.iter_mut() {
            if Some(child.id) == self.panel_content_id {
                child.size = teksilo_canvas::Size::ZERO;
                continue;
            }
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    /// Structure only: the editing field is the `SearchInput` and carries
    /// the name and the combobox properties (see `build`). A second node
    /// saying them around it is what made a screen reader stop on an unnamed
    /// object before the field. `GenericContainer` is what the
    /// presentational pass collapses.
    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    /// The editing field stands for the search field, so what an application
    /// attaches to the `SearchField`'s id (`access_label`, a `FormLayout`'s
    /// `labelled_by`, `access_described_by`) lands on the node that holds
    /// focus.
    fn accessibility_proxy(&self) -> Option<WidgetId> {
        self.field_id.get()
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut out = Vec::new();
        if let Some(id) = self.root_child_id {
            out.push(id);
        }
        if let Some(id) = self.panel_content_id {
            out.push(id);
        }
        out
    }
}

// ── SuggestionPanel — the in-tree listbox rendered below the field ─

struct SuggestionPanel {
    text: Signal<String>,
    suggestions: Signal<Vec<String>>,
    highlighted: Signal<Option<usize>>,
    on_select: Option<OnSelect>,
    dismissed: Signal<bool>,
    /// Where this panel publishes its `ListBox`, for the field's `controls`.
    listbox: Signal<Option<WidgetId>>,
    /// Where this panel publishes its rows in display order, which the field
    /// maps the highlighted index through for its `active_descendant`.
    rows: Signal<Vec<WidgetId>>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for SuggestionPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SuggestionPanel").finish_non_exhaustive()
    }
}

impl Widget for SuggestionPanel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use teksilo_core::binding::BindingLevel;
        // Bind the panel to `suggestions` at `Rebuild` so its row
        // list refreshes whenever the Vec changes — same pattern
        // `Repeater` uses for ListModel-backed dynamic content.
        self.suggestions
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        use crate::styles::recipe_search_field_style as sf;
        let suggestions = self.suggestions.clone();
        let highlighted = self.highlighted.clone();
        let on_select = self.on_select.clone();
        let text = self.text.clone();
        let dismissed = self.dismissed.clone();

        // Row gutters come from the active `SearchFieldStyle`, not from
        // `sf::ROW_PADDING_*`: a preset writes its own on the recipe (macOS's
        // `NSSearchField` results row is 8 x 3 dp) and reading the module
        // constants here rendered 10 x 4 whatever the theme had decided.
        let panel_style =
            crate::styles::recipe_search_field_style::resolve_search_field_style(&None, ctx);
        let panel_tokens = ctx.theme().input;
        let row_padding_horizontal = panel_style.row_padding_horizontal(&panel_tokens);
        let row_padding_vertical = panel_style.row_padding_vertical(&panel_tokens);

        let list = suggestions.get();
        let total = list.len();
        let mut row_ids: Vec<WidgetId> = Vec::with_capacity(total);
        let mut column = VStack::new().alignment(HAlignment::Leading);
        for (idx, value) in list.into_iter().enumerate() {
            let bg_role = highlighted.map(move |h| match h {
                Some(i) if *i == idx => SurfaceRole::Hover,
                _ => SurfaceRole::Transparent,
            });
            let bg = ctx.add(
                RectWidget::new()
                    .background(bg_role)
                    .corner_radius(CornerRadius::uniform(sf::ROW_CORNER_RADIUS)),
            );
            let label_id = ctx.add(
                TextWidget::new(lit!(&value))
                    .style(TextStyleRole::Body)
                    .single_line()
                    // `SuggestionRow::accessibility` already carries `value`
                    // as the `Role::ListBoxOption` node's own name.
                    .a11y_hidden(),
            );
            let inner_padded = ctx.add(
                Padding::symmetric(row_padding_vertical, row_padding_horizontal).child(label_id),
            );
            let row_z = ctx.add(ZStack::new().child(bg).child(inner_padded));

            let value_for_tap = value.clone();
            let on_select_for_tap = on_select.clone();
            let text_for_tap = text.clone();
            let highlighted_for_hover = highlighted.clone();
            let dismissed_for_tap = dismissed.clone();
            let row = ctx.add(
                SuggestionRow {
                    label: value.clone(),
                    index: idx,
                    row_height: sf::ROW_HEIGHT,
                    selected_signal: highlighted.clone(),
                    inner_id: row_z,
                }
                .on_tap(move |_pos, ctx| {
                    text_for_tap.set(value_for_tap.clone());
                    if let Some(handler) = &on_select_for_tap {
                        handler(&value_for_tap, ctx);
                    }
                    dismissed_for_tap.set(true);
                    // Close the popover after picking a suggestion.
                    // The on_dismiss callback resets `overlay_open`
                    // and re-sets `dismissed` (idempotent — we just
                    // set it ourselves).
                    ctx.dismiss_all_except_hosts();
                })
                .on_hover(move |entered, _| {
                    if entered {
                        highlighted_for_hover.set(Some(idx));
                    }
                })
                // An assistive `Focus` on the row (advertised by
                // `SuggestionRow`) highlights it, as the arrows would; the
                // field keeps keyboard focus and names it as its active
                // descendant.
                .on_access_action({
                    let highlighted = highlighted.clone();
                    move |action, ctx| {
                        if action != teksilo_core::accesskit::Action::Focus {
                            return teksilo_core::event::EventResponse::Ignored;
                        }
                        highlighted.set(Some(idx));
                        ctx.request_accessibility_update();
                        teksilo_core::event::EventResponse::Handled
                    }
                })
                .cursor(CursorIcon::Pointer),
            );
            row_ids.push(row);
            column = column.child(row);
        }
        // Publish the row ids for the field to resolve `highlighted ->
        // active_descendant`. Republished on every rebuild so it tracks the
        // live row WidgetIds.
        self.rows.set_if_changed(row_ids);

        // Listbox surface — routed through `PopoverStyle` (the
        // `Menu`-flavoured variant), so the panel background, border,
        // corner radius, and the field-attached drop shadow are owned
        // by the active popover style. The suggestion popup normally
        // opens below the field (`NearAnchor` in `SearchField`, which
        // flips above only when there is no room below), so the
        // placement handed to the style suppresses the top-side shadow.
        let listbox_inner = ctx.add(column);
        let padded = ctx.add(Padding::uniform(sf::PANEL_PADDING).child(listbox_inner));
        let popover_style: teksilo_core::styles::SharedPopoverStyle =
            ctx.theme().style_slots.popover.clone().unwrap_or_else(|| {
                Rc::new(crate::styles::RecipePopoverStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });
        let surface = popover_style.make_body(
            &PopoverStyleConfig {
                content: padded,
                variant: PopoverVariant::Menu,
                name: String::new(),
                placement: OverlayPlacement::BelowPreferred,
                show_caret: false,
                caret_size: 0.0,
            },
            ctx,
        );

        let listbox = ctx.add(SuggestionListBox {
            inner: surface,
            total,
        });
        // Publish the listbox WidgetId for the field's `controls`.
        self.listbox.set_if_changed(Some(listbox));

        self.root_child_id = Some(listbox);
        vec![listbox]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
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
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ── ListBox a11y wrapper around the styled column ─────────────────

struct SuggestionListBox {
    inner: WidgetId,
    /// How many suggestions the query returned — the one number a screen-reader
    /// user needs before deciding whether to arrow through them.
    ///
    /// On the container, not on each row: AccessKit's `size_of_set` belongs on
    /// the container (unlike ARIA's per-item `aria-setsize`), and
    /// `size_of_set_from_container` walks *up* from an item to find it, so a
    /// count written on a `Role::ListBoxOption` is read by no adapter.
    total: usize,
}

impl std::fmt::Debug for SuggestionListBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SuggestionListBox").finish_non_exhaustive()
    }
}

impl Widget for SuggestionListBox {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        ctx.child_size(self.inner, proposal)
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
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::ListBox);
        if self.total > 0 {
            builder.set_size_of_set(self.total);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.inner]
    }
}

// ── Per-row a11y wrapper: Role::ListBoxOption with ARIA position
//    metadata. Bridges tap / hover handlers onto the styled inner
//    ZStack via WidgetBuilder. ─────────────────────────────────────

struct SuggestionRow {
    label: String,
    index: usize,
    /// Minimum row height — the `recipe_search_field_style::ROW_HEIGHT`
    /// module constant, read at build time.
    row_height: f32,
    selected_signal: Signal<Option<usize>>,
    inner_id: WidgetId,
}

impl std::fmt::Debug for SuggestionRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SuggestionRow")
            .field("label", &self.label)
            .field("index", &self.index)
            .finish()
    }
}

impl Widget for SuggestionRow {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let inner = ctx
            .child_size(self.inner_id, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, self.row_height));
        let height = inner.height.max(self.row_height);
        let width = proposal.width.unwrap_or(inner.width).max(inner.width);
        teksilo_canvas::Size::new(width, height).into()
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
        builder.set_name(&self.label);
        let is_selected = self.selected_signal.get() == Some(self.index);
        builder.set_selected(is_selected);
        builder.set_position_in_set(self.index + 1);
        // The highlighted row is the field's active descendant, so the
        // platform's focus, and `accesskit_consumer` reads a node as
        // focusable (UIA `IsKeyboardFocusable`) from this action alone. The
        // row answers it by taking the highlight.
        builder.add_action(teksilo_core::accesskit::Action::Focus);
        // The "of N" half lives on the `SuggestionListBox` container; a count
        // written here would be read by no adapter.
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.inner_id]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::widget_tree::WidgetTree;

    #[test]
    fn search_field_builds() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let q = Signal::new(String::new());
        let id = tree.add(SearchField::new(q.clone()).placeholder(lit!("Search docs")));
        tree.layout(SizeProposal {
            width: Some(320.0),
            height: None,
        });
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn search_field_a11y_role() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            SearchField::new(Signal::new(String::new())).with_suggestions(|q| {
                if q.is_empty() {
                    Vec::new()
                } else {
                    vec!["alpha".into(), "beta".into()]
                }
            }),
        );
        tree.layout(SizeProposal {
            width: Some(280.0),
            height: None,
        });
        // The role is on the editing field, the node focus lands on; the
        // `SearchField`'s own node is structure.
        tree.focus(id);
        let field = tree
            .focused()
            .expect("focusing the SearchField focuses its field");
        assert_ne!(field, id);
        let info = tree.accessibility_node(field);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::SearchInput);
    }

    #[test]
    fn tooltip_appears_on_hover() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let q = Signal::new(String::new());
        let id = tree.add(SearchField::new(q.clone()).tooltip(lit!("Tip")));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(tree.find_by_label("Tip").is_some());
    }

    #[test]
    fn suggestions_provider_runs_on_text_change() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let q = Signal::new(String::new());
        let calls: Rc<Cell<usize>> = Rc::new(Cell::new(0));
        let calls_clone = calls.clone();
        tree.add(SearchField::new(q.clone()).with_suggestions(move |s| {
            calls_clone.set(calls_clone.get() + 1);
            if s.is_empty() {
                Vec::new()
            } else {
                vec!["alpha".into()]
            }
        }));
        tree.layout(SizeProposal::exact(300.0, 100.0));
        let baseline = calls.get();
        q.set("a".to_string());
        tree.layout(SizeProposal::exact(300.0, 100.0));
        assert!(
            calls.get() > baseline,
            "provider should fire on text change"
        );
    }

    #[test]
    fn suggestion_row_label_does_not_duplicate_the_listbox_options_name() {
        // `SuggestionRow::accessibility` already carries `value` as the
        // `Role::ListBoxOption` node's own name, so the embedded Label
        // built for the row's text must not survive as a second,
        // duplicate-named stop.
        let q = Signal::new(String::new());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(SearchField::new(q.clone()).with_suggestions(|s| {
            if s.is_empty() {
                Vec::new()
            } else {
                vec!["alpha".into()]
            }
        }));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        let field = tree
            .first_focusable_descendant(id)
            .expect("SearchField should have a focusable inner input");
        tree.focus(field);
        tree.type_text(id, "a");
        tree.layout(SizeProposal::exact(300.0, 200.0));

        let update = tree.sync_accessibility();

        let row_node = update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == teksilo_core::accesskit::Role::ListBoxOption)
            .map(|(_, node)| node)
            .expect("suggestion row should be in the AT tree once the popover is open");
        assert_eq!(
            row_node.label(),
            Some("alpha"),
            "hiding the embedded label must not take the row's own name away with it",
        );

        assert!(
            !update.nodes.iter().any(|(_, node)| node.role()
                == teksilo_core::accesskit::Role::Label
                && node.label() == Some("alpha")),
            "the row's Text widget must not survive as a duplicate-named AT node",
        );
    }

    // ── One node to assistive technology ───────────────────────────────

    use teksilo_core::accessibility::widget_id_to_node_id;
    use teksilo_core::accesskit::{AutoComplete, HasPopup, Node, NodeId, Role};

    fn fruit(prefix: &str) -> Vec<String> {
        ["apple", "apricot", "avocado"]
            .iter()
            .filter(|f| !prefix.is_empty() && f.starts_with(prefix))
            .map(|s| s.to_string())
            .collect()
    }

    fn size() -> SizeProposal {
        SizeProposal::exact(320.0, 240.0)
    }

    /// A root holding `field`, laid out.
    fn mounted(field: impl Widget + 'static) -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(field);
        tree.layout(size());
        (tree, id)
    }

    /// The update's nodes, and the node it names as focus.
    fn snapshot(tree: &mut WidgetTree) -> (std::collections::HashMap<NodeId, Node>, NodeId) {
        tree.layout(size());
        let update = tree.sync_accessibility();
        (update.nodes.into_iter().collect(), update.focus)
    }

    fn focused_node(tree: &mut WidgetTree) -> (std::collections::HashMap<NodeId, Node>, Node) {
        let (nodes, focus) = snapshot(tree);
        let node = nodes
            .get(&focus)
            .cloned()
            .expect("the focused node is in the update");
        (nodes, node)
    }

    fn press(tree: &mut WidgetTree, key: Key) {
        tree.press_key(key, teksilo_core::event::Modifiers::empty());
        tree.layout(size());
    }

    /// What Skribisto's Settings panel does: a host names the `SearchField`
    /// itself as the place focus goes. Focus lands on the field, which takes
    /// the keys.
    #[test]
    fn focusing_the_search_field_focuses_its_field_and_typing_reaches_it() {
        let query = Signal::new(String::new());
        let (mut tree, id) = mounted(SearchField::new(query.clone()).label(lit!("Find")));
        tree.focus(id);
        let field = tree.focused().expect("something is focused");
        assert_ne!(
            field, id,
            "focus must not park on the structural outer node"
        );
        assert_eq!(tree.accessibility_node(field).role(), Role::SearchInput);
        tree.type_text(field, "q");
        tree.layout(size());
        assert_eq!(query.get(), "q", "the keys reach the field");
    }

    /// One Tab stop, whatever focused it: the outer node is not one.
    #[test]
    fn the_search_field_is_a_single_tab_stop() {
        let (mut tree, id) =
            mounted(SearchField::new(Signal::new(String::new())).with_suggestions(fruit));
        tree.focus(id);
        let field = tree.focused().unwrap();
        let stops = tree.tab_stops_within(id);
        assert_eq!(stops.iter().filter(|s| **s == field).count(), 1);
        assert!(!stops.contains(&id));
    }

    /// With suggestions, the combobox is the focused field: one node claims
    /// the popup, it is the `SearchInput`, it carries the label, and the
    /// `SearchField`'s own node is not in the tree.
    #[test]
    fn with_suggestions_the_focused_field_is_the_named_combobox() {
        let (mut tree, id) = mounted(
            SearchField::new(Signal::new(String::new()))
                .label(lit!("Find"))
                .with_suggestions(fruit),
        );
        tree.focus(id);
        let (nodes, focused) = focused_node(&mut tree);
        let claimants: Vec<_> = nodes.values().filter(|n| n.has_popup().is_some()).collect();
        assert_eq!(claimants.len(), 1, "exactly one node claims the popup");
        assert_eq!(focused.has_popup(), Some(HasPopup::Listbox));
        assert_eq!(focused.auto_complete(), Some(AutoComplete::List));
        assert_eq!(focused.role(), Role::SearchInput);
        assert_eq!(focused.label(), Some("Find"));
        assert_eq!(focused.is_expanded(), Some(false), "closed, and said so");
        assert!(
            !nodes.contains_key(&widget_id_to_node_id(id)),
            "the SearchField's own node is structure and is pruned"
        );
    }

    /// Typing opens the popup and the field says so; Escape closes it and the
    /// field says `false`, not nothing.
    #[test]
    fn the_field_reports_the_popup_opening_and_closing() {
        let (mut tree, id) =
            mounted(SearchField::new(Signal::new(String::new())).with_suggestions(fruit));
        tree.focus(id);
        let field = tree.focused().unwrap();
        tree.type_text(field, "a");
        tree.layout(size());
        let (_, focused) = focused_node(&mut tree);
        assert_eq!(focused.is_expanded(), Some(true));
        press(&mut tree, Key::Escape);
        let (_, focused) = focused_node(&mut tree);
        assert_eq!(focused.is_expanded(), Some(false));
    }

    /// A suggestion row is the field's active descendant while highlighted,
    /// so it must read as focusable to a platform adapter (UIA
    /// `IsKeyboardFocusable`), and an assistive `Focus` on it highlights it,
    /// as the arrows would, the keyboard staying in the field.
    #[test]
    fn an_assistive_focus_on_a_suggestion_highlights_it() {
        use crate::common::item_focus::test_support::{assistive_focus, reads_as_focusable};
        let (mut tree, id) =
            mounted(SearchField::new(Signal::new(String::new())).with_suggestions(fruit));
        tree.focus(id);
        let field = tree.focused().unwrap();
        tree.type_text(field, "a");
        tree.layout(size());
        press(&mut tree, Key::ArrowDown);
        let (nodes, focused) = focused_node(&mut tree);
        let first = focused.active_descendant().expect("the highlight");
        let second = nodes
            .iter()
            .find(|(_, node)| node.label() == Some("apricot"))
            .map(|(id, _)| *id)
            .expect("the second suggestion");
        assert_ne!(first, second);
        assert!(reads_as_focusable(&mut tree, second));

        assert!(assistive_focus(&mut tree, second), "the row took it");
        tree.layout(size());
        let (_, focused) = focused_node(&mut tree);
        assert_eq!(focused.active_descendant(), Some(second));
        assert_eq!(
            tree.focused(),
            Some(field),
            "the keyboard stays in the field"
        );
    }

    /// Arrowing through the suggestions moves the focused field's active
    /// descendant along the rows, each one a `ListBoxOption` in the update,
    /// and closing the popup clears it.
    #[test]
    fn arrowing_moves_the_fields_active_descendant_along_the_rows() {
        let (mut tree, id) =
            mounted(SearchField::new(Signal::new(String::new())).with_suggestions(fruit));
        tree.focus(id);
        let field = tree.focused().unwrap();
        tree.type_text(field, "a");
        tree.layout(size());
        press(&mut tree, Key::ArrowDown);
        let (nodes, focused) = focused_node(&mut tree);
        let first = focused
            .active_descendant()
            .expect("the highlight is announced");
        assert_eq!(nodes.get(&first).map(Node::role), Some(Role::ListBoxOption));
        assert_eq!(nodes[&first].label(), Some("apple"));
        let listbox = focused
            .controls()
            .first()
            .copied()
            .expect("controls the list");
        assert_eq!(nodes.get(&listbox).map(Node::role), Some(Role::ListBox));
        press(&mut tree, Key::ArrowDown);
        let (nodes, focused) = focused_node(&mut tree);
        let second = focused.active_descendant().expect("still announced");
        assert_ne!(first, second, "the active descendant follows the highlight");
        assert_eq!(nodes[&second].label(), Some("apricot"));
        press(&mut tree, Key::Escape);
        let (_, focused) = focused_node(&mut tree);
        assert_eq!(focused.active_descendant(), None, "cleared on close");
        assert!(
            focused.controls().is_empty(),
            "a closed popup's list is not in the tree, so no relation names it"
        );
    }

    /// Without suggestions there is no popup to claim.
    #[test]
    fn without_suggestions_the_field_claims_no_popup() {
        let (mut tree, id) = mounted(SearchField::new(Signal::new(String::new())));
        tree.focus(id);
        let (nodes, focused) = focused_node(&mut tree);
        assert_eq!(focused.role(), Role::SearchInput);
        assert_eq!(focused.has_popup(), None);
        assert_eq!(focused.auto_complete(), None);
        assert_eq!(focused.is_expanded(), None);
        assert!(nodes.values().all(|n| n.has_popup().is_none()));
    }

    /// A list the host owns (a command palette): the field follows the host's
    /// signals and claims no popup, since that list is not one.
    #[test]
    fn drives_listbox_follows_the_hosts_list_without_a_popup() {
        let list = Signal::new(None::<WidgetId>);
        let active = Signal::new(None::<WidgetId>);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let row = tree.add(TextWidget::new(lit!("row")));
        let host_list = tree.add(VStack::new().child(row));
        let id = tree.add(
            SearchField::new(Signal::new(String::new()))
                .drives_listbox(list.clone(), active.clone()),
        );
        tree.add(VStack::new().child(id).child(host_list));
        tree.layout(size());
        tree.focus(id);
        list.set(Some(host_list));
        active.set(Some(row));
        let (_, focused) = focused_node(&mut tree);
        assert_eq!(focused.controls(), &[widget_id_to_node_id(host_list)]);
        assert_eq!(focused.active_descendant(), Some(widget_id_to_node_id(row)));
        assert_eq!(focused.has_popup(), None);
        assert_eq!(focused.is_expanded(), None);
        active.set(None);
        let (_, focused) = focused_node(&mut tree);
        assert_eq!(focused.active_descendant(), None);
    }

    /// Both lists: while the popup is open its row is the one announced, and
    /// once it closes the host's is again.
    #[test]
    fn the_open_popups_row_wins_over_the_hosts_list() {
        let list = Signal::new(None::<WidgetId>);
        let active = Signal::new(None::<WidgetId>);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let row = tree.add(TextWidget::new(lit!("row")));
        let host_list = tree.add(VStack::new().child(row));
        let id = tree.add(
            SearchField::new(Signal::new(String::new()))
                .with_suggestions(fruit)
                .drives_listbox(list.clone(), active.clone()),
        );
        tree.add(VStack::new().child(id).child(host_list));
        tree.layout(size());
        list.set(Some(host_list));
        active.set(Some(row));
        tree.focus(id);
        let field = tree.focused().unwrap();
        let (_, focused) = focused_node(&mut tree);
        assert_eq!(focused.active_descendant(), Some(widget_id_to_node_id(row)));
        assert_eq!(focused.controls(), &[widget_id_to_node_id(host_list)]);
        tree.type_text(field, "a");
        tree.layout(size());
        press(&mut tree, Key::ArrowDown);
        let (nodes, focused) = focused_node(&mut tree);
        let own = focused.active_descendant().expect("the popup's row");
        assert_eq!(nodes.get(&own).map(Node::role), Some(Role::ListBoxOption));
        assert_ne!(focused.controls(), &[widget_id_to_node_id(host_list)]);
        press(&mut tree, Key::Escape);
        let (_, focused) = focused_node(&mut tree);
        assert_eq!(focused.active_descendant(), Some(widget_id_to_node_id(row)));
        assert_eq!(focused.controls(), &[widget_id_to_node_id(host_list)]);
    }

    /// What an application attaches to the `SearchField`'s id reaches the
    /// field, the node a screen reader lands on.
    #[test]
    fn an_access_label_on_the_search_field_names_its_field() {
        use teksilo_core::widget_builder::WidgetBuilder;
        let (mut tree, id) = mounted(
            SearchField::new(Signal::new(String::new()))
                .with_suggestions(fruit)
                .access_label(lit!("Find a setting")),
        );
        tree.focus(id);
        let (_, focused) = focused_node(&mut tree);
        assert_eq!(focused.label(), Some("Find a setting"));
    }

    /// A `FormLayout` row label names the field through `labelled_by` on the
    /// `SearchField`'s id.
    #[test]
    fn a_form_row_label_names_the_search_field() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let label = tree.add(TextWidget::new(lit!("Query")));
        let search = tree.add(SearchField::new(Signal::new(String::new())).with_suggestions(fruit));
        tree.add(crate::primitives::FormLayout::new().line(label, search));
        tree.layout(size());
        tree.focus(search);
        let (_, focused) = focused_node(&mut tree);
        assert!(
            focused.labelled_by().contains(&widget_id_to_node_id(label)),
            "the row label must reach the focused field"
        );
    }

    /// A description attached to the `SearchField`'s id reaches the field.
    #[test]
    fn an_access_described_by_on_the_search_field_describes_its_field() {
        use teksilo_core::widget_builder::WidgetBuilder;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let hint = tree.add(TextWidget::new(lit!("Searches every setting")));
        let search = tree.add(
            SearchField::new(Signal::new(String::new()))
                .with_suggestions(fruit)
                .access_described_by(hint),
        );
        tree.add(VStack::new().child(search).child(hint));
        tree.layout(size());
        tree.focus(search);
        let (_, focused) = focused_node(&mut tree);
        assert!(focused.described_by().contains(&widget_id_to_node_id(hint)));
    }
}
