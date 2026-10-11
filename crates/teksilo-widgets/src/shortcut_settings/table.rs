// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The list of shortcuts: one row per shortcut, grouped by category, in
//! columns that line up from the first row to the last.
//!
//! The list rebuilds whenever anything it shows changes: the registry, the
//! search, a recording starting or ending, a conflict waiting for an answer.
//! A rebuild destroys the control that held focus, and the tree drops focus
//! silently when that happens, so the list remembers the control by its
//! [`FocusKey`] and gives focus back to the control with the same key.
//!
//! Its controls are one Tab stop: Tab enters the list at the control last
//! used, the arrow keys move between rows and columns, Home and End jump to
//! the first and last row.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::styles::ButtonVariant;
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{lit, tr_widget};
use teksilo_tokens::{TextRole, TextStyleRole};

use super::controls::{
    DOT, GroupHeading, LeadingFill, ModifiedDot, RowButton, key_caps, quiet_text,
};
use super::state::{
    CaptureTarget, FocusKey, Shared, SlotKind, cancel_pending, confirm_pending, rebind_deferred,
    start_recording,
};
use crate::icon_button::BuiltInIcons;
use crate::keystroke_format::{format_keystroke, key_label};
use crate::primitives::{HStack, TextWidget, VStack};
use crate::severity_badge::{SeverityBadge, SeverityIconKind};

/// Space between columns.
const GAP: f32 = 8.0;
/// Space between rows.
const ROW_GAP: f32 = 2.0;
/// Extra space above every group heading but the first.
const GROUP_GAP: f32 = 12.0;
/// Space under a group heading.
const HEADING_GAP: f32 = 4.0;
/// Space between a row and its detail line.
const DETAIL_GAP: f32 = 2.0;
/// Space under a banner.
const BANNER_GAP: f32 = 8.0;
/// The narrowest the command column gets before the row overflows.
const MIN_LABEL: f32 = 96.0;
/// The widest the command column gets. Past it, a wide panel leaves the
/// space after the last column rather than between a command and its
/// chord, where the eye would have to cross it on every row.
const MAX_LABEL: f32 = 360.0;
/// Size of the icons inside the row controls.
const ICON: f32 = 14.0;

/// One shortcut, as the list shows it.
#[derive(Debug, Clone)]
struct RowData {
    id: &'static str,
    name: String,
    description: Option<String>,
    primary: Option<KeyStroke>,
    secondary: Option<KeyStroke>,
    /// The grouping key. Groups are formed by it and ordered by their text.
    category: Option<&'static str>,
    category_text: String,
    modified: bool,
}

/// The widgets of one row.
#[derive(Debug, Clone, Copy)]
struct RowIds {
    dot: WidgetId,
    label: WidgetId,
    primary: WidgetId,
    secondary: WidgetId,
    reset: WidgetId,
    detail: Option<WidgetId>,
}

/// One item of the list, top to bottom.
#[derive(Debug, Clone, Copy)]
enum Entry {
    /// A full-width line above the list: the find hint, the reset-all
    /// confirmation.
    Banner(WidgetId),
    Heading {
        id: WidgetId,
        first: bool,
    },
    Row(RowIds),
    /// What the list says when it shows nothing.
    Empty(WidgetId),
}

/// Where each control sits, for the keyboard handler.
#[derive(Debug, Default)]
struct NavMap {
    by_id: HashMap<WidgetId, FocusKey>,
    by_key: HashMap<FocusKey, WidgetId>,
    /// The navigable controls of each row, in visual order.
    rows: Vec<Vec<FocusKey>>,
}

/// The list. See the module docs.
pub(super) struct ShortcutTable {
    shared: Rc<Shared>,
    entries: Vec<Entry>,
    /// A recording field that is never shown. The chord columns are at
    /// least as wide as it, so starting a recording never widens a column
    /// and pushes the commands aside.
    template: Option<WidgetId>,
    nav: Rc<RefCell<NavMap>>,
}

impl std::fmt::Debug for ShortcutTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShortcutTable")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl ShortcutTable {
    pub(super) fn new(shared: Rc<Shared>) -> Self {
        Self {
            shared,
            entries: Vec::new(),
            template: None,
            nav: Rc::new(RefCell::new(NavMap::default())),
        }
    }

    /// The navigable keys of every row, for tests.
    #[cfg(test)]
    pub(super) fn nav_rows(&self) -> Vec<Vec<FocusKey>> {
        self.nav.borrow().rows.clone()
    }

    /// The state the list shares with the widget, for tests.
    #[cfg(test)]
    pub(super) fn shared(&self) -> &Rc<Shared> {
        &self.shared
    }

    /// The widget behind a key, for tests.
    #[cfg(test)]
    pub(super) fn control(&self, key: FocusKey) -> Option<WidgetId> {
        self.nav.borrow().by_key.get(&key).copied()
    }
}

/// Read the registry into rows, filtered and in display order.
fn collect_rows(ctx: &BuildContext, shared: &Shared) -> (Vec<RowData>, Vec<&'static str>) {
    let reg = ctx.shortcut_registry();
    let uncategorized = tr_widget!(shortcut_settings_uncategorized()).resolve_now();
    let mut overridden = Vec::new();
    let all: Vec<RowData> = reg
        .iter_effective()
        .map(|eff| {
            let modified = reg.override_for(eff.shortcut.id).is_some();
            if modified {
                overridden.push(eff.shortcut.id);
            }
            RowData {
                id: eff.shortcut.id,
                name: eff.shortcut.name.get(),
                description: eff
                    .shortcut
                    .description
                    .as_ref()
                    .map(|d| d.get())
                    .filter(|d| !d.trim().is_empty()),
                primary: eff.primary,
                secondary: eff.secondary,
                category: eff.shortcut.category,
                category_text: eff
                    .shortcut
                    .category_text()
                    .unwrap_or_else(|| uncategorized.clone()),
                modified,
            }
        })
        .collect();

    let text = shared.filter.get();
    let needle = text.trim().to_lowercase();
    // "Find by keys" filters by the exact chord, for as long as the search
    // text still reads as that chord. Editing the text drops back to a text
    // search.
    let chord = shared
        .chord_filter
        .get()
        .filter(|ks| format_keystroke(*ks) == text.trim());
    let modified_only = shared.modified_only.get();

    let mut rows: Vec<RowData> = all
        .into_iter()
        .filter(|row| !modified_only || row.modified)
        .filter(|row| match chord {
            Some(ks) => row.primary == Some(ks) || row.secondary == Some(ks),
            None => needle.is_empty() || row_matches(row, &needle),
        })
        .collect();
    // Groups in the order their names read, the uncategorised group first;
    // rows in the order their names read. Both in the interface language,
    // so a translated list is still in order.
    rows.sort_by_cached_key(|row| {
        (
            row.category.is_some(),
            row.category_text.to_lowercase(),
            row.category,
            row.name.to_lowercase(),
            row.id,
        )
    });
    (rows, overridden)
}

/// Whether a text search finds `row`: by its name, its group, its
/// description, or the label of either chord.
fn row_matches(row: &RowData, needle: &str) -> bool {
    let has = |s: &str| s.to_lowercase().contains(needle);
    has(&row.name)
        || has(&row.category_text)
        || row.description.as_deref().is_some_and(has)
        || row.primary.is_some_and(|ks| has(&format_keystroke(ks)))
        || row.secondary.is_some_and(|ks| has(&format_keystroke(ks)))
}

/// The label of the key that cancels a recording, and of the one that
/// removes a chord.
fn cancel_and_clear_labels() -> (String, String) {
    let clear = if cfg!(target_os = "macos") {
        Key::Backspace
    } else {
        Key::Delete
    };
    (key_label(Key::Escape), key_label(clear))
}

/// A line of text inside a control.
fn control_text(text: String, role: TextRole) -> TextWidget {
    TextWidget::new(lit!(text))
        .style(TextStyleRole::Body)
        .color(role)
        .single_line()
        .no_shrink()
        .a11y_hidden()
}

impl ShortcutTable {
    /// Build the chord control of one slot, in whichever state it is in.
    fn slot_control(&self, row: &RowData, slot: SlotKind) -> RowButton {
        let shared = &self.shared;
        let target = CaptureTarget::new(row.id, slot);
        let chord = match slot {
            SlotKind::Primary => row.primary,
            SlotKind::Secondary => row.secondary,
        };
        let command = row.name.clone();
        let activate = {
            let shared = shared.clone();
            move |ctx: &mut EventContext| start_recording(&shared, ctx, target)
        };

        if shared.capturing.get() == Some(target) {
            let description = match shared.rejection.get() {
                Some(r) if r.target == target => r.message,
                _ => {
                    let (escape, delete) = cancel_and_clear_labels();
                    tr_widget!(shortcut_settings_recording_hint(
                        escape = escape,
                        delete = delete
                    ))
                    .resolve_now()
                }
            };
            return RowButton::new(
                FocusKey::Slot(target),
                shared.clone(),
                LeadingFill::new(control_text(
                    tr_widget!(shortcut_settings_recording()).resolve_now(),
                    TextRole::Accent,
                )),
                tr_widget!(a11y_shortcut_settings_recording(command = command)).resolve_now(),
                activate,
            )
            .variant(ButtonVariant::Plain)
            .ring(true)
            .description(Some(description));
        }

        let description = (slot == SlotKind::Primary)
            .then(|| row_description(row))
            .flatten();
        match (chord, slot) {
            (Some(ks), _) => {
                let chord_text = format_keystroke(ks);
                let name = match slot {
                    SlotKind::Primary => tr_widget!(a11y_shortcut_settings_primary(
                        command = command,
                        chord = chord_text
                    )),
                    SlotKind::Secondary => tr_widget!(a11y_shortcut_settings_alternative(
                        command = command,
                        chord = chord_text
                    )),
                };
                RowButton::new(
                    FocusKey::Slot(target),
                    shared.clone(),
                    LeadingFill::new(key_caps(ks)),
                    name.resolve_now(),
                    activate,
                )
                .description(description)
            }
            (None, SlotKind::Primary) => RowButton::new(
                FocusKey::Slot(target),
                shared.clone(),
                LeadingFill::new(control_text(
                    tr_widget!(shortcut_settings_not_set()).resolve_now(),
                    TextRole::Secondary,
                )),
                tr_widget!(a11y_shortcut_settings_primary_unset(command = command)).resolve_now(),
                activate,
            )
            .description(description),
            (None, SlotKind::Secondary) => RowButton::new(
                FocusKey::Slot(target),
                shared.clone(),
                LeadingFill::new(
                    (BuiltInIcons::global().add)()
                        .icon_size(ICON)
                        .color(TextRole::Secondary),
                ),
                tr_widget!(a11y_shortcut_settings_add_alternative(command = command)).resolve_now(),
                activate,
            )
            .tooltip(tr_widget!(shortcut_settings_add_alternative())),
        }
    }

    /// Build a row's reset button.
    fn reset_control(&self, row: &RowData) -> RowButton {
        let id = row.id;
        let shared = self.shared.clone();
        RowButton::new(
            FocusKey::Reset(id),
            self.shared.clone(),
            (BuiltInIcons::global().reset)()
                .icon_size(ICON)
                .color(TextRole::Secondary),
            tr_widget!(a11y_shortcut_settings_reset(command = row.name.clone())).resolve_now(),
            move |ctx: &mut EventContext| {
                shared.cancel_capture();
                if shared.pending.get().is_some() {
                    shared.pending.set(None);
                }
                // The button goes away with the override, so focus moves to
                // the chord it just restored.
                let landing = FocusKey::Slot(CaptureTarget::new(id, SlotKind::Primary));
                shared.focus.set(Some(landing));
                shared.roving.set(Some(landing));
                ctx.clear_shortcut_override(id);
            },
        )
        .tooltip(tr_widget!(shortcut_settings_reset()))
    }

    /// The line under a row, when the row has something to say: a
    /// recording's hint, a refused chord, a conflict to confirm.
    fn detail_line(&self, ctx: &mut BuildContext, row: &RowData) -> Option<WidgetId> {
        let shared = &self.shared;
        if let Some(p) = shared.pending.get().filter(|p| p.target.id == row.id) {
            let message = tr_widget!(shortcut_settings_conflict(
                chord = format_keystroke(p.ks),
                command = p.conflict_name.clone()
            ))
            .resolve_now();
            let reassign = {
                let shared = shared.clone();
                RowButton::new(
                    FocusKey::Reassign(row.id),
                    shared.clone(),
                    control_text(
                        tr_widget!(shortcut_settings_reassign()).resolve_now(),
                        TextRole::OnAccent,
                    ),
                    tr_widget!(shortcut_settings_reassign()).resolve_now(),
                    move |ctx: &mut EventContext| confirm_pending(&shared, ctx),
                )
                .variant(ButtonVariant::Filled)
                .description(Some(message.clone()))
            };
            let keep = {
                let shared = shared.clone();
                RowButton::new(
                    FocusKey::KeepBinding(row.id),
                    shared.clone(),
                    control_text(
                        tr_widget!(shortcut_settings_cancel()).resolve_now(),
                        TextRole::Primary,
                    ),
                    tr_widget!(shortcut_settings_cancel()).resolve_now(),
                    move |_ctx: &mut EventContext| cancel_pending(&shared),
                )
                .variant(ButtonVariant::Plain)
                .description(Some(message.clone()))
            };
            let reassign = ctx.add(reassign);
            let keep = ctx.add(keep);
            self.register(FocusKey::Reassign(row.id), reassign);
            self.register(FocusKey::KeepBinding(row.id), keep);
            return Some(
                ctx.add(
                    HStack::new()
                        .spacing(6.0)
                        .child(SeverityBadge::new(SeverityIconKind::Warning, ICON))
                        .child(quiet_text(message, TextRole::Primary))
                        .child(reassign)
                        .child(keep),
                ),
            );
        }
        let recording = shared.capturing.get().filter(|t| t.id == row.id)?;
        let line = match shared.rejection.get().filter(|r| r.target == recording) {
            Some(r) => HStack::new()
                .spacing(6.0)
                .child(SeverityBadge::new(SeverityIconKind::Error, ICON))
                .child(quiet_text(r.message, TextRole::Error)),
            None => {
                let (escape, delete) = cancel_and_clear_labels();
                HStack::new().child(quiet_text(
                    tr_widget!(shortcut_settings_recording_hint(
                        escape = escape,
                        delete = delete
                    ))
                    .resolve_now(),
                    TextRole::Secondary,
                ))
            }
        };
        Some(ctx.add(line))
    }

    /// The banners above the list.
    fn banners(&self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let shared = &self.shared;
        let mut out = Vec::new();
        if shared.finding.get() {
            let (escape, _) = cancel_and_clear_labels();
            out.push(
                ctx.add(
                    HStack::new()
                        .spacing(6.0)
                        .child(
                            (BuiltInIcons::global().keyboard)()
                                .icon_size(ICON)
                                .color(TextRole::Accent),
                        )
                        .child(quiet_text(
                            tr_widget!(shortcut_settings_find_hint(escape = escape)).resolve_now(),
                            TextRole::Secondary,
                        )),
                ),
            );
        }
        if shared.confirm_reset_all.get() {
            let prompt = tr_widget!(shortcut_settings_reset_all_prompt()).resolve_now();
            let confirm = {
                let shared = shared.clone();
                RowButton::new(
                    FocusKey::ResetAllConfirm,
                    shared.clone(),
                    control_text(
                        tr_widget!(shortcut_settings_reset_all_confirm()).resolve_now(),
                        TextRole::OnAccent,
                    ),
                    tr_widget!(shortcut_settings_reset_all_confirm()).resolve_now(),
                    move |ctx: &mut EventContext| {
                        for id in shared.overridden_ids.borrow().iter() {
                            ctx.clear_shortcut_override(*id);
                        }
                        shared.focus.set(None);
                        shared.confirm_reset_all.set(false);
                        if let Some(search) = shared.search_field.get() {
                            ctx.request_focus(search);
                        }
                        ctx.announce(
                            tr_widget!(a11y_shortcut_settings_reset_all_done()).resolve_now(),
                        );
                    },
                )
                .variant(ButtonVariant::Destructive)
                .description(Some(prompt.clone()))
            };
            let cancel = {
                let shared = shared.clone();
                RowButton::new(
                    FocusKey::ResetAllCancel,
                    shared.clone(),
                    control_text(
                        tr_widget!(shortcut_settings_cancel()).resolve_now(),
                        TextRole::Primary,
                    ),
                    tr_widget!(shortcut_settings_cancel()).resolve_now(),
                    move |ctx: &mut EventContext| close_reset_all(&shared, ctx),
                )
                .variant(ButtonVariant::Plain)
                .description(Some(prompt.clone()))
            };
            let confirm = ctx.add(confirm);
            let cancel = ctx.add(cancel);
            self.register(FocusKey::ResetAllConfirm, confirm);
            self.register(FocusKey::ResetAllCancel, cancel);
            out.push(
                ctx.add(
                    HStack::new()
                        .spacing(6.0)
                        .child(SeverityBadge::new(SeverityIconKind::Warning, ICON))
                        .child(quiet_text(prompt, TextRole::Primary))
                        .child(confirm)
                        .child(cancel),
                ),
            );
        }
        out
    }

    /// What the list says when it has no row to show.
    fn empty_message(&self, any_registered: bool) -> String {
        let shared = &self.shared;
        let text = shared.filter.get();
        let query = text.trim().to_string();
        if let Some(ks) = shared
            .chord_filter
            .get()
            .filter(|ks| format_keystroke(*ks) == query)
        {
            return tr_widget!(shortcut_settings_no_chord_match(
                chord = format_keystroke(ks)
            ))
            .resolve_now();
        }
        if !query.is_empty() {
            return tr_widget!(shortcut_settings_no_match(query = query)).resolve_now();
        }
        if any_registered && shared.modified_only.get() {
            return tr_widget!(shortcut_settings_none_modified()).resolve_now();
        }
        tr_widget!(shortcut_settings_empty()).resolve_now()
    }

    fn register(&self, key: FocusKey, id: WidgetId) {
        let mut nav = self.nav.borrow_mut();
        nav.by_id.insert(id, key);
        nav.by_key.insert(key, id);
    }
}

/// The description a primary control carries: the shortcut's own
/// description, then whether the user changed it.
fn row_description(row: &RowData) -> Option<String> {
    let modified = row
        .modified
        .then(|| tr_widget!(a11y_shortcut_settings_modified()).resolve_now());
    match (row.description.clone(), modified) {
        (Some(d), Some(m)) => Some(format!("{d} {m}")),
        (d, m) => d.or(m),
    }
}

/// Close the reset-all confirmation and give focus back to "Reset all".
fn close_reset_all(shared: &Shared, ctx: &mut EventContext) {
    shared.focus.set(None);
    shared.confirm_reset_all.set(false);
    if let Some(button) = shared.reset_all_button.get() {
        ctx.request_focus(button);
    }
}

impl Widget for ShortcutTable {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let shared = self.shared.clone();
        ctx.shortcut_version()
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        shared
            .capturing
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        shared
            .finding
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        shared
            .pending
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        shared
            .rejection
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        shared
            .confirm_reset_all
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        shared
            .filter
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        shared
            .chord_filter
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        shared
            .modified_only
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);

        *self.nav.borrow_mut() = NavMap::default();
        self.entries.clear();

        let any_registered = ctx.shortcut_registry().iter_effective().next().is_some();
        let (rows, overridden) = collect_rows(ctx, &shared);
        let has_overrides = !overridden.is_empty();
        *shared.overridden_ids.borrow_mut() = overridden;
        if shared.has_overrides.get() != has_overrides {
            shared.has_overrides.set(has_overrides);
        }

        if shared.announce_find_result.replace(false)
            && let Some(ks) = shared.chord_filter.get()
        {
            let count = rows.len() as i64;
            ctx.announce(
                tr_widget!(a11y_shortcut_settings_find_result(
                    count = count,
                    chord = format_keystroke(ks)
                ))
                .resolve_now(),
            );
        }

        for banner in self.banners(ctx) {
            self.entries.push(Entry::Banner(banner));
        }

        let mut last_category: Option<Option<&'static str>> = None;
        for row in &rows {
            if last_category != Some(row.category) {
                let first = last_category.is_none();
                let id = ctx.add(GroupHeading::new(row.category_text.clone()));
                self.entries.push(Entry::Heading { id, first });
                last_category = Some(row.category);
            }

            let dot = ctx.add(ModifiedDot::new(row.modified));
            let mut label = VStack::new().child(
                TextWidget::new(lit!(row.name.clone()))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary)
                    .single_line()
                    .a11y_hidden(),
            );
            if let Some(description) = &row.description {
                label = label.child(
                    TextWidget::new(lit!(description.clone()))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary)
                        .max_lines(2)
                        .a11y_hidden(),
                );
            }
            let label = ctx.add(label);

            let primary_key = FocusKey::Slot(CaptureTarget::new(row.id, SlotKind::Primary));
            let secondary_key = FocusKey::Slot(CaptureTarget::new(row.id, SlotKind::Secondary));
            let reset_key = FocusKey::Reset(row.id);
            let primary = ctx.add(self.slot_control(row, SlotKind::Primary));
            let secondary = ctx.add(self.slot_control(row, SlotKind::Secondary));
            let reset = ctx.add(self.reset_control(row));
            if !row.modified {
                // Kept, dormant, so its column keeps its width.
                ctx.visible_when(reset, false);
            }
            self.register(primary_key, primary);
            self.register(secondary_key, secondary);
            let mut nav_row = vec![primary_key, secondary_key];
            if row.modified {
                self.register(reset_key, reset);
                nav_row.push(reset_key);
            }
            self.nav.borrow_mut().rows.push(nav_row);

            let detail = self.detail_line(ctx, row);
            self.entries.push(Entry::Row(RowIds {
                dot,
                label,
                primary,
                secondary,
                reset,
                detail,
            }));
        }

        if rows.is_empty() {
            let id = ctx.add(
                TextWidget::new(lit!(self.empty_message(any_registered)))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Secondary),
            );
            self.entries.push(Entry::Empty(id));
        }

        let template = ctx.add(
            RowButton::new(
                FocusKey::Slot(CaptureTarget::new("", SlotKind::Primary)),
                shared.clone(),
                LeadingFill::new(control_text(
                    tr_widget!(shortcut_settings_recording()).resolve_now(),
                    TextRole::Accent,
                )),
                String::new(),
                |_ctx: &mut EventContext| {},
            )
            .variant(ButtonVariant::Plain),
        );
        ctx.visible_when(template, false);
        self.template = Some(template);

        // The list's one Tab stop: the control last used while it still
        // exists, the first control otherwise.
        let first = self
            .nav
            .borrow()
            .rows
            .first()
            .and_then(|r| r.first().copied());
        let roving = shared.roving.get();
        let roving_ok = roving.is_some_and(|k| {
            let nav = self.nav.borrow();
            nav.rows.iter().any(|r| r.contains(&k))
        });
        if !roving_ok && roving != first {
            shared.roving.set(first);
        }
        let ids: Vec<(FocusKey, WidgetId)> = self
            .nav
            .borrow()
            .rows
            .iter()
            .flatten()
            .filter_map(|k| self.nav.borrow().by_key.get(k).map(|id| (*k, *id)))
            .collect();
        for (key, id) in ids {
            ctx.set_tab_stop(id, shared.roving.map(move |r| *r == Some(key)));
        }

        // Give focus back to the control the rebuild destroyed.
        if let Some(key) = shared.focus.get() {
            let nav = self.nav.borrow();
            let fallback = match key {
                FocusKey::Reset(id) => {
                    Some(FocusKey::Slot(CaptureTarget::new(id, SlotKind::Primary)))
                }
                _ => shared.roving.get(),
            };
            let target = nav
                .by_key
                .get(&key)
                .or_else(|| fallback.and_then(|k| nav.by_key.get(&k)))
                .copied();
            drop(nav);
            if let Some(id) = target {
                ctx.focus(id);
            }
        }

        ctx.apply_self_handlers(self.key_handlers());

        let mut children: Vec<WidgetId> = Vec::new();
        for entry in &self.entries {
            match *entry {
                Entry::Banner(id) | Entry::Heading { id, .. } | Entry::Empty(id) => {
                    children.push(id)
                }
                Entry::Row(r) => {
                    children.extend([r.dot, r.label, r.primary, r.secondary, r.reset]);
                    children.extend(r.detail);
                }
            }
        }
        children.push(template);
        children
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.compute(proposal.width, ctx).0.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let (_, rects) = self.compute(Some(bounds.width), ctx);
        for child in children.iter_mut() {
            if let Some(rect) = rects.get(&child.id) {
                child.origin = Point::new(bounds.x + rect.x, bounds.y + rect.y);
                child.size = Size::new(rect.width, rect.height);
            }
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut out = Vec::new();
        for entry in &self.entries {
            match *entry {
                Entry::Banner(id) | Entry::Heading { id, .. } | Entry::Empty(id) => out.push(id),
                Entry::Row(r) => {
                    out.extend([r.dot, r.label, r.primary, r.secondary, r.reset]);
                    out.extend(r.detail);
                }
            }
        }
        out.extend(self.template);
        out
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// A widget's size for `proposal`, whether it is showing or dormant.
fn measure(ctx: &LayoutContext, id: WidgetId, proposal: SizeProposal) -> Size {
    ctx.child_size(id, proposal)
        .or_else(|| ctx.measure_intrinsic(id, proposal))
        .unwrap_or(Size::ZERO)
}

impl ShortcutTable {
    /// Lay the list out at `width` (its natural width when `None`): the
    /// total size and every child's rectangle, relative to the list.
    fn compute(&self, width: Option<f32>, ctx: &LayoutContext) -> (Size, HashMap<WidgetId, Rect>) {
        let open = SizeProposal::unspecified();
        let floor = self
            .template
            .map(|t| measure(ctx, t, open).width)
            .unwrap_or(0.0);
        let (mut primary_w, mut secondary_w, mut reset_w, mut label_w_natural) =
            (floor, floor, 0.0_f32, 0.0_f32);
        let mut other_natural = 0.0_f32;
        for entry in &self.entries {
            match *entry {
                Entry::Row(r) => {
                    primary_w = primary_w.max(measure(ctx, r.primary, open).width);
                    secondary_w = secondary_w.max(measure(ctx, r.secondary, open).width);
                    reset_w = reset_w.max(measure(ctx, r.reset, open).width);
                    label_w_natural = label_w_natural.max(measure(ctx, r.label, open).width);
                }
                Entry::Banner(id) | Entry::Heading { id, .. } | Entry::Empty(id) => {
                    other_natural = other_natural.max(measure(ctx, id, open).width);
                }
            }
        }
        let fixed = DOT + GAP + GAP + primary_w + GAP + secondary_w + GAP + reset_w;
        let total_w = width.unwrap_or_else(|| (fixed + label_w_natural).max(other_natural));
        let label_w = (total_w - fixed)
            .min(label_w_natural.clamp(MIN_LABEL, MAX_LABEL))
            .max(MIN_LABEL);
        let label_x = DOT + GAP;
        let primary_x = label_x + label_w + GAP;
        let secondary_x = primary_x + primary_w + GAP;
        let reset_x = secondary_x + secondary_w + GAP;
        let rtl = ctx.is_rtl();
        let full = SizeProposal::with_width(total_w);

        let mut rects: HashMap<WidgetId, Rect> = HashMap::new();
        let mut put = |id: WidgetId, x: f32, y: f32, size: Size| {
            let x = if rtl { total_w - x - size.width } else { x };
            rects.insert(id, Rect::new(x, y, size.width, size.height));
        };
        let mut y = 0.0_f32;
        let mut last_gap = 0.0_f32;
        for entry in &self.entries {
            match *entry {
                Entry::Banner(id) => {
                    let size = measure(ctx, id, full);
                    put(id, 0.0, y, Size::new(total_w.min(size.width), size.height));
                    y += size.height + BANNER_GAP;
                    last_gap = BANNER_GAP;
                }
                Entry::Heading { id, first } => {
                    if !first {
                        y += GROUP_GAP;
                    }
                    let size = measure(ctx, id, full);
                    put(id, 0.0, y, Size::new(total_w, size.height));
                    y += size.height + HEADING_GAP;
                    last_gap = HEADING_GAP;
                }
                Entry::Empty(id) => {
                    let size = measure(ctx, id, full);
                    put(id, 0.0, y, Size::new(total_w, size.height));
                    y += size.height;
                    last_gap = 0.0;
                }
                Entry::Row(r) => {
                    let label = measure(ctx, r.label, SizeProposal::with_width(label_w));
                    let primary = measure(ctx, r.primary, SizeProposal::with_width(primary_w));
                    let secondary =
                        measure(ctx, r.secondary, SizeProposal::with_width(secondary_w));
                    let reset = measure(ctx, r.reset, SizeProposal::with_width(reset_w));
                    let band = primary.height.max(secondary.height).max(reset.height);
                    let row_h = band.max(label.height);
                    let center = |h: f32| y + ((band - h) / 2.0).max(0.0);
                    put(r.dot, 0.0, center(DOT), Size::new(DOT, DOT));
                    put(
                        r.label,
                        label_x,
                        center(label.height),
                        Size::new(label_w, label.height),
                    );
                    put(
                        r.primary,
                        primary_x,
                        center(primary.height),
                        Size::new(primary_w, primary.height),
                    );
                    put(
                        r.secondary,
                        secondary_x,
                        center(secondary.height),
                        Size::new(secondary_w, secondary.height),
                    );
                    put(
                        r.reset,
                        reset_x,
                        center(reset.height),
                        Size::new(reset.width.min(reset_w), reset.height),
                    );
                    let mut h = row_h;
                    if let Some(detail) = r.detail {
                        let detail_w = (total_w - label_x).max(0.0);
                        let size = measure(ctx, detail, SizeProposal::with_width(detail_w));
                        put(
                            detail,
                            label_x,
                            y + h + DETAIL_GAP,
                            Size::new(detail_w.min(size.width), size.height),
                        );
                        h += DETAIL_GAP + size.height;
                    }
                    y += h + ROW_GAP;
                    last_gap = ROW_GAP;
                }
            }
        }
        let height = (y - last_gap).max(0.0);
        (Size::new(total_w, height), rects)
    }

    /// Arrow keys, Home and End move between the list's controls; Delete
    /// removes the focused chord; Escape answers a pending question with
    /// "Cancel".
    fn key_handlers(&self) -> HandlerSet {
        let nav = self.nav.clone();
        let shared = self.shared.clone();
        HandlerSet::new().on_key_preview(
            move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };
                let Some(focused) = ctx.focused() else {
                    return EventResponse::Ignored;
                };
                let Some(current) = nav.borrow().by_id.get(&focused).copied() else {
                    return EventResponse::Ignored;
                };
                let bare = *modifiers == Modifiers::NONE;
                match current {
                    FocusKey::Reassign(_) | FocusKey::KeepBinding(_)
                        if bare && *key == Key::Escape =>
                    {
                        cancel_pending(&shared);
                        return EventResponse::Handled;
                    }
                    FocusKey::ResetAllConfirm | FocusKey::ResetAllCancel
                        if bare && *key == Key::Escape =>
                    {
                        close_reset_all(&shared, ctx);
                        return EventResponse::Handled;
                    }
                    FocusKey::Slot(target)
                        if bare && matches!(key, Key::Delete | Key::Backspace) =>
                    {
                        rebind_deferred(ctx, target.id, target.slot, None);
                        return EventResponse::Handled;
                    }
                    _ => {}
                }
                if !bare {
                    return EventResponse::Ignored;
                }
                let nav_ref = nav.borrow();
                let Some((row, col)) =
                    nav_ref.rows.iter().enumerate().find_map(|(r, keys)| {
                        keys.iter().position(|k| *k == current).map(|c| (r, c))
                    })
                else {
                    return EventResponse::Ignored;
                };
                let (back, forward) = if ctx.is_rtl() {
                    (Key::ArrowRight, Key::ArrowLeft)
                } else {
                    (Key::ArrowLeft, Key::ArrowRight)
                };
                let last_row = nav_ref.rows.len() - 1;
                let (row, col) = match *key {
                    Key::ArrowUp => (row.saturating_sub(1), col),
                    Key::ArrowDown => ((row + 1).min(last_row), col),
                    Key::Home => (0, col),
                    Key::End => (last_row, col),
                    k if k == back => (row, col.saturating_sub(1)),
                    k if k == forward => (row, col + 1),
                    _ => return EventResponse::Ignored,
                };
                let keys = &nav_ref.rows[row];
                let target = keys[col.min(keys.len() - 1)];
                let Some(id) = nav_ref.by_key.get(&target).copied() else {
                    return EventResponse::Ignored;
                };
                drop(nav_ref);
                shared.roving.set(Some(target));
                ctx.request_focus(id);
                EventResponse::Handled
            },
        )
    }
}
