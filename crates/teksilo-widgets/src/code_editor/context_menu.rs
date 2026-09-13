// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The right-click menu — and, from the same four rows, the touch selection
//! toolbar — for the code editor, the plain-text editor and the log view.
//!
//! These three surfaces had **no context menu at all**: a right-click bubbled
//! past them, so the only route to Cut / Copy / Paste was the chord, and on a
//! read-only `LogView` — where copying is the *only* thing a reader can do —
//! `Ctrl+C` was the only route to it. A finger has no chord and no second
//! button, which is what makes this a prerequisite for touch rather than a
//! nicety.
//!
//! # Why no Actions / Intents for the built-in items
//!
//! `show_context_menu_for` adds the menu widget at the **top of the arena**, so
//! it is not a child of the editor: an intent fired from a menu row walks up the
//! *menu*'s subtree and terminates there. Worse, dismissing the menu flips its
//! whole subtree dormant in the same `collect_from_ctx` call, before the pending
//! intent queue drains, so an `Action` inside it is skipped by
//! `dispatch_intent`'s `is_active` gate. Each row therefore captures the
//! editor's [`SharedState`] and does the work inline, during the tap handler,
//! while the subtree is still active — the same conclusion, for the same two
//! reasons, that
//! `rich_text`'s own context-menu module reached.
//!
//! # One set of rows, two surfaces
//!
//! The menu and the toolbar offer the same commands, so they are the same rows.
//! Each builder returns a row without an `enabled` gate: the menu adds one (a
//! greyed row is the desktop convention), while the toolbar **omits** a command
//! it cannot offer, through `MenuList::item_when` gated on the touch
//! controller's published
//! [`ClipboardActions`](teksilo_core::text_touch::ClipboardActions) — which is
//! what every platform's touch toolbar does. See `docs/text-touch-editing.md`.

use teksilo_i18n::tr_widget;

use teksilo_core::event::Key;
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::widget::Widget;

use crate::keystroke_format::format_keystroke;
use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;

use teksilo_core::text_touch::TextAction;

use super::policy::CodeCommand;
use super::state::SharedState;
use crate::common::editor_runtime::PolicyBundle;

/// Cut the selection, or the caret's whole line when there is none — the same
/// `Ctrl+X` this editor already had.
fn row_cut(state: &SharedState) -> MenuItem {
    let state = state.clone();
    MenuItem::new(tr_widget!(menu_cut()))
        .shortcut_label(format_keystroke(KeyStroke::command(Key::X)))
        .on_activate_fn(move |ctx| {
            {
                let mut st = state.borrow_mut();
                super::clipboard::cut(&mut st, ctx);
                st.pending_text_changed = true;
            }
            super::sync_cursor_signals(&state);
            ctx.request_frame();
        })
}

/// Copy the selection, or the caret's whole line when there is none.
fn row_copy(state: &SharedState) -> MenuItem {
    let state = state.clone();
    MenuItem::new(tr_widget!(menu_copy()))
        .shortcut_label(format_keystroke(KeyStroke::command(Key::C)))
        .on_activate_fn(move |ctx| {
            super::clipboard::copy(&state.borrow(), ctx);
        })
}

/// Paste at the primary caret.
fn row_paste(state: &SharedState) -> MenuItem {
    let state = state.clone();
    MenuItem::new(tr_widget!(menu_paste()))
        .shortcut_label(format_keystroke(KeyStroke::command(Key::V)))
        .on_activate_fn(move |ctx| {
            {
                let mut st = state.borrow_mut();
                super::clipboard::paste(&mut st, ctx);
                st.pending_text_changed = true;
            }
            super::sync_cursor_signals(&state);
            ctx.request_frame();
        })
}

/// Select the whole document.
fn row_select_all(state: &SharedState) -> MenuItem {
    let state = state.clone();
    MenuItem::new(tr_widget!(menu_select_all()))
        .shortcut_label(format_keystroke(KeyStroke::command(Key::A)))
        .on_activate_fn(move |ctx| {
            {
                let mut st = state.borrow_mut();
                st.clear_extra_carets();
                st.cursor
                    .select(teksilo_text::text_document::SelectionType::Document);
            }
            super::sync_cursor_signals(&state);
            ctx.request_frame();
        })
}

/// The commands this surface offers, in menu order — **one** source of truth for
/// the right-click menu and the touch toolbar.
///
/// Each answer is the predicate the surface's own chord consults, so neither
/// surface can offer a command the keyboard would refuse or refuse one it allows.
/// Read from the live [`PolicyBundle`] rather than from a snapshot: the command
/// filter is swappable on a mounted editor, and a menu built from a stale one
/// would keep offering a Cut the surface has stopped accepting.
///
/// Note Cut is offered with **no selection** too — it takes the caret's whole
/// line, the desktop convention this editor's `Ctrl+X` already follows. Whether
/// a row is *enabled* (the menu) or *shown* (the toolbar) is decided by the
/// caller; this says only what the surface will honour.
pub(super) fn offered(policy: &PolicyBundle) -> Vec<TextAction> {
    let mut out = Vec::with_capacity(4);
    if policy.clipboard_policy.allows_cut() && policy.command_filter.accepts(CodeCommand::Cut) {
        out.push(TextAction::Cut);
    }
    if policy.clipboard_policy.allows_copy() && policy.command_filter.accepts(CodeCommand::Copy) {
        out.push(TextAction::Copy);
    }
    if policy.clipboard_policy.allows_paste() && policy.command_filter.accepts(CodeCommand::Paste) {
        out.push(TextAction::Paste);
    }
    if policy.command_filter.accepts(CodeCommand::SelectAll) {
        out.push(TextAction::SelectAll);
    }
    out
}

/// Whether the surface's own `Ctrl+C` would run — the predicate
/// [`TextHitSource::allows_copy`](teksilo_core::text_touch::TextHitSource::allows_copy)
/// reports, so the touch toolbar and the keyboard cannot disagree.
pub(super) fn copy_allowed(policy: &PolicyBundle) -> bool {
    offered(policy).contains(&TextAction::Copy)
}

/// The row for one command, or `None` for one this surface has no command
/// behind.
///
/// [`TextAction::Custom`] is a host's own entry, contributed to a toolbar the
/// host builds itself; the built-in rows are the four this surface's keyboard
/// already runs, and [`offered`] never names another.
fn row_for(action: TextAction, state: &SharedState) -> Option<MenuItem> {
    Some(match action {
        TextAction::Cut => row_cut(state),
        TextAction::Copy => row_copy(state),
        TextAction::Paste => row_paste(state),
        TextAction::SelectAll => row_select_all(state),
        TextAction::Custom(_) => return None,
    })
}

/// Build the right-click menu for the live state.
///
/// Called on **every** right-click, so each row's enabled state is recomputed
/// from the selection and the policy at the instant the menu opens.
fn build_menu(state: SharedState) -> MenuList {
    let (policy, doc_non_empty) = {
        let st = state.borrow();
        (st.policy, st.document.character_count() > 0)
    };
    let actions = offered(&policy);
    let mut list = MenuList::new();
    for (i, action) in actions.iter().enumerate() {
        // A separator before Select All, but only when a clipboard group
        // precedes it — a read-only surface's two rows are one group.
        if *action == TextAction::SelectAll && i > 0 {
            list = list.separator();
        }
        let Some(row) = row_for(*action, &state) else {
            continue;
        };
        list = list.item(match action {
            // Paste does not depend on the document; the clipboard it reads is
            // not visible from here, and the closure no-ops when it is empty —
            // which is exactly what this surface's `Ctrl+V` already does.
            TextAction::Paste => row,
            _ => row.enabled(doc_non_empty),
        });
    }
    list
}

/// The factory the wrapper installs on its arena node.
///
/// `position` is a **window** point; the caret is repositioned to it unless the
/// click landed inside the current selection, which is the platform convention
/// for "right-click, then Cut / Copy / Paste at the new caret". A read-only
/// surface is exempt: its caret is invisible, so a reposition buys nothing there
/// and would destroy the selection the reader was about to copy.
pub(super) fn factory(state: SharedState) -> CodeContextMenuFactory {
    Box::new(move |position, _ctx| {
        if !state.borrow().policy.is_read_only() {
            reposition_caret(&state, position);
        }
        Some(Box::new(build_menu(state.clone())) as Box<dyn Widget>)
    })
}

/// Move the caret to a right-click **window** point when it lands outside the
/// current selection.
///
/// This has to run from inside the factory: `show_context_menu_for` consumes the
/// Secondary `PointerDown` and returns before `dispatch_to_widget` runs, so the
/// editor's own pointer handler never sees it.
fn reposition_caret(state: &SharedState, window: teksilo_canvas::Point) {
    let hit = {
        let st = state.borrow();
        let local = super::touch::window_to_engine_local(&st, window);
        crate::rich_text::hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
    };
    let Some(hit) = hit else { return };
    {
        let st = state.borrow();
        if st.cursor.has_selection() {
            let (lo, hi) = (
                st.cursor.anchor().min(st.cursor.position()),
                st.cursor.anchor().max(st.cursor.position()),
            );
            if hit.position >= lo && hit.position <= hi {
                // Inside the selection — keep it so Cut / Copy act on it.
                return;
            }
        }
    }
    {
        let mut st = state.borrow_mut();
        st.clear_extra_carets();
        st.cursor.set_position(
            hit.position,
            teksilo_text::text_document::MoveMode::MoveAnchor,
        );
        st.cursor_affinity = hit.affinity;
        st.preferred_x = None;
    }
    super::sync_cursor_signals(state);
}

/// The selection toolbar a finger raises: the same four rows, each shown only
/// while the controller says the surface will honour it **and** this surface's
/// own policy allows it.
///
/// Built once per `build()` rather than per raise, because it is overlay content
/// and an `EventContext` cannot add widgets. Reactive visibility does the work a
/// fresh build would.
pub(super) fn selection_toolbar(
    state: SharedState,
    affordances: teksilo_core::text_touch::TextAffordances,
) -> Box<dyn Widget> {
    let offers = |action: TextAction| {
        let aff = affordances.clone();
        let state = state.clone();
        affordances.version_signal().map(move |_| {
            offered(&state.borrow().policy).contains(&action)
                && aff.toolbar().is_some_and(|t| t.actions.contains(&action))
        })
    };
    let mut list = MenuList::new();
    for action in [
        TextAction::Cut,
        TextAction::Copy,
        TextAction::Paste,
        TextAction::SelectAll,
    ] {
        let Some(row) = row_for(action, &state) else {
            continue;
        };
        list = list.item_when(row, offers(action));
    }
    Box::new(list)
}

/// Resolve which factory (if any) the surface installs. Precedence: a
/// host-supplied factory always wins; then the built-in one while
/// `default_enabled`; then none at all, in which case a right-click bubbles past
/// the surface and the application can render its own menu.
pub(super) fn resolve_factory(
    user_factory: Option<CodeContextMenuFactory>,
    default_enabled: bool,
    state: SharedState,
) -> Option<CodeContextMenuFactory> {
    if let Some(user) = user_factory {
        return Some(user);
    }
    default_enabled.then(|| factory(state))
}

/// Same shape as the framework's
/// [`ContextMenuFactory`](teksilo_core::widget_builder::ContextMenuFactory),
/// re-declared locally so this module does not thread the public alias through
/// every signature.
pub(super) type CodeContextMenuFactory = Box<
    dyn Fn(
        teksilo_canvas::Point,
        &mut teksilo_core::widget::EventContext,
    ) -> Option<Box<dyn Widget>>,
>;
