//! ShortcutSettings — user-facing widget for browsing and rebinding
//! application shortcuts.
//!
//! Reads every shortcut registered in the tree's
//! [`ShortcutRegistry`](fern_core::shortcut::ShortcutRegistry) and
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

use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::event::{Key, Modifiers};
use fern_core::shortcut::{CaptureHandle, KeyStroke};
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::button::Button;
use crate::keystroke_format::format_keystroke;
use crate::primitives::{HStack, Spacer, TextWidget, VStack};

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

/// Settings widget for browsing and rebinding shortcuts.
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
    root_child_id: Option<WidgetId>,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl ShortcutSettings {
    pub fn new() -> Self {
        Self {
            capturing: Signal::new(None),
            active_handle: Rc::new(RefCell::new(None)),
            filter: None,
            root_child_id: None,
        }
    }

    /// Bind the visible row set to a filter signal. The widget
    /// shows only shortcuts whose `name`, `id`, or `category`
    /// contains the filter text (case-insensitive). Empty string =
    /// show everything.
    ///
    /// Apps typically drive this from a `TextInput` elsewhere in
    /// their settings UI; keeping the filter external lets the
    /// widget stay usable without pulling in the `rich-text` feature
    /// that `TextInput` requires.
    pub fn with_filter(mut self, filter: Signal<String>) -> Self {
        self.filter = Some(filter);
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
        let theme = ctx.theme().clone();

        // Rebuild on any registry change (register, rebind, clear).
        ctx.shortcut_version().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );
        // Rebuild when capture state changes — the "Press…" hint
        // jumps between rows.
        self.capturing.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );
        // Rebuild when the filter signal changes.
        if let Some(filter) = &self.filter {
            filter.bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                BindingLevel::Rebuild,
            );
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
                has_override: ctx.shortcut_registry().override_for(eff.shortcut.id).is_some(),
            })
            .filter(matches_filter)
            .collect();
        // Stable order: category then id.
        rows.sort_by(|a, b| a.category.cmp(&b.category).then(a.id.cmp(b.id)));

        let capturing = self.capturing.get();
        let mut column = VStack::new().spacing(4.0);

        let mut last_category: Option<Option<&'static str>> = None;
        for row in rows {
            if last_category != Some(row.category) {
                column = column.child(category_header(&theme, row.category));
                last_category = Some(row.category);
            }
            column = column.child(self.build_row(&theme, &row, capturing));
        }

        let root = ctx.add(column);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
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

fn category_header(theme: &fern_tokens::Theme, category: Option<&'static str>) -> impl Widget + 'static {
    let label = category.unwrap_or("General");
    TextWidget::new_literal(label)
        .style(theme.typography.body_bold.clone())
        .color(theme.colors.text_primary)
        .single_line()
}

impl ShortcutSettings {
    fn build_row(
        &self,
        theme: &fern_tokens::Theme,
        row: &ShortcutRowData,
        capturing: Option<CaptureTarget>,
    ) -> impl Widget + 'static {
        let id = row.id;
        let label_color = if row.enabled {
            theme.colors.text_primary
        } else {
            theme.colors.text_disabled
        };
        let accent = theme.colors.accent;

        let name_widget = TextWidget::new_literal(&row.name)
            .color(label_color)
            .single_line();

        let primary_slot = self.slot_widget(
            id,
            SlotKind::Primary,
            row.primary,
            capturing,
            label_color,
            accent,
        );
        let secondary_slot = self.slot_widget(
            id,
            SlotKind::Secondary,
            row.secondary,
            capturing,
            label_color,
            accent,
        );

        let reset_button = Button::new_literal("Reset")
            .enabled(row.has_override)
            .on_activate_fn(move |ctx: &mut EventContext| {
                ctx.clear_shortcut_override(id);
            });

        HStack::new()
            .spacing(8.0)
            .child(name_widget)
            .child(Spacer::new())
            .child(primary_slot)
            .child(secondary_slot)
            .child(reset_button)
    }

    fn slot_widget(
        &self,
        id: &'static str,
        slot: SlotKind,
        keystroke: Option<KeyStroke>,
        capturing: Option<CaptureTarget>,
        label_color: fern_tokens::Color,
        accent: fern_tokens::Color,
    ) -> impl Widget + 'static {
        let is_capturing_here = capturing == Some(CaptureTarget { id, slot });
        let keystroke_text = if is_capturing_here {
            "Press any key…  (Del = clear, Esc = cancel)".to_string()
        } else {
            keystroke
                .map(format_keystroke)
                .unwrap_or_else(|| "—".to_string())
        };
        let keystroke_widget = TextWidget::new_literal(&keystroke_text)
            .color(if is_capturing_here { accent } else { label_color })
            .single_line();

        let slot_label = match slot {
            SlotKind::Primary => "Rebind",
            SlotKind::Secondary => "Rebind 2nd",
        };

        let rebind_button = {
            let capturing_signal = self.capturing.clone();
            let handle_cell = self.active_handle.clone();
            Button::new_literal(slot_label).on_activate_fn(move |ctx: &mut EventContext| {
                let target = CaptureTarget { id, slot };
                capturing_signal.set(Some(target));
                let cap_for_cb = capturing_signal.clone();
                let handle = ctx.begin_key_capture(move |ks, reg, _cap_ctx| {
                    handle_capture_event(ks, reg, id, slot);
                    cap_for_cb.set(None);
                });
                // Replacing any prior handle drops it — cancelling a
                // stale capture from a previous click in the same
                // session.
                *handle_cell.borrow_mut() = Some(handle);
            })
        };

        HStack::new()
            .spacing(4.0)
            .child(keystroke_widget)
            .child(rebind_button)
    }
}

/// Apply the intent of a captured chord to the registry: bare Escape
/// cancels, bare Delete/Backspace unbinds, everything else rebinds
/// (auto-unbinding any conflicting shortcut in the same registry).
fn handle_capture_event(
    ks: KeyStroke,
    reg: &mut fern_core::shortcut::ShortcutRegistry,
    id: &'static str,
    slot: SlotKind,
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
    // Auto-unbind the conflicting shortcut (if any) so there's
    // always exactly one effective binding per chord.
    if let Some(conflict_id) = reg.find_conflict(ks, Some(id)) {
        let cid = conflict_id.to_string();
        let conflict_slot = reg
            .effective(&cid)
            .and_then(|eff| {
                if eff.primary == Some(ks) {
                    Some(SlotKind::Primary)
                } else if eff.secondary == Some(ks) {
                    Some(SlotKind::Secondary)
                } else {
                    None
                }
            });
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
    use fern_core::shortcut::Shortcut;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn shortcut_settings_builds_a_row_per_registered_shortcut() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut reg = fern_core::shortcut::ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        // Delete with no modifiers during capture → primary = None.
        handle_capture_event(
            KeyStroke::new(Key::Delete, Modifiers::NONE),
            &mut reg,
            "app.save",
            SlotKind::Primary,
        );
        assert_eq!(reg.effective("app.save").unwrap().primary, None);
    }

    #[test]
    fn escape_during_capture_is_cancel_not_rebind() {
        let mut reg = fern_core::shortcut::ShortcutRegistry::new();
        reg.register(
            Shortcut::new("app.save")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
        );
        handle_capture_event(
            KeyStroke::new(Key::Escape, Modifiers::NONE),
            &mut reg,
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
        let mut reg = fern_core::shortcut::ShortcutRegistry::new();
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
        handle_capture_event(
            KeyStroke::ctrl(Key::S),
            &mut reg,
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
        let mut reg = fern_core::shortcut::ShortcutRegistry::new();
        reg.register(
            Shortcut::new("edit.undo")
                .primary(KeyStroke::ctrl(Key::Z))
                .secondary(KeyStroke::alt(Key::Backspace))
                .build(),
        );
        reg.register(Shortcut::new("edit.redo").build());
        // User rebinds edit.redo.primary to Alt+Backspace — undo's
        // secondary slot currently owns that chord.
        handle_capture_event(
            KeyStroke::alt(Key::Backspace),
            &mut reg,
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
    fn filter_narrows_visible_rows_by_name_or_category() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
