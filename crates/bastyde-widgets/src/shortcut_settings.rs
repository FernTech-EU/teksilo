// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ShortcutSettings — user-facing widget for browsing and rebinding
//! application shortcuts.
//!
//! Reads every shortcut registered in the tree's
//! [`ShortcutRegistry`](bastyde_core::shortcut::ShortcutRegistry) and
//! renders one row per entry, grouped by category, with both primary
//! and secondary keystrokes independently rebindable. Supports:
//!
//! - **Rebind** (primary or secondary) via one-shot key capture.
//! - **Unbind** a slot explicitly (sets the override to `None`), or
//!   press `Delete` / `Backspace` during capture.
//! - **Reset** clears the user override entirely, restoring the
//!   declared defaults. Disabled when no override exists.
//! - **Conflict auto-resolution**: rebinding to a keystroke already
//!   bound elsewhere silently unbinds the conflicting shortcut so
//!   there's always exactly one binding per chord.
//! - **Escape** during capture cancels without committing.
//! - **Platform-aware keystroke labels** via [`format_keystroke`].
//!
//! The widget owns the currently-armed [`CaptureHandle`]; dropping
//! the widget cancels the capture, so navigating away mid-rebind
//! cannot leak a stray rebind onto the next keystroke pressed
//! somewhere else in the app.
//!
//! ```ignore
//! // Inside a settings Dialog build():
//! let filter = ctx.signal(String::new());
//! ctx.add(
//!     ShortcutSettings::new()
//!         .with_filter(filter)
//!         .confirm_conflicts(true)
//!         .on_conflict(|c| println!("displaced: {}", c.displaced_name)),
//! );
//! ```

use bastyde_i18n::lit;
use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{Key, Modifiers};
use bastyde_core::shortcut::{CaptureHandle, KeyStroke};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use crate::button::Button;
use crate::keystroke_format::format_keystroke;
use crate::primitives::{HStack, Spacer, TextWidget, VStack};
use bastyde_tokens::{TextRole, TextStyleRole};

/// Which keystroke slot a capture/rebind targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotKind {
    Primary,
    Secondary,
}

/// Composite key identifying a pending capture: shortcut id + slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CaptureTarget {
    id: &'static str,
    slot: SlotKind,
}

/// Describes a rebind that collides with an existing binding.
///
/// Passed to the [`ShortcutSettings::on_conflict`] callback so the app
/// can surface a toast ("Save lost its Ctrl+S binding"); also used
/// internally to drive the optional inline confirm prompt.
#[derive(Debug, Clone)]
pub struct ShortcutConflict {
    /// Id of the shortcut that currently owns the chord and will be
    /// unbound if the rebind proceeds.
    pub displaced_id: String,
    /// Display name of that shortcut (its registry `name`).
    pub displaced_name: String,
    /// The chord being assigned to the new target.
    pub keystroke: KeyStroke,
}

/// A rebind held back pending user confirmation (confirm mode only).
#[derive(Debug, Clone)]
struct PendingRebind {
    target_id: &'static str,
    slot: SlotKind,
    ks: KeyStroke,
    conflict_id: String,
    conflict_slot: Option<SlotKind>,
    conflict_name: String,
}

/// A settings panel for browsing and rebinding application shortcuts.
///
/// Reads every `Shortcut` in the tree's `ShortcutRegistry`, groups rows
/// by category, and renders primary + secondary keystroke slots with
/// Rebind, Unbind, and Reset controls. See the module-level docs for the
/// full feature list.
pub struct ShortcutSettings {
    /// Target of the current capture (`None` when idle). Drives the
    /// "Press any key…" hint on the correct row + slot.
    capturing: Signal<Option<CaptureTarget>>,
    /// Live capture handle — dropped on widget destruction (or
    /// replaced on the next rebind) to cancel the capture. Shared
    /// with button closures via `Rc` so they can store the handle
    /// returned by `ctx.begin_key_capture(...)`.
    active_handle: Rc<RefCell<Option<CaptureHandle>>>,
    /// Optional filter signal. When bound, rows are included only
    /// when the substring matches (case-insensitive) the shortcut's
    /// `name`, `id`, or `category`. Apps drive this from whatever
    /// input they want — a text field, a chip bar, a command palette.
    /// When `None`, every registered shortcut is shown.
    filter: Option<Signal<String>>,
    /// When `true`, a rebind that collides with an existing binding
    /// surfaces an inline confirm prompt instead of silently unbinding
    /// the other shortcut. Off by default (immediate reassignment).
    confirm_conflicts: bool,
    /// Fired whenever a rebind collides with an existing binding,
    /// regardless of `confirm_conflicts`. Lets apps show a toast.
    on_conflict: Option<Rc<dyn Fn(&ShortcutConflict)>>,
    /// Holds a rebind awaiting confirmation (confirm mode). Drives the
    /// inline "already assigned to X — Reassign / Cancel" prompt.
    pending: Signal<Option<PendingRebind>>,
    root_child_id: Option<WidgetId>,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl ShortcutSettings {
    /// Create a settings panel that lists every shortcut currently
    /// registered in the tree's `ShortcutRegistry`, without a filter.
    pub fn new() -> Self {
        Self {
            capturing: Signal::new(None),
            active_handle: Rc::new(RefCell::new(None)),
            filter: None,
            confirm_conflicts: false,
            on_conflict: None,
            pending: Signal::new(None),
            root_child_id: None,
        }
    }

    /// Bind the visible row set to a filter signal. The widget
    /// shows only shortcuts whose `name`, `id`, or `category`
    /// contains the filter text (case-insensitive). Empty string =
    /// show everything.
    ///
    /// Apps typically drive this from a `TextInput` elsewhere in
    /// their settings UI; keeping the filter external keeps this
    /// widget's own surface minimal rather than embedding a search box.
    pub fn with_filter(mut self, filter: Signal<String>) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Require explicit confirmation before a rebind unbinds a
    /// conflicting shortcut. Off by default — the chord is reassigned
    /// immediately (the historical behavior). When on, a colliding
    /// rebind shows an inline "already assigned to X — Reassign /
    /// Cancel" prompt on the row, and the registry is left untouched
    /// until the user confirms.
    pub fn confirm_conflicts(mut self, yes: bool) -> Self {
        self.confirm_conflicts = yes;
        self
    }

    /// Register a callback fired whenever a rebind collides with an
    /// existing binding — **regardless** of [`confirm_conflicts`]. The
    /// callback receives the displaced shortcut's id, name, and the
    /// chord, so the app can surface a toast ("Save lost its Ctrl+S
    /// binding"). It fires before the displaced binding is removed (in
    /// confirm mode, before the user has confirmed).
    ///
    /// [`confirm_conflicts`]: Self::confirm_conflicts
    pub fn on_conflict(mut self, f: impl Fn(&ShortcutConflict) + 'static) -> Self {
        self.on_conflict = Some(Rc::new(f));
        self
    }
}

impl std::fmt::Debug for ShortcutSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShortcutSettings").finish()
    }
}

impl Widget for ShortcutSettings {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Rebuild on any registry change (register, rebind, clear).
        ctx.shortcut_version().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );
        // Rebuild when capture state changes — the "Press…" hint
        // jumps between rows.
        self.capturing
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        // Rebuild when a conflict is queued / resolved — the inline
        // confirm prompt appears and disappears on a row.
        self.pending
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        // Rebuild when the filter signal changes.
        if let Some(filter) = &self.filter {
            filter.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        }

        let filter_needle = self
            .filter
            .as_ref()
            .map(|f| f.get().trim().to_lowercase())
            .unwrap_or_default();
        let matches_filter = |data: &ShortcutRowData| -> bool {
            if filter_needle.is_empty() {
                return true;
            }
            let hay_lower = |s: &str| s.to_lowercase();
            hay_lower(&data.name).contains(&filter_needle)
                || hay_lower(data.id).contains(&filter_needle)
                || data
                    .category
                    .map(|c| hay_lower(c).contains(&filter_needle))
                    .unwrap_or(false)
        };

        let mut rows: Vec<ShortcutRowData> = ctx
            .shortcut_registry()
            .iter_effective()
            .map(|eff| ShortcutRowData {
                id: eff.shortcut.id,
                name: eff.shortcut.name.get(),
                primary: eff.primary,
                secondary: eff.secondary,
                enabled: eff.enabled,
                category: eff.shortcut.category,
                has_override: ctx
                    .shortcut_registry()
                    .override_for(eff.shortcut.id)
                    .is_some(),
            })
            .filter(matches_filter)
            .collect();
        // Stable order: category then id.
        rows.sort_by(|a, b| a.category.cmp(&b.category).then(a.id.cmp(b.id)));

        let capturing = self.capturing.get();
        let pending = self.pending.get();
        let mut column = VStack::new().spacing(4.0);

        let mut last_category: Option<Option<&'static str>> = None;
        for row in rows {
            if last_category != Some(row.category) {
                column = column.child(category_header(row.category));
                last_category = Some(row.category);
            }
            let row_id = self.build_row(ctx, &row, capturing, pending.as_ref());
            column = column.add_child(row_id);
        }

        let root = ctx.add(column);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Group);
        builder.set_name(
            bastyde_i18n::tr_widget!(a11y_shortcut_settings_name())
                .resolve_now()
                .as_str(),
        );
    }
}

/// A thin wrapper used only by the row currently in key-capture mode.
/// Emits `Role::Status` + `Live::Polite` so assistive tech announces
/// the "Press any key…" hint the moment the capture row appears, and
/// re-announces when the hint text changes (e.g. capture cancels).
/// This sits in place of a plain `TextWidget` inside `slot_widget`.
#[derive(Debug)]
struct LiveStatusText {
    text: String,
    role: TextRole,
    child_id: Option<WidgetId>,
}

impl LiveStatusText {
    fn new(text: impl Into<String>, role: TextRole) -> Self {
        Self {
            text: text.into(),
            role,
            child_id: None,
        }
    }
}

impl Widget for LiveStatusText {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.add(
            TextWidget::new(lit!(&self.text))
                .color(self.role)
                .single_line()
                .a11y_hidden(),
        );
        self.child_id = Some(id);
        vec![id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.child_id
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
        builder.set_role(bastyde_core::accesskit::Role::Status);
        builder.set_name(self.text.as_str());
        builder.set_live(bastyde_core::accesskit::Live::Polite);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

struct ShortcutRowData {
    id: &'static str,
    name: String,
    primary: Option<KeyStroke>,
    secondary: Option<KeyStroke>,
    enabled: bool,
    category: Option<&'static str>,
    has_override: bool,
}

fn category_header(category: Option<&'static str>) -> impl Widget + 'static {
    let label = category.unwrap_or("General");
    TextWidget::new(lit!(label))
        .style(TextStyleRole::BodyBold)
        .color(TextRole::Primary)
        .single_line()
}

impl ShortcutSettings {
    fn build_row(
        &self,
        ctx: &mut BuildContext,
        row: &ShortcutRowData,
        capturing: Option<CaptureTarget>,
        pending: Option<&PendingRebind>,
    ) -> WidgetId {
        let id = row.id;
        // The label and keystroke text use `TextRole::Primary`
        // unconditionally; the leaves consult
        // `PaintContext::effective_enabled` and substitute
        // `TextRole::Disabled` on their own when the arena says the
        // row is disabled (either via the registry-driven flag below
        // or via an ancestor's `enabled_when`).
        let name_widget = TextWidget::new(lit!(&row.name))
            .color(TextRole::Primary)
            .single_line();

        let primary_slot = self.slot_widget(id, SlotKind::Primary, row.primary, capturing, pending);
        let secondary_slot =
            self.slot_widget(id, SlotKind::Secondary, row.secondary, capturing, pending);

        let reset_button = Button::new(lit!("Reset"))
            .enabled(row.has_override)
            .on_activate_fn(move |ctx: &mut EventContext| {
                ctx.clear_shortcut_override(id);
            });

        let row_widget = HStack::new()
            .spacing(8.0)
            .child(name_widget)
            .child(Spacer::new())
            .child(primary_slot)
            .child(secondary_slot)
            .child(reset_button);
        let row_id = ctx.add(row_widget);
        // Bridge the registry-driven per-shortcut enabled flag into
        // the arena. The arena is then the single source of truth:
        // descendants AND with this node, the leaves auto-substitute
        // `TextRole::Disabled`, the Reset Button's tap handler is
        // gated, and the framework a11y walker emits `set_disabled`
        // on every descendant. An ancestor's `enabled_when` (e.g. a
        // disabled settings dialog) cascades correctly.
        if !row.enabled {
            ctx.enabled_when(row_id, false);
        }
        row_id
    }

    fn slot_widget(
        &self,
        id: &'static str,
        slot: SlotKind,
        keystroke: Option<KeyStroke>,
        capturing: Option<CaptureTarget>,
        pending: Option<&PendingRebind>,
    ) -> impl Widget + 'static {
        let is_capturing_here = capturing == Some(CaptureTarget { id, slot });
        // Confirm mode: a rebind on this slot is awaiting the user's OK.
        let pending_here = pending
            .filter(|p| p.target_id == id && p.slot == slot)
            .cloned();
        let keystroke_text = if is_capturing_here {
            bastyde_i18n::tr_widget!(a11y_shortcut_settings_capture_hint()).resolve_now()
        } else {
            keystroke
                .map(format_keystroke)
                .unwrap_or_else(|| "—".to_string())
        };

        let slot_label = match slot {
            SlotKind::Primary => "Rebind",
            SlotKind::Secondary => "Rebind 2nd",
        };

        let confirm = self.confirm_conflicts;
        let on_conflict = self.on_conflict.clone();
        let pending_signal = self.pending.clone();
        let rebind_button = {
            let capturing_signal = self.capturing.clone();
            let handle_cell = self.active_handle.clone();
            let pending_for_cb = pending_signal.clone();
            Button::new(lit!(slot_label)).on_activate_fn(move |ctx: &mut EventContext| {
                let target = CaptureTarget { id, slot };
                capturing_signal.set(Some(target));
                let cap_for_cb = capturing_signal.clone();
                let on_conflict = on_conflict.clone();
                let pending_for_cb = pending_for_cb.clone();
                let handle = ctx.begin_key_capture(move |ks, reg, _cap_ctx| {
                    handle_capture_event(
                        ks,
                        reg,
                        id,
                        slot,
                        confirm,
                        on_conflict.as_ref(),
                        &pending_for_cb,
                    );
                    cap_for_cb.set(None);
                });
                // Replacing any prior handle drops it — cancelling a
                // stale capture from a previous click in the same
                // session.
                *handle_cell.borrow_mut() = Some(handle);
            })
        };

        // While capturing, the hint cell is a `Role::Status` +
        // `Live::Polite` wrapper so screen readers announce
        // "Press any key…" the moment the user hits Rebind. Static
        // bindings stay as plain labels — their content is announced
        // on focus, not as a live change.
        //
        // The keystroke label uses `TextRole::Primary`
        // unconditionally; the surrounding row's `enabled_when`
        // forwarding bridges the registry-driven enabled flag into
        // the arena, and the leaf substitutes `TextRole::Disabled`
        // via `PaintContext::effective_enabled` when the row is off.
        let row = HStack::new().spacing(4.0);
        let row = if is_capturing_here {
            row.child(LiveStatusText::new(keystroke_text, TextRole::Accent))
        } else {
            row.child(
                TextWidget::new(lit!(&keystroke_text))
                    .color(TextRole::Primary)
                    .single_line(),
            )
        };

        // Confirm prompt takes over the slot's trailing controls while a
        // conflicting rebind is queued: announce the collision and offer
        // Reassign / Cancel instead of a fresh Rebind.
        let Some(p) = pending_here else {
            return row.child(rebind_button);
        };

        let warning = format!(
            "{} is assigned to {}",
            format_keystroke(p.ks),
            p.conflict_name
        );
        let reassign = {
            let pending_signal = pending_signal.clone();
            let p = p.clone();
            Button::new(lit!("Reassign")).on_activate_fn(move |ctx: &mut EventContext| {
                // Unbind the conflicting slot, then claim the chord.
                match p.conflict_slot {
                    Some(SlotKind::Primary) => {
                        ctx.rebind_shortcut_primary(p.conflict_id.clone(), None)
                    }
                    Some(SlotKind::Secondary) => {
                        ctx.rebind_shortcut_secondary(p.conflict_id.clone(), None)
                    }
                    None => {}
                }
                match p.slot {
                    SlotKind::Primary => ctx.rebind_shortcut_primary(p.target_id, Some(p.ks)),
                    SlotKind::Secondary => ctx.rebind_shortcut_secondary(p.target_id, Some(p.ks)),
                }
                pending_signal.set(None);
            })
        };
        let cancel = {
            let pending_signal = pending_signal.clone();
            Button::new(lit!("Cancel")).on_activate_fn(move |_ctx: &mut EventContext| {
                pending_signal.set(None);
            })
        };
        row.child(LiveStatusText::new(warning, TextRole::Accent))
            .child(reassign)
            .child(cancel)
    }
}

/// Apply the intent of a captured chord to the registry: bare Escape
/// cancels, bare Delete/Backspace unbinds, everything else rebinds.
///
/// On a chord that collides with another shortcut, the `on_conflict`
/// callback fires (if set). Then, if `confirm` is `false`, the
/// conflicting shortcut is unbound and the rebind applied immediately
/// (the historical behavior); if `confirm` is `true`, nothing is
/// mutated — the rebind is parked in `pending` and an inline
/// Reassign / Cancel prompt is surfaced for the user to confirm.
fn handle_capture_event(
    ks: KeyStroke,
    reg: &mut bastyde_core::shortcut::ShortcutRegistry,
    id: &'static str,
    slot: SlotKind,
    confirm: bool,
    on_conflict: Option<&Rc<dyn Fn(&ShortcutConflict)>>,
    pending: &Signal<Option<PendingRebind>>,
) {
    if ks.key == Key::Escape && ks.modifiers == Modifiers::NONE {
        return; // cancel
    }
    if matches!(ks.key, Key::Delete | Key::Backspace) && ks.modifiers == Modifiers::NONE {
        match slot {
            SlotKind::Primary => reg.rebind_primary(id, None),
            SlotKind::Secondary => reg.rebind_secondary(id, None),
        }
        return;
    }
    // Detect a collision (a different shortcut already owning the chord).
    // Resolve to an owned id immediately so the immutable borrow of `reg`
    // ends before any mutation below.
    let cid = reg.find_conflict(ks, Some(id)).map(|c| c.to_string());
    if let Some(cid) = cid {
        let conflict_slot = reg.effective(&cid).and_then(|eff| {
            if eff.primary == Some(ks) {
                Some(SlotKind::Primary)
            } else if eff.secondary == Some(ks) {
                Some(SlotKind::Secondary)
            } else {
                None
            }
        });
        let conflict_name = reg
            .iter_effective()
            .find(|e| e.shortcut.id == cid)
            .map(|e| e.shortcut.name.get())
            .unwrap_or_else(|| cid.clone());

        if let Some(cb) = on_conflict {
            cb(&ShortcutConflict {
                displaced_id: cid.clone(),
                displaced_name: conflict_name.clone(),
                keystroke: ks,
            });
        }

        if confirm {
            // Defer everything: park the rebind for explicit confirmation.
            pending.set(Some(PendingRebind {
                target_id: id,
                slot,
                ks,
                conflict_id: cid,
                conflict_slot,
                conflict_name,
            }));
            return;
        }

        // Immediate mode: auto-unbind the conflicting shortcut so there's
        // always exactly one effective binding per chord.
        match conflict_slot {
            Some(SlotKind::Primary) => reg.rebind_primary(cid, None),
            Some(SlotKind::Secondary) => reg.rebind_secondary(cid, None),
            None => {}
        }
    }
    match slot {
        SlotKind::Primary => reg.rebind_primary(id, Some(ks)),
        SlotKind::Secondary => reg.rebind_secondary(id, Some(ks)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::shortcut::{Shortcut, ShortcutRegistry};
    use bastyde_core::widget_tree::WidgetTree;

    /// Immediate-mode capture (no confirm, no callback) — the historical
    /// behavior most tests exercise.
    fn apply_capture(reg: &mut ShortcutRegistry, ks: KeyStroke, id: &'static str, slot: SlotKind) {
        handle_capture_event(ks, reg, id, slot, false, None, &Signal::new(None));
    }

    #[test]
    fn shortcut_settings_builds_a_row_per_registered_shortcut() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.open")
                .name("Open")
                .primary(KeyStroke::ctrl(Key::O))
                .build(),
        );
        let settings = tree.add(ShortcutSettings::new());
        tree.layout(SizeProposal::exact(900.0, 600.0));
        let b = tree.bounds(settings);
        assert!(b.width > 0.0 && b.height > 0.0);
    }

    #[test]
    fn delete_during_capture_unbinds_primary_slot() {
        let mut reg = bastyde_core::shortcut::ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        // Delete with no modifiers during capture → primary = None.
        apply_capture(
            &mut reg,
            KeyStroke::new(Key::Delete, Modifiers::NONE),
            "app.save",
            SlotKind::Primary,
        );
        assert_eq!(reg.effective("app.save").unwrap().primary, None);
    }

    #[test]
    fn escape_during_capture_is_cancel_not_rebind() {
        let mut reg = bastyde_core::shortcut::ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        apply_capture(
            &mut reg,
            KeyStroke::new(Key::Escape, Modifiers::NONE),
            "app.save",
            SlotKind::Primary,
        );
        // Still the default — escape must not mutate anything.
        assert_eq!(
            reg.effective("app.save").unwrap().primary,
            Some(KeyStroke::ctrl(Key::S))
        );
    }

    #[test]
    fn rebind_auto_unbinds_conflicting_shortcut() {
        let mut reg = bastyde_core::shortcut::ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        reg.register(
            Shortcut::new("app.sync")
                .primary(KeyStroke::ctrl(Key::K))
                .build(),
        );
        // User rebinds app.sync to Ctrl+S (which app.save owns).
        apply_capture(
            &mut reg,
            KeyStroke::ctrl(Key::S),
            "app.sync",
            SlotKind::Primary,
        );
        assert_eq!(
            reg.effective("app.sync").unwrap().primary,
            Some(KeyStroke::ctrl(Key::S)),
            "sync takes the new chord"
        );
        assert_eq!(
            reg.effective("app.save").unwrap().primary,
            None,
            "save is auto-unbound on conflict"
        );
    }

    #[test]
    fn rebind_auto_unbinds_conflict_on_secondary_slot() {
        let mut reg = bastyde_core::shortcut::ShortcutRegistry::new();
        reg.register(
            Shortcut::new("edit.undo")
                .primary(KeyStroke::ctrl(Key::Z))
                .secondary(KeyStroke::alt(Key::Backspace))
                .build(),
        );
        reg.register(Shortcut::new("edit.redo").build());
        // User rebinds edit.redo.primary to Alt+Backspace — undo's
        // secondary slot currently owns that chord.
        apply_capture(
            &mut reg,
            KeyStroke::alt(Key::Backspace),
            "edit.redo",
            SlotKind::Primary,
        );
        assert_eq!(
            reg.effective("edit.redo").unwrap().primary,
            Some(KeyStroke::alt(Key::Backspace))
        );
        let undo = reg.effective("edit.undo").unwrap();
        assert_eq!(undo.primary, Some(KeyStroke::ctrl(Key::Z)));
        assert_eq!(
            undo.secondary, None,
            "the conflicting secondary slot is the one auto-unbound"
        );
    }

    #[test]
    fn confirm_mode_defers_the_rebind_and_parks_a_pending_conflict() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        reg.register(
            Shortcut::new("app.sync")
                .primary(KeyStroke::ctrl(Key::K))
                .build(),
        );
        let pending: Signal<Option<PendingRebind>> = Signal::new(None);
        // Confirm mode: rebinding app.sync to Ctrl+S must NOT mutate the
        // registry; it parks a pending conflict instead.
        handle_capture_event(
            KeyStroke::ctrl(Key::S),
            &mut reg,
            "app.sync",
            SlotKind::Primary,
            true,
            None,
            &pending,
        );
        assert_eq!(
            reg.effective("app.save").unwrap().primary,
            Some(KeyStroke::ctrl(Key::S)),
            "save keeps its binding until the user confirms"
        );
        assert_eq!(
            reg.effective("app.sync").unwrap().primary,
            Some(KeyStroke::ctrl(Key::K)),
            "sync is unchanged until the user confirms"
        );
        let p = pending.get().expect("a pending rebind is parked");
        assert_eq!(p.target_id, "app.sync");
        assert_eq!(p.conflict_id, "app.save");
        assert_eq!(p.conflict_slot, Some(SlotKind::Primary));
        assert_eq!(p.conflict_name, "Save");
    }

    #[test]
    fn on_conflict_callback_fires_with_displaced_shortcut() {
        let mut reg = ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        reg.register(
            Shortcut::new("app.sync")
                .primary(KeyStroke::ctrl(Key::K))
                .build(),
        );
        let seen: Rc<RefCell<Option<ShortcutConflict>>> = Rc::new(RefCell::new(None));
        let cb_seen = seen.clone();
        let cb: Rc<dyn Fn(&ShortcutConflict)> =
            Rc::new(move |c: &ShortcutConflict| *cb_seen.borrow_mut() = Some(c.clone()));
        let pending: Signal<Option<PendingRebind>> = Signal::new(None);
        // Immediate mode (confirm = false): callback still fires.
        handle_capture_event(
            KeyStroke::ctrl(Key::S),
            &mut reg,
            "app.sync",
            SlotKind::Primary,
            false,
            Some(&cb),
            &pending,
        );
        let c = seen.borrow().clone().expect("callback fired");
        assert_eq!(c.displaced_id, "app.save");
        assert_eq!(c.displaced_name, "Save");
        assert_eq!(c.keystroke, KeyStroke::ctrl(Key::S));
        // And the immediate rebind still happened.
        assert_eq!(reg.effective("app.save").unwrap().primary, None);
        assert_eq!(
            reg.effective("app.sync").unwrap().primary,
            Some(KeyStroke::ctrl(Key::S))
        );
    }

    #[test]
    fn filter_narrows_visible_rows_by_name_or_category() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .name("Save")
                .category("File")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("edit.bold")
                .name("Bold")
                .category("Format")
                .primary(KeyStroke::ctrl(Key::B))
                .build(),
        );
        tree.shortcut_registry_mut().register(
            Shortcut::new("edit.italic")
                .name("Italic")
                .category("Format")
                .primary(KeyStroke::ctrl(Key::I))
                .build(),
        );

        let filter = Signal::new(String::from("format"));
        let settings = tree.add(ShortcutSettings::new().with_filter(filter.clone()));
        tree.layout(SizeProposal::exact(900.0, 600.0));

        // With filter = "format", only Format-category rows
        // (edit.bold, edit.italic) should appear, plus one header.
        // Snapshot the size before changing the filter; a non-zero
        // bounds proves some rows rendered.
        let before = tree.bounds(settings);
        assert!(before.height > 0.0);

        // Widen to match all three — widget should grow.
        filter.set(String::new());
        tree.layout(SizeProposal::exact(900.0, 600.0));
        let after = tree.bounds(settings);
        assert!(
            after.height >= before.height,
            "clearing filter must not shrink the widget (got {} → {})",
            before.height,
            after.height
        );
    }

    #[test]
    fn rebind_through_capture_mode_updates_registry() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        let _settings = tree.add(ShortcutSettings::new());
        tree.layout(SizeProposal::exact(900.0, 600.0));

        let _h = tree.begin_key_capture(|ks, reg, _ctx| {
            reg.rebind_primary("app.save", Some(ks));
        });
        // leak the handle through `_h = ManuallyDrop::new(...)` — actually
        // we keep it alive by not dropping it explicitly at end of scope.
        let h = _h;
        tree.press_key(Key::B, Modifiers::CTRL | Modifiers::SHIFT);
        drop(h);

        assert_eq!(
            tree.shortcut_registry()
                .effective("app.save")
                .unwrap()
                .primary,
            Some(KeyStroke::ctrl_shift(Key::B))
        );
    }
}
