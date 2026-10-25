// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The application menu's reach into the windows.
//!
//! The application tree declares the application menu
//! ([`ApplicationMenuExt::set_application_menu`](teksilo_widgets::menu::ApplicationMenuExt)),
//! and registers the shortcuts its rows name there. A key, though, is
//! resolved in the window it is pressed in, against that window's
//! [`ShortcutRegistry`]: a shortcut only the application tree registers
//! never fires. So each window gets a copy of the application tree's
//! declaration of every shortcut a row of the application menu names, unless
//! the window registers that id itself, owned or
//! [shared](teksilo_core::shortcut::ShortcutRegistry::register_shared) (the
//! commands its editors share). The copy sends the shortcut's intent
//! by name, as a shortcut nothing in a window takes goes to the application
//! tree; a window shows its chord in the menu and its settings UI lists it.
//!
//! The copies are unowned, like the standard edit shortcuts: a dialog
//! inside the window does not stop them, as the application scope they
//! lead to stays reachable from one.

use std::collections::HashMap;

use teksilo_core::shortcut::{KeyStrokeOverride, Shortcut, ShortcutRegistry, ShortcutScope};
use teksilo_core::signal::{Prop, Signal};

/// What identifies the declarations a window's copies were made from: the
/// application menu's revision (its generation and its model's version) and
/// the application tree's shortcut registry version.
pub(crate) type Source = (u64, u64, u64);

/// The copies of application-menu shortcuts one window holds.
#[derive(Debug, Default)]
pub(crate) struct MirroredShortcuts {
    /// The ids this window holds a copy of, which it does not register
    /// itself.
    ids: Vec<&'static str>,
    /// For each copy, the user override the window was given with it: the
    /// application tree's at the last sync. The window's override differing
    /// from it is a rebind the user made in this window.
    given: HashMap<&'static str, Option<KeyStrokeOverride>>,
    /// The source and the window registry's version after the last sync:
    /// while both are unchanged there is nothing to do.
    synced: Option<(Source, u64)>,
}

impl MirroredShortcuts {
    /// Bring `window`'s copies in line with `wanted`, the shortcut ids the
    /// application menu names, as `app` declares them, with `app`'s user
    /// overrides of them. An id the window registers itself is left alone,
    /// and a copy the window has since registered itself (owned, or shared
    /// as an editor command is) stops being one; a copy no longer wanted is
    /// removed, with the override it was given. Cheap when nothing changed
    /// since the last call.
    ///
    /// A rebind the user made of a copy in this window is overwritten:
    /// [`sync_all`] takes it to `app` first, with [`rebinds`](Self::rebinds).
    pub(crate) fn sync(
        &mut self,
        source: Source,
        wanted: impl FnOnce() -> Vec<&'static str>,
        app: &ShortcutRegistry,
        window: &mut ShortcutRegistry,
    ) {
        if self.synced == Some((source, window.version().get())) {
            return;
        }
        let wanted = wanted();
        // A copy the window replaced with its own registration is its own,
        // and so is the override it holds for it.
        self.ids
            .retain(|id| window.get_default(id).is_some() && !window.has_owner(id));
        self.given.retain(|id, _| self.ids.contains(id));
        for id in std::mem::take(&mut self.ids) {
            if !wanted.contains(&id) || app.get_default(id).is_none() {
                window.unregister(id);
                if self.given.remove(id) == Some(window.override_for(id)) {
                    window.clear_override(id);
                }
            } else {
                self.ids.push(id);
            }
        }
        for id in wanted {
            let Some(declared) = app.get_default(id) else {
                continue;
            };
            let ours = self.ids.contains(&id);
            if ours || window.get_default(id).is_none() {
                // A copy as it is stays: registering it again would move
                // the window registry's version for nothing.
                if !window
                    .get_default(id)
                    .is_some_and(|copy| same_copy(copy, declared))
                {
                    window.register(mirror(declared));
                }
                if !ours {
                    self.ids.push(id);
                }
                let override_ = app.override_for(id);
                if window.override_for(id) != override_ {
                    match override_ {
                        Some(override_) => window.put_override(id, override_),
                        None => window.clear_override(id),
                    }
                }
                self.given.insert(id, override_);
            }
        }
        self.synced = Some((source, window.version().get()));
    }

    /// The rebinds the user made of this window's copies since the last
    /// [`sync`](Self::sync), each the copy's id and the window's override
    /// for it now (`None`: reset to the declared chords). Empty while the
    /// window's registry has not changed since then.
    pub(crate) fn rebinds(
        &self,
        window: &ShortcutRegistry,
    ) -> Vec<(&'static str, Option<KeyStrokeOverride>)> {
        if self
            .synced
            .is_some_and(|(_, version)| version == window.version().get())
        {
            return Vec::new();
        }
        self.ids
            .iter()
            .filter(|id| window.get_default(id).is_some() && !window.has_owner(id))
            .filter_map(|id| {
                let now = window.override_for(id);
                (self.given.get(id) != Some(&now)).then_some((*id, now))
            })
            .collect()
    }

    /// The ids this window holds a copy of.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> &[&'static str] {
        &self.ids
    }
}

/// Bring every window's copies in line with the application menu, whose
/// revision is `menu`, and the application tree's registry `app`.
///
/// A user's rebind of an application-menu shortcut applies to the
/// application. A window's `ShortcutSettings` rebinds the copy in that
/// window's registry, so each rebind found there ([`MirroredShortcuts::rebinds`])
/// is first made in `app`, then every window's copy is given `app`'s
/// override, and the native menu bar, which reads `app`, follows.
pub(crate) fn sync_all(
    menu: (u64, u64),
    wanted: impl Fn() -> Vec<&'static str>,
    app: &mut ShortcutRegistry,
    windows: &mut [(&mut MirroredShortcuts, &mut ShortcutRegistry)],
) {
    for (mirrored, window) in windows.iter() {
        for (id, override_) in mirrored.rebinds(window) {
            match override_ {
                Some(override_) => app.put_override(id, override_),
                None => app.clear_override(id),
            }
        }
    }
    let source = (menu.0, menu.1, app.version().get());
    for (mirrored, window) in windows.iter_mut() {
        mirrored.sync(source, &wanted, app, window);
    }
}

/// Whether `copy` is what [`mirror`] makes of `declared`: the same fields,
/// a text or a predicate the same value or the same signal. A text or a
/// predicate derived from signals compares unequal, so its copy is made
/// again, as it always was.
fn same_copy(copy: &Shortcut, declared: &Shortcut) -> bool {
    fn same_prop<T: Clone + PartialEq + 'static>(a: &Prop<T>, b: &Prop<T>) -> bool {
        match (a, b) {
            (Prop::Static(a), Prop::Static(b)) => a == b,
            (Prop::Bound(a), Prop::Bound(b)) => Signal::same(a, b),
            _ => false,
        }
    }
    fn same_option<T: Clone + PartialEq + 'static>(
        a: &Option<Prop<T>>,
        b: &Option<Prop<T>>,
    ) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => same_prop(a, b),
            _ => false,
        }
    }
    copy.id == declared.id
        && same_prop(&copy.name, &declared.name)
        && copy.category == declared.category
        && same_option(&copy.category_label, &declared.category_label)
        && same_option(&copy.description, &declared.description)
        && copy.primary == declared.primary
        && copy.secondary == declared.secondary
        && copy.intent == Some(declared.intent_name())
        && copy.on_activate.is_none()
        && copy.scope == ShortcutScope::Global
        && copy.propagate_when_disabled == declared.propagate_when_disabled
        && same_option(&copy.enabled_when, &declared.enabled_when)
        && copy.literal_modifiers == declared.literal_modifiers
}

/// A window's copy of `declared`: the same id, name, chords, category and
/// enabled predicate, global, sending its intent by name. A closure built
/// with `ShortcutBuilder::on_activate` stays in the application tree; the
/// copy sends the plain intent it would be named after.
fn mirror(declared: &Shortcut) -> Shortcut {
    Shortcut {
        id: declared.id,
        name: declared.name.clone(),
        category: declared.category,
        category_label: declared.category_label.clone(),
        description: declared.description.clone(),
        primary: declared.primary,
        secondary: declared.secondary,
        intent: Some(declared.intent_name()),
        on_activate: None,
        scope: ShortcutScope::Global,
        propagate_when_disabled: declared.propagate_when_disabled,
        enabled_when: declared.enabled_when.clone(),
        literal_modifiers: declared.literal_modifiers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::event::Key;
    use teksilo_core::shortcut::KeyStroke;

    fn declared(id: &'static str, key: Key) -> Shortcut {
        Shortcut::new(id).primary(KeyStroke::ctrl(key)).build()
    }

    fn app() -> ShortcutRegistry {
        let mut app = ShortcutRegistry::new();
        app.register(declared("app.new", Key::N));
        app.register(declared("app.open", Key::O));
        app
    }

    #[test]
    fn a_window_gets_a_copy_of_each_wanted_shortcut_the_application_declares() {
        let app = app();
        let mut window = ShortcutRegistry::new();
        let mut mirrored = MirroredShortcuts::default();
        mirrored.sync(
            (1, 0, 0),
            || vec!["app.new", "app.missing"],
            &app,
            &mut window,
        );
        assert_eq!(mirrored.ids(), ["app.new"]);
        let copy = window.get_default("app.new").expect("copied");
        assert_eq!(copy.primary, Some(KeyStroke::ctrl(Key::N)));
        assert_eq!(copy.scope, ShortcutScope::Global);
        assert!(window.get_default("app.open").is_none(), "not wanted");
    }

    #[test]
    fn a_shortcut_the_window_registers_itself_is_left_alone() {
        let app = app();
        let mut window = ShortcutRegistry::new();
        window.register_owned(
            declared("app.new", Key::M),
            teksilo_core::WidgetId::default(),
        );
        let mut mirrored = MirroredShortcuts::default();
        mirrored.sync((1, 0, 0), || vec!["app.new"], &app, &mut window);
        assert!(mirrored.ids().is_empty());
        assert_eq!(
            window.get_default("app.new").unwrap().primary,
            Some(KeyStroke::ctrl(Key::M))
        );
    }

    /// A window's shared registration (an editor command every editor of
    /// the window declares) is the window's, like an owned one: a copy made
    /// before it stops being one, and is never registered over it again.
    /// The copy used to be re-registered, which drops the shared owners.
    #[test]
    fn a_shared_registration_of_the_window_is_its_own() {
        let app = app();
        let mut window = ShortcutRegistry::new();
        let mut mirrored = MirroredShortcuts::default();
        mirrored.sync((1, 0, 0), || vec!["app.new"], &app, &mut window);
        assert_eq!(mirrored.ids(), ["app.new"]);
        let editor = teksilo_core::WidgetId::default();
        window.register_shared(declared("app.new", Key::M), editor);
        mirrored.sync((1, 0, 0), || vec!["app.new"], &app, &mut window);
        assert!(mirrored.ids().is_empty(), "no longer a copy");
        assert_eq!(
            window.get_default("app.new").unwrap().primary,
            Some(KeyStroke::ctrl(Key::M)),
            "the editor's declaration stands"
        );
        window.unregister_all_for_owner(editor);
        assert!(
            window.get_default("app.new").is_none(),
            "and is still the editor's: it goes with its last owner"
        );

        // Registered shared before any sync: never copied over.
        let mut window = ShortcutRegistry::new();
        window.register_shared(declared("app.new", Key::M), editor);
        let mut mirrored = MirroredShortcuts::default();
        mirrored.sync((1, 0, 0), || vec!["app.new"], &app, &mut window);
        assert!(mirrored.ids().is_empty());
    }

    /// A user's rebind of a copy, made in one window's `ShortcutSettings`,
    /// applies to the application: it goes to the application tree's
    /// registry and every window's copy follows. A reset in another window
    /// brings them all back. A rebind used to stay in the window it was
    /// made in.
    #[test]
    fn a_rebind_of_a_copy_applies_to_the_application() {
        use teksilo_core::shortcut::SlotOverride;
        let mut app = app();
        let (mut one, mut two) = (ShortcutRegistry::new(), ShortcutRegistry::new());
        let (mut one_copies, mut two_copies) =
            (MirroredShortcuts::default(), MirroredShortcuts::default());
        let wanted = || vec!["app.new", "app.open"];
        let sync = |app: &mut ShortcutRegistry,
                    one: &mut ShortcutRegistry,
                    one_copies: &mut MirroredShortcuts,
                    two: &mut ShortcutRegistry,
                    two_copies: &mut MirroredShortcuts| {
            sync_all(
                (1, 0),
                wanted,
                app,
                &mut [(one_copies, one), (two_copies, two)],
            );
        };
        let primary = |registry: &ShortcutRegistry| {
            registry
                .effective("app.new")
                .and_then(|shortcut| shortcut.primary)
        };
        sync(
            &mut app,
            &mut one,
            &mut one_copies,
            &mut two,
            &mut two_copies,
        );

        one.rebind_primary("app.new", Some(KeyStroke::ctrl(Key::M)));
        sync(
            &mut app,
            &mut one,
            &mut one_copies,
            &mut two,
            &mut two_copies,
        );
        assert_eq!(
            app.override_for("app.new").map(|o| o.primary),
            Some(SlotOverride::Bound(KeyStroke::ctrl(Key::M))),
            "the application's registry holds the rebind"
        );
        assert_eq!(primary(&app), Some(KeyStroke::ctrl(Key::M)));
        assert_eq!(primary(&one), Some(KeyStroke::ctrl(Key::M)));
        assert_eq!(
            primary(&two),
            Some(KeyStroke::ctrl(Key::M)),
            "and every window"
        );
        assert!(
            app.override_for("app.open").is_none(),
            "only the rebound id"
        );

        two.clear_override("app.new");
        sync(
            &mut app,
            &mut one,
            &mut one_copies,
            &mut two,
            &mut two_copies,
        );
        assert!(app.override_for("app.new").is_none(), "a reset applies too");
        assert_eq!(primary(&one), Some(KeyStroke::ctrl(Key::N)));
        assert_eq!(primary(&two), Some(KeyStroke::ctrl(Key::N)));
    }

    /// An override the application tree's registry holds (one an app loaded
    /// at startup with `import_overrides`) reaches every window's copy, and
    /// a copy that goes takes the override it was given with it.
    #[test]
    fn the_applications_override_reaches_each_copy_and_leaves_with_it() {
        let mut app = app();
        app.rebind_primary("app.new", Some(KeyStroke::ctrl(Key::M)));
        let mut window = ShortcutRegistry::new();
        let mut mirrored = MirroredShortcuts::default();
        sync_all(
            (1, 0),
            || vec!["app.new"],
            &mut app,
            &mut [(&mut mirrored, &mut window)],
        );
        assert_eq!(
            window.effective("app.new").and_then(|s| s.primary),
            Some(KeyStroke::ctrl(Key::M))
        );
        sync_all(
            (2, 0),
            Vec::new,
            &mut app,
            &mut [(&mut mirrored, &mut window)],
        );
        assert!(window.get_default("app.new").is_none());
        assert!(
            window.override_for("app.new").is_none(),
            "no override is left behind for a registration of the window's own"
        );
        assert!(
            app.override_for("app.new").is_some(),
            "the application keeps it"
        );
    }

    /// A rebind of the window's own registration of an id stays the
    /// window's: only a copy's goes to the application.
    #[test]
    fn a_rebind_of_the_windows_own_shortcut_stays_in_the_window() {
        let mut app = app();
        let mut window = ShortcutRegistry::new();
        window.register_owned(
            declared("app.new", Key::M),
            teksilo_core::WidgetId::default(),
        );
        let mut mirrored = MirroredShortcuts::default();
        sync_all(
            (1, 0),
            || vec!["app.new"],
            &mut app,
            &mut [(&mut mirrored, &mut window)],
        );
        window.rebind_primary("app.new", Some(KeyStroke::ctrl(Key::J)));
        sync_all(
            (1, 0),
            || vec!["app.new"],
            &mut app,
            &mut [(&mut mirrored, &mut window)],
        );
        assert!(app.override_for("app.new").is_none());
        assert_eq!(
            window.effective("app.new").and_then(|s| s.primary),
            Some(KeyStroke::ctrl(Key::J))
        );
    }

    /// A sync that finds every copy as it is registers nothing: the
    /// window's registry version does not move, so nothing that follows it
    /// (a settings UI, the menu rows' chords) churns. Every copy used to be
    /// registered again whenever anything in the application tree's
    /// registry changed.
    #[test]
    fn an_unchanged_copy_is_not_registered_again() {
        let mut app = app();
        let mut window = ShortcutRegistry::new();
        let mut mirrored = MirroredShortcuts::default();
        let wanted = || vec!["app.new", "app.open"];
        sync_all(
            (1, 0),
            wanted,
            &mut app,
            &mut [(&mut mirrored, &mut window)],
        );
        let version = window.version().get();
        // Something else changes in the application tree's registry.
        app.register(declared("app.print", Key::P));
        sync_all(
            (1, 0),
            wanted,
            &mut app,
            &mut [(&mut mirrored, &mut window)],
        );
        assert_eq!(window.version().get(), version, "no copy registered again");
        // A declaration that did change is copied again.
        app.register(declared("app.new", Key::J));
        sync_all(
            (1, 0),
            wanted,
            &mut app,
            &mut [(&mut mirrored, &mut window)],
        );
        assert_eq!(
            window.get_default("app.new").unwrap().primary,
            Some(KeyStroke::ctrl(Key::J))
        );
    }

    #[test]
    fn a_copy_no_longer_wanted_goes() {
        let app = app();
        let mut window = ShortcutRegistry::new();
        let mut mirrored = MirroredShortcuts::default();
        mirrored.sync((1, 0, 0), || vec!["app.new"], &app, &mut window);
        mirrored.sync((2, 0, 0), || vec!["app.open"], &app, &mut window);
        assert_eq!(mirrored.ids(), ["app.open"]);
        assert!(window.get_default("app.new").is_none());
        assert!(window.get_default("app.open").is_some());
    }
}
