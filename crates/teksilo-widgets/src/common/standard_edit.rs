// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The standard edit commands, as the widgets of this crate take them.
//!
//! Every widget tree binds cut, copy, paste, undo, redo and select-all to
//! rebindable shortcuts ([`StandardEdit`]). A widget that implements any of
//! them registers an action per command with
//! [`register_standard_edit_actions`], on the node that holds the keyboard
//! focus, and no longer matches the chords itself: a key handler that still
//! matched Ctrl+C would keep copying after the user moved Copy elsewhere.

use std::rc::Rc;

use teksilo_core::action::Action;
use teksilo_core::build_context::BuildContext;
use teksilo_core::intent::IntentResponse;
use teksilo_core::shortcut::{ShortcutRegistry, StandardEdit};
use teksilo_core::widget::EventContext;
use teksilo_i18n::{LocalizedString, tr_widget};

/// Register, on the widget being built, an action for every standard edit
/// command. `run` performs one and says whether it did; `available` says
/// whether one has anything to act on now (Copy a selection, Paste an
/// editable field), which is what greys its menu rows.
///
/// A command `run` declines is passed on: to an ancestor's action (an
/// application's own Paste, say), and failing that to the focused widget as
/// the plain key it was. `available` changes none of that: a command it says
/// no to still reaches `run` when its shortcut is pressed, and `run` decides.
/// It is asked outside every handler, once per event-loop turn by the native
/// menu bar, so it reads state and does nothing else.
pub(crate) fn register_standard_edit_actions(
    ctx: &mut BuildContext,
    run: impl Fn(StandardEdit, &mut EventContext) -> bool + 'static,
    available: impl Fn(StandardEdit) -> bool + 'static,
) {
    let run = Rc::new(run);
    let available = Rc::new(available);
    for command in StandardEdit::ALL {
        let run = Rc::clone(&run);
        let available = Rc::clone(&available);
        register_action(
            ctx,
            command.id(),
            move |ctx| run(command, ctx),
            Some(Box::new(move || available(command))),
        );
    }
}

/// [`register_standard_edit_actions`] for a widget that implements one
/// command only (a data view's select-all), with no availability: its rows
/// are enabled whenever the widget is on the route.
pub(crate) fn register_standard_edit_action(
    ctx: &mut BuildContext,
    command: StandardEdit,
    run: impl FnMut(&mut EventContext) -> bool + 'static,
) {
    register_command_action(ctx, command.id(), run);
}

/// Register, on the widget being built, the action for the command `id`
/// names: the standard edit commands, and the editors' own
/// [`FormatShortcut`](crate::editor_shortcuts::FormatShortcut) and
/// [`CodeShortcut`](crate::editor_shortcuts::CodeShortcut). A command `run`
/// declines is passed on, as for [`register_standard_edit_actions`].
pub(crate) fn register_command_action(
    ctx: &mut BuildContext,
    id: &'static str,
    run: impl FnMut(&mut EventContext) -> bool + 'static,
) {
    register_action(ctx, id, run, None);
}

fn register_action(
    ctx: &mut BuildContext,
    id: &'static str,
    mut run: impl FnMut(&mut EventContext) -> bool + 'static,
    available: Option<Box<dyn Fn() -> bool>>,
) {
    let mut action = Action::new(id);
    if let Some(available) = available {
        action = action.available_when(available);
    }
    ctx.register_action(action.on_invoke_with_response(move |_intent, ctx| {
        if run(ctx) {
            IntentResponse::Handled
        } else {
            IntentResponse::Propagated
        }
    }));
}

/// Give the standard edit shortcuts in `registry` their names in the active
/// language, which follow it from then on.
///
/// `teksilo-core` registers them with English names because it carries no
/// translations; `teksilo-app` calls this for every window it opens. A no-op
/// when no i18n manager is installed on this thread, which leaves the English
/// names. User rebindings are kept.
pub fn localize_standard_shortcuts(registry: &mut ShortcutRegistry) {
    if teksilo_i18n::thread_local::with_active(|_| ()).is_none() {
        return;
    }
    for command in StandardEdit::ALL {
        let Some(name) = localized_name(command) else {
            continue;
        };
        let mut shortcut = command.shortcut();
        shortcut.name = name.into();
        shortcut.category_label = Some(tr_widget!(shortcut_category_edit()).into());
        registry.register(shortcut);
    }
}

/// The name a command is shown under: the same words as the context-menu
/// rows that run it.
fn localized_name(command: StandardEdit) -> Option<LocalizedString> {
    Some(match command {
        StandardEdit::Cut => tr_widget!(menu_cut()),
        StandardEdit::Copy => tr_widget!(menu_copy()),
        StandardEdit::Paste => tr_widget!(menu_paste()),
        StandardEdit::PastePlain => tr_widget!(menu_paste_unformatted()),
        StandardEdit::Undo => tr_widget!(menu_undo()),
        StandardEdit::Redo => tr_widget!(menu_redo()),
        StandardEdit::SelectAll => tr_widget!(menu_select_all()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::locale_switch_test::speaking;

    fn shown(registry: &ShortcutRegistry, command: StandardEdit) -> (String, Option<String>) {
        let effective = registry.effective(command.id()).expect("registered");
        (
            effective.shortcut.name.get(),
            effective.shortcut.category_text(),
        )
    }

    #[test]
    fn the_names_follow_the_language_and_the_bindings_survive() {
        let (mgr, mut tree) = speaking("fr-FR");
        let undo_too = teksilo_core::shortcut::KeyStroke::new(
            teksilo_core::event::Key::Character('à'),
            teksilo_core::event::Modifiers::CTRL,
        );
        tree.shortcut_registry_mut()
            .rebind_secondary(StandardEdit::Undo.id(), Some(undo_too));

        localize_standard_shortcuts(tree.shortcut_registry_mut());
        let registry = tree.shortcut_registry();
        assert_eq!(
            shown(registry, StandardEdit::Undo),
            ("Annuler".to_string(), Some("Édition".to_string()))
        );
        assert_eq!(
            shown(registry, StandardEdit::SelectAll).0,
            "Tout sélectionner"
        );
        assert_eq!(
            registry
                .effective(StandardEdit::Undo.id())
                .unwrap()
                .secondary,
            Some(undo_too),
            "localizing must not drop a user's binding"
        );

        mgr.set_locale("en-US".parse().unwrap());
        assert_eq!(
            shown(tree.shortcut_registry(), StandardEdit::Undo),
            ("Undo".to_string(), Some("Edit".to_string())),
            "the names follow a change of language"
        );
        teksilo_i18n::thread_local::clear();
    }

    #[test]
    fn without_a_manager_the_english_names_stay() {
        teksilo_i18n::thread_local::clear();
        let mut tree = teksilo_core::widget_tree::WidgetTree::new();
        localize_standard_shortcuts(tree.shortcut_registry_mut());
        assert_eq!(
            shown(tree.shortcut_registry(), StandardEdit::PastePlain),
            ("Paste Unformatted".to_string(), Some("Edit".to_string()))
        );
    }
}
