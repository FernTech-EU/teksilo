//! Default right-click context menu for `RichTextEditor`.
//!
//! Uses the framework's built-in
//! [`context_menu(factory)`](bastyde_core::widget_builder::WidgetBuilder::context_menu)
//! infrastructure: a fresh menu widget is created on every right-click,
//! shown at the pointer position, and torn down on dismissal. That
//! machinery lives in `bastyde-core::widget_tree::event_dispatch_impl::show_context_menu_for`
//! — we just hand it a factory.
//!
//! ## Why no Actions / Intents for the built-in items
//!
//! The framework's `show_context_menu_for` adds the menu widget at the
//! **top of the arena** (via `add_boxed`). It is *not* a child of the
//! editor. So an intent fired from a menu item walks up the menu's own
//! subtree and terminates there — it never reaches any `Action`
//! registered on the editor.
//!
//! Additionally, when the menu dismisses (the tap's default behaviour)
//! the whole subtree flips dormant recursively in the same
//! `collect_from_ctx` call, *before* the tree's pending-intent queue
//! drains. Any `Action` inside the dormant subtree is skipped by
//! `dispatch_intent`'s `is_active` gate.
//!
//! Both problems go away by **not using Actions for the default menu**.
//! Each `MenuItem`'s `on_activate_fn` closure captures a clone of the
//! editor's [`SharedState`] and calls the corresponding
//! `rt_clipboard::*` function directly. The work happens inline, during
//! the tap handler, while the menu subtree is still active — no
//! dispatch-timing concerns at all.
//!
//! ## Reserved intent names (external observation only)
//!
//! After doing the work directly, each closure also fires a
//! `bastyde.rich_text.*` intent for applications that want to observe
//! (e.g. telemetry, undo-stack annotation, clipboard-manager mirroring).
//! These intents reach ancestor Actions through the normal walk chain —
//! the intent walk starts from the *editor* (via
//! [`EventContext::send_intent`] which anchors on `source_widget`,
//! which for a PointerDown is the hit widget — but we're dispatching
//! from inside the menu item, which is top-level, so the walk
//! terminates without reaching the editor).
//!
//! Since the framework's `show_context_menu_for` makes the menu
//! top-level, intent observation via this path doesn't reach the host
//! app either. Applications that want to observe or override should
//! use the slot instead (see `RichTextEditor::context_menu`). The
//! reserved intent names are reserved for a future reworked dispatch
//! but are not currently useful — we emit them anyway so the contract
//! is stable.
//!
//! ## Slot-based replacement
//!
//! [`RichTextEditor::context_menu`] accepts a user-provided factory
//! that replaces the default entirely. The user's factory returns
//! whatever widget they want — typically a `MenuList`, but any
//! `Widget` works (a `Panel` with custom chrome, a domain-specific
//! command palette, etc.).

use bastyde_i18n::lit;

use bastyde_core::intent::Intent;
use bastyde_core::widget::Widget;

use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;

use super::clipboard as rt_clipboard;
use super::policy::{ClipboardPolicy, EditCommandKind, PolicyBundle};
use super::state::SharedState;

/// Intent fired when the user activates the **Cut** item of the
/// built-in context menu. Reserved for the framework — applications
/// that want bespoke cut semantics should replace the menu via
/// [`RichTextEditor::context_menu`](super::RichTextEditor::context_menu)
/// rather than registering a custom `Action` against this name.
pub const INTENT_CUT: &str = "bastyde.rich_text.cut";

/// Intent fired by the built-in Copy menu item.
pub const INTENT_COPY: &str = "bastyde.rich_text.copy";

/// Intent fired by the built-in Paste menu item.
pub const INTENT_PASTE: &str = "bastyde.rich_text.paste";

/// Intent fired by the built-in Paste Unformatted menu item.
pub const INTENT_PASTE_UNFORMATTED: &str = "bastyde.rich_text.paste_unformatted";

/// Intent fired by the built-in Select All menu item.
pub const INTENT_SELECT_ALL: &str = "bastyde.rich_text.select_all";

/// Build the default context-menu factory for the given editor
/// state and policy.
///
/// The returned closure is installed on the editor's arena node via
/// [`HandlerSet::context_menu`](bastyde_core::widget_builder::HandlerSet::context_menu);
/// the framework calls it on each right-click to produce a **fresh**
/// menu subtree. That freshness matters: each invocation recomputes
/// item enabled-state from the live editor state + live clipboard at
/// the instant the user right-clicks, so greyed entries never lie.
pub(super) fn default_factory(
    state: SharedState,
    policy: PolicyBundle,
) -> RichTextContextMenuFactory {
    // The closure is called each right-click. `state` is captured
    // once and cloned for each menu item's action closure; the
    // `Rc<RefCell<...>>` behind `SharedState` makes that cheap. The
    // built-in menu is unconditional — it always returns
    // `Some(menu)`, ignoring position and ctx. Callers needing a
    // position-aware menu install their own via
    // `RichTextEditor::context_menu`.
    Box::new(move |_pos, _ctx| {
        let state_for_build = state.clone();
        Some(Box::new(build_menu(state_for_build, policy)) as Box<dyn Widget>)
    })
}

/// Construct the `MenuList` for the current editor / policy state.
/// Called from the factory on every right-click.
fn build_menu(state: SharedState, policy: PolicyBundle) -> MenuList {
    let mut list = MenuList::new();

    let has_selection = state.borrow().cursor.has_selection();
    let doc_non_empty = !state
        .borrow()
        .document
        .to_plain_text()
        .unwrap_or_default()
        .is_empty();

    // --- Cut -----------------------------------------------------
    if policy.clipboard_policy.allows_cut() {
        let state_for_cut = state.clone();
        list = list.item(
            MenuItem::new(lit!("Cut"))
                .shortcut_label("Ctrl+X")
                .enabled(has_selection)
                .on_activate_fn(move |evt_ctx| {
                    let mut st = state_for_cut.borrow_mut();
                    rt_clipboard::cut(&mut st, evt_ctx);
                    drop(st);
                    super::sync_cursor_signals(&state_for_cut);
                    evt_ctx.request_frame();
                    evt_ctx.send_intent(Intent::new(INTENT_CUT));
                }),
        );
    }

    // --- Copy ----------------------------------------------------
    {
        let state_for_copy = state.clone();
        list = list.item(
            MenuItem::new(lit!("Copy"))
                .shortcut_label("Ctrl+C")
                .enabled(has_selection)
                .on_activate_fn(move |evt_ctx| {
                    let mut st = state_for_copy.borrow_mut();
                    rt_clipboard::copy(&mut st, evt_ctx);
                    drop(st);
                    evt_ctx.send_intent(Intent::new(INTENT_COPY));
                }),
        );
    }

    // --- Paste ---------------------------------------------------
    // Availability: at factory-call time, probe the clipboard handle
    // via the EventContext path. But here, inside the factory, we have
    // no EventContext. We can only check what the editor itself knows
    // — the stashed `rich_clipboard_fragment` isn't the right signal.
    // Leave Paste always enabled when the policy allows; the closure
    // itself silently no-ops if the clipboard is empty (matches the
    // existing Ctrl+V behaviour).
    if policy.clipboard_policy.allows_paste() {
        let state_for_paste = state.clone();
        list = list.item(
            MenuItem::new(lit!("Paste"))
                .shortcut_label("Ctrl+V")
                .on_activate_fn(move |evt_ctx| {
                    let mut st = state_for_paste.borrow_mut();
                    rt_clipboard::paste(&mut st, evt_ctx);
                    drop(st);
                    super::sync_cursor_signals(&state_for_paste);
                    evt_ctx.request_frame();
                    evt_ctx.send_intent(Intent::new(INTENT_PASTE));
                }),
        );
    }

    // --- Paste Unformatted ---------------------------------------
    if policy.clipboard_policy.allows_paste_unformatted() {
        let state_for_pu = state.clone();
        list = list.item(
            MenuItem::new(lit!("Paste Unformatted"))
                .shortcut_label("Ctrl+Shift+V")
                .on_activate_fn(move |evt_ctx| {
                    let mut st = state_for_pu.borrow_mut();
                    rt_clipboard::paste_unformatted(&mut st, evt_ctx);
                    drop(st);
                    super::sync_cursor_signals(&state_for_pu);
                    evt_ctx.request_frame();
                    evt_ctx.send_intent(Intent::new(INTENT_PASTE_UNFORMATTED));
                }),
        );
    }

    // --- Toggle blockquote ---------------------------------------
    // Only meaningful when the editor accepts mutations (read-only
    // presets get the minimal menu — Cut/Copy/Paste/Select-All).
    if policy.command_filter.accepts(EditCommandKind::ToggleBlockquote) {
        let state_for_bq = state.clone();
        let cross_frame_selection = state.borrow().cursor.selection_spans_multiple_frames();
        let in_quote = state.borrow().cursor.is_in_blockquote();
        let label = if in_quote {
            lit!("Remove blockquote")
        } else {
            lit!("Toggle blockquote")
        };
        list = list.separator();
        list = list.item(
            MenuItem::new(label)
                .enabled(!cross_frame_selection)
                .on_activate_fn(move |evt_ctx| {
                    {
                        let st = state_for_bq.borrow();
                        let _ = st.cursor.toggle_blockquote();
                    }
                    super::sync_cursor_signals(&state_for_bq);
                    evt_ctx.request_frame();
                }),
        );
    }

    // Separator before Select All under the full policy; read-only
    // presets keep the menu minimal (no separator).
    if matches!(policy.clipboard_policy, ClipboardPolicy::Full) {
        list = list.separator();
    }

    // --- Select All ----------------------------------------------
    {
        let state_for_sa = state.clone();
        list = list.item(
            MenuItem::new(lit!("Select All"))
                .shortcut_label("Ctrl+A")
                .enabled(doc_non_empty)
                .on_activate_fn(move |evt_ctx| {
                    {
                        let mut st = state_for_sa.borrow_mut();
                        st.cursor
                            .select(bastyde_text::text_document::SelectionType::Document);
                        st.select_all_level = 0;
                        st.select_all_anchor_cell = None;
                    }
                    super::sync_cursor_signals(&state_for_sa);
                    evt_ctx.request_frame();
                    evt_ctx.send_intent(Intent::new(INTENT_SELECT_ALL));
                }),
        );
    }

    list
}

/// Resolve which factory (if any) the editor should install for its
/// arena node's `context_menu_factory`. Precedence:
///
/// 1. **User factory** (supplied via `RichTextEditor::context_menu`) —
///    always wins when provided. The host app is explicitly replacing
///    the default.
/// 2. **Default factory** when `default_context_menu` is enabled
///    (the default) and no user factory is set.
/// 3. **No factory** when the host called
///    `default_context_menu(false)` — right-click bubbles past the
///    widget unhandled; `context_target_at` remains available so the
///    app can render its own menu from outside.
pub(super) fn resolve_factory(
    user_factory: Option<RichTextContextMenuFactory>,
    default_enabled: bool,
    state: SharedState,
    policy: PolicyBundle,
) -> Option<RichTextContextMenuFactory> {
    if let Some(user) = user_factory {
        return Some(user);
    }
    if default_enabled {
        return Some(default_factory(state, policy));
    }
    None
}

/// Internal alias — same shape as the framework's
/// [`bastyde_core::widget_builder::ContextMenuFactory`]. Re-declared
/// locally so the rich-text module doesn't have to thread the public
/// alias through every signature.
pub(super) type RichTextContextMenuFactory = Box<
    dyn Fn(
        bastyde_canvas::Point,
        &mut bastyde_core::widget::EventContext,
    ) -> Option<Box<dyn Widget>>,
>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_names_use_reserved_bastyde_prefix() {
        // The `bastyde.` prefix is the framework's reserved namespace
        // for built-in plumbing; applications that want to register
        // custom intents should use their own prefix. Locking the
        // strings makes any rename a deliberate, breaking change.
        assert_eq!(INTENT_CUT, "bastyde.rich_text.cut");
        assert_eq!(INTENT_COPY, "bastyde.rich_text.copy");
        assert_eq!(INTENT_PASTE, "bastyde.rich_text.paste");
        assert_eq!(
            INTENT_PASTE_UNFORMATTED,
            "bastyde.rich_text.paste_unformatted"
        );
        assert_eq!(INTENT_SELECT_ALL, "bastyde.rich_text.select_all");
    }
}
