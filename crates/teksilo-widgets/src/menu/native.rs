// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Bridge from a [`MenuModel`] to the platform native menu (`teksilo-platform`'s
//! [`NativeMenuHandle`]).
//!
//! Resolves the model into a plain [`NativeMenuSnapshot`] (titles localized +
//! mnemonic-stripped, shortcuts resolved to key equivalents), installs it for
//! the current window, and wires reactive `Signal`s so a toggled check, a
//! disabled item or a hidden one updates the native item in place.

use std::collections::HashMap;

use teksilo_core::MenuItemId;
use teksilo_core::ObserverHandle;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::signal::{Prop, Signal};
use teksilo_data::CheckState;
use teksilo_i18n::LocalizedString;
use teksilo_platform::native_menu::{
    MenuItemDelta, NativeCheck, NativeKeyEquivalent, NativeMenuActivation, NativeMenuHandle,
    NativeMenuNode, NativeMenuSnapshot, StandardMenuRole, StandardRoutedItem,
};

use crate::menu_item::parse_mnemonic;

use super::model::{MenuItemState, MenuModel, MenuNode, StandardMenu};

/// How a [`MenuBar`](crate::menu_bar::MenuBar) built from a [`MenuModel`]
/// behaves on macOS, where the convention is a global menu bar at the top of the
/// screen rather than an in-window strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NativeMenuMode {
    /// Don't touch the native menu bar; render the in-window bar only. The
    /// default — opt in with [`MenuBar::native_on_macos`](crate::menu_bar::MenuBar::native_on_macos).
    #[default]
    Off,
    /// Mirror the model into the OS menu bar AND suppress the in-window bar on
    /// macOS (the native-looking choice). On other platforms the in-window bar
    /// still renders (the native backend is a no-op there).
    Suppress,
    /// Mirror into the OS menu bar AND keep the in-window bar visible too.
    Coexist,
}

impl NativeMenuMode {
    /// Whether the in-window bar should be suppressed for the current target.
    pub(crate) fn suppresses_in_window(self) -> bool {
        cfg!(target_os = "macos") && matches!(self, NativeMenuMode::Suppress)
    }

    /// Whether the native menu should be installed at all.
    pub(crate) fn installs_native(self) -> bool {
        !matches!(self, NativeMenuMode::Off)
    }
}

/// RAII binding that keeps the model's reactive observers alive for as long as
/// the [`MenuBar`](crate::menu_bar::MenuBar) is mounted. Dropping it stops the
/// per-item updates (the native menu itself is torn down when the window closes
/// or its menu is replaced).
pub(crate) struct NativeMenuBinding {
    _observers: Vec<ObserverHandle>,
}

/// Resolve `model` into a native menu, install it for the current window, and
/// wire reactive updates. Returns `None` (no-op) when there is no
/// [`NativeMenuHandle`] in app-state, or no window / poster — e.g. in headless
/// tests, or when the app did not call `install_native_menu()`.
pub(crate) fn install(model: &MenuModel, ctx: &BuildContext) -> Option<NativeMenuBinding> {
    let handle = ctx.app_state::<NativeMenuHandle>()?.clone();
    let window_id = ctx.window()?.id();
    let poster = ctx.poster()?.clone();

    let mut activations = HashMap::new();
    let mut reactive = Vec::new();
    let mut roots: Vec<NativeMenuNode> = {
        let nodes = model.nodes();
        nodes
            .iter()
            .filter_map(|n| resolve_node(n, ctx, &mut activations, &mut reactive))
            .collect()
    };
    // macOS requires a leading application menu. If the model didn't declare one,
    // inject a default (English `lit!` labels; the app overrides via
    // `MenuModel::standard_menu(StandardMenu::app()...)`). Resolving here keeps
    // every user-visible string in the i18n layer.
    let has_app = roots.iter().any(|n| {
        matches!(
            n,
            NativeMenuNode::Standard {
                role: StandardMenuRole::App,
                ..
            }
        )
    });
    if !has_app {
        roots.insert(
            0,
            NativeMenuNode::Standard {
                role: StandardMenuRole::App,
                labels: StandardMenu::app().resolve_labels(),
                // Deliberately unrouted: a model that declares no App menu has
                // declared no quit handler either, so `terminate:` is the only
                // thing that can still make ⌘Q work here.
                quit_item: None,
                // Likewise no Settings row: with no App menu declared there is
                // no intent to route it to, and an unrouted one would do
                // nothing.
                settings_item: None,
            },
        );
    }
    let snapshot = NativeMenuSnapshot { roots };

    // The App menu is also what the app keeps once its last window has
    // closed (an app that stays open without a window), with its routed
    // rows reaching the application tree.
    let (app_menu, app_activations) = app_menu_of(&snapshot, &activations);
    handle.set_window_menu(window_id, snapshot, activations, poster.clone());
    handle.set_app_menu(app_menu, app_activations, poster);

    // Wire reactive per-item updates (title / enabled / visible / check /
    // radio).
    let mut observers = Vec::new();
    for item in reactive {
        // The title first, and by the same delta mechanism as the rest: a menu
        // whose Undo row names its target has to say the same thing in the
        // global bar as in the window, and re-installing the whole native menu
        // to change one string would be both heavy and visibly flickery.
        {
            let sig = item.title.to_signal();
            let h = handle.clone();
            let id = item.id;
            push_observer(&mut observers, &sig, move |v| {
                h.update_item(
                    id,
                    MenuItemDelta {
                        title: Some(strip_title(v)),
                        ..Default::default()
                    },
                );
            });
        }
        if let Prop::Bound(sig) = item.enabled {
            let h = handle.clone();
            let id = item.id;
            push_observer(&mut observers, &sig, move |v| {
                h.update_item(
                    id,
                    MenuItemDelta {
                        enabled: Some(*v),
                        ..Default::default()
                    },
                );
            });
        }
        if let Prop::Bound(sig) = item.visible {
            let h = handle.clone();
            let id = item.id;
            push_observer(&mut observers, &sig, move |v| {
                h.update_item(
                    id,
                    MenuItemDelta {
                        visible: Some(*v),
                        ..Default::default()
                    },
                );
            });
        }
        match item.state {
            MenuItemState::Plain => {}
            // Two-way and reflect-only both mirror the signal into the native
            // checkmark; they differ only in what choosing the item writes
            // (`MenuEntry::native_activation`).
            MenuItemState::Check(sig) | MenuItemState::ReflectCheck(sig) => {
                let h = handle.clone();
                let id = item.id;
                push_observer(&mut observers, &sig, move |v| {
                    h.update_item(
                        id,
                        check_delta(if *v {
                            NativeCheck::On
                        } else {
                            NativeCheck::Off
                        }),
                    );
                });
            }
            MenuItemState::TriCheck(sig) => {
                let h = handle.clone();
                let id = item.id;
                push_observer(&mut observers, &sig, move |v| {
                    h.update_item(id, check_delta(tri_to_native(*v)));
                });
            }
            // Two-way and reflect-only likewise differ only in what choosing
            // the item writes (`MenuEntry::native_activation`).
            MenuItemState::Radio { value, selected }
            | MenuItemState::ReflectRadio { value, selected } => {
                let h = handle.clone();
                let id = item.id;
                push_observer(&mut observers, &selected, move |sel| {
                    let check = if *sel == value {
                        NativeCheck::On
                    } else {
                        NativeCheck::Off
                    };
                    h.update_item(id, check_delta(check));
                });
            }
        }
    }

    Some(NativeMenuBinding {
        _observers: observers,
    })
}

/// Attach one live-update observer, or leave the row at the value already
/// baked into the native snapshot.
///
/// Fallible on purpose. This bridge runs inside `applicationDidFinishLaunching`
/// on macOS, an Objective-C frame a Rust panic cannot unwind through: a panic
/// here does not surface as an error, it aborts the process before the first
/// window is drawn. `Signal::try_observe` covers every signal with fixed
/// mutable roots — a plain one, and anything derived from them with `map` /
/// `zip` / `and` / `not`, which is what a caller building
/// `enabled(unsaved.and(&backup_mode.not()))` gets. The remaining shape is
/// `flat_map`, whose roots are re-chosen as it is read; for that the row keeps
/// the state it was resolved with and simply does not follow later changes,
/// in the global bar only. The in-window menu binds it directly and stays live
/// either way.
/// The application menu a window's menu leaves behind: its App menu alone,
/// with the activations of the rows the App menu routes (Quit, Settings).
fn app_menu_of(
    snapshot: &NativeMenuSnapshot,
    activations: &HashMap<MenuItemId, NativeMenuActivation>,
) -> (
    NativeMenuSnapshot,
    HashMap<MenuItemId, NativeMenuActivation>,
) {
    let app = snapshot.roots.iter().find(|node| {
        matches!(
            node,
            NativeMenuNode::Standard {
                role: StandardMenuRole::App,
                ..
            }
        )
    });
    let mut app_activations = HashMap::new();
    if let Some(NativeMenuNode::Standard {
        quit_item,
        settings_item,
        ..
    }) = app
    {
        for item in quit_item.iter().chain(settings_item) {
            if let Some(activation) = activations.get(&item.id) {
                app_activations.insert(item.id, activation.clone());
            }
        }
    }
    let roots = app.cloned().into_iter().collect();
    (NativeMenuSnapshot { roots }, app_activations)
}

fn push_observer<T: 'static>(
    observers: &mut Vec<ObserverHandle>,
    signal: &Signal<T>,
    f: impl Fn(&T) + 'static,
) {
    if let Ok(handle) = signal.try_observe(f) {
        observers.push(handle);
    }
}

/// One item's reactive sources, gathered during resolution.
struct ReactiveItem {
    id: MenuItemId,
    enabled: Prop<bool>,
    visible: Prop<bool>,
    state: MenuItemState,
    /// The entry's label, kept so a title that depends on application state —
    /// "Undo renaming «Chapter 3»" — reaches the native bar too. Almost every
    /// title only ever changes with the locale, and a locale change rebuilds
    /// the whole menu, so for those this observation simply never fires.
    title: LocalizedString,
}

fn resolve_node(
    node: &MenuNode,
    ctx: &BuildContext,
    activations: &mut HashMap<MenuItemId, NativeMenuActivation>,
    reactive: &mut Vec<ReactiveItem>,
) -> Option<NativeMenuNode> {
    match node {
        MenuNode::Separator => Some(NativeMenuNode::Separator),
        MenuNode::Standard(sm) => Some(resolve_standard(sm, activations, |id| {
            ctx.effective_shortcut(id).and_then(|eff| eff.primary)
        })),
        MenuNode::Submenu {
            title, children, ..
        } => Some(NativeMenuNode::Submenu {
            title: strip_title(&title.resolve_now()),
            children: children
                .iter()
                .filter_map(|n| resolve_node(n, ctx, activations, reactive))
                .collect(),
        }),
        // A hidden item is installed hidden rather than left out, so its
        // `visible` signal can show it again in place.
        MenuNode::Item(entry) => {
            let check = match &entry.state {
                MenuItemState::Plain => NativeCheck::None,
                MenuItemState::Check(s) | MenuItemState::ReflectCheck(s) => {
                    if s.get() {
                        NativeCheck::On
                    } else {
                        NativeCheck::Off
                    }
                }
                MenuItemState::TriCheck(s) => tri_to_native(s.get()),
                MenuItemState::Radio { value, selected }
                | MenuItemState::ReflectRadio { value, selected } => {
                    if selected.get() == *value {
                        NativeCheck::On
                    } else {
                        NativeCheck::Off
                    }
                }
            };
            let key_equiv = entry
                .shortcut_id
                .and_then(|id| ctx.effective_shortcut(id).and_then(|eff| eff.primary))
                .map(native_key_equiv);

            activations.insert(entry.id, entry.native_activation());
            reactive.push(ReactiveItem {
                id: entry.id,
                enabled: entry.enabled.clone(),
                visible: entry.visible.clone(),
                state: entry.state.clone(),
                title: entry.title.clone(),
            });

            Some(NativeMenuNode::Item {
                id: entry.id,
                title: strip_title(&entry.title.resolve_now()),
                key_equiv,
                enabled: entry.enabled.get(),
                visible: entry.visible.get(),
                check,
            })
        }
    }
}

/// The conventional chord for a routed standard row when the app named no
/// shortcut of its own — ⌘Q for Quit, ⌘, for Settings, which is what a Mac user
/// reaches for whatever the app calls the command.
///
/// A fallback, never an override: an app that registers a quit shortcut should
/// name it (see [`StandardMenu::quit_shortcut`]) so the row follows a rebind.
fn conventional_chord(key: &str) -> NativeKeyEquivalent {
    NativeKeyEquivalent {
        key: key.to_string(),
        command: true,
        shift: false,
        alt: false,
        control: false,
    }
}

/// Resolve one platform-standard menu into its boundary node.
///
/// Split out of [`resolve_node`] because it needs no [`BuildContext`], only a
/// way to look a shortcut up: a standard menu carries labels and, optionally,
/// routed Quit / Settings rows, and the platform fills in the rest. That makes
/// it the one part of the native bridge a test can exercise on any OS —
/// everything around it is behind the macOS gate in `MenuBar::build`, so a
/// routing bug would otherwise only be observable on the platform it breaks.
///
/// `shortcut` is the registry lookup, threaded rather than reached for so a test
/// can hand over a stub: the chords these rows advertise are otherwise the one
/// thing about them nothing off macOS can check.
fn resolve_standard(
    sm: &StandardMenu,
    activations: &mut HashMap<MenuItemId, NativeMenuActivation>,
    shortcut: impl Fn(&str) -> Option<KeyStroke>,
) -> NativeMenuNode {
    // A routed row is an ordinary activation under an id the model minted once,
    // so it survives every rebuild — unlike the rest of a standard menu, which
    // the platform fills in from labels alone.
    let mut route = |entry: Option<(&'static str, MenuItemId)>,
                     shortcut_id: Option<&'static str>,
                     fallback: &str|
     -> Option<StandardRoutedItem> {
        let (intent, id) = entry?;
        activations.insert(
            id,
            NativeMenuActivation {
                intent: Some(intent),
                action: None,
            },
        );
        // The registry's answer, resolved through the primary-accelerator
        // convention exactly as `MenuEntry`'s chord is — so this row cannot
        // advertise one chord while the dispatcher fires another. An id that
        // resolves to nothing (unregistered, or unbound by the user) leaves the
        // row with no key equivalent rather than resurrecting the convention:
        // the app said where the chord comes from, and it currently says none.
        let key_equiv = match shortcut_id {
            Some(sid) => shortcut(sid).map(native_key_equiv),
            None => Some(conventional_chord(fallback)),
        };
        Some(StandardRoutedItem { id, key_equiv })
    };

    let quit_item = route(sm.quit_route(), sm.quit_shortcut_id(), "q");
    // Settings is routed the same way, and only ever routed — the platform has
    // no selector of its own to fall back on.
    let settings_item = route(sm.settings_route(), sm.settings_shortcut_id(), ",");

    NativeMenuNode::Standard {
        role: sm.role(),
        labels: sm.resolve_labels(),
        quit_item,
        settings_item,
    }
}

fn check_delta(check: NativeCheck) -> MenuItemDelta {
    MenuItemDelta {
        check: Some(check),
        ..Default::default()
    }
}

fn tri_to_native(state: CheckState) -> NativeCheck {
    match state {
        CheckState::Checked => NativeCheck::On,
        CheckState::Unchecked => NativeCheck::Off,
        CheckState::Indeterminate => NativeCheck::Mixed,
    }
}

fn strip_title(raw: &str) -> String {
    parse_mnemonic(raw).stripped
}

/// Map a Teksilo [`KeyStroke`] to a platform key equivalent.
///
/// The chord arrives already resolved by the registry, which has applied the
/// primary-accelerator convention (Qt's `Qt::CTRL` → ⌘) to the declared
/// default: an app that writes `KeyStroke::ctrl(Key::S)` gets ⌘S here.
///
/// So the Command flag takes the accelerator, and any **leftover** literal
/// `Ctrl` goes to Control rather than being folded into Command — otherwise a
/// deliberately literal ⌃ chord (a user's own rebind, or a `literal_modifiers`
/// Ctrl+Tab) would be advertised on the wrong key. `Super` maps to Command
/// unconditionally: [`NativeKeyEquivalent`] has no Super flag, and on the one
/// backend that consumes this today ⌘ *is* Super.
fn native_key_equiv(ks: KeyStroke) -> NativeKeyEquivalent {
    NativeKeyEquivalent {
        key: key_to_equiv(ks.key),
        command: ks.modifiers.command() || ks.modifiers.super_key(),
        shift: ks.modifiers.shift(),
        alt: ks.modifiers.alt(),
        control: ks.modifiers.without(Modifiers::COMMAND).ctrl(),
    }
}

fn key_to_equiv(key: Key) -> String {
    let special = match key {
        Key::Enter => "\r",
        Key::Tab => "\t",
        Key::Space => " ",
        Key::Escape => "\u{1b}",
        Key::Backspace => "\u{8}",
        Key::Delete => "\u{7f}",
        Key::ArrowUp => "\u{F700}",
        Key::ArrowDown => "\u{F701}",
        Key::ArrowLeft => "\u{F702}",
        Key::ArrowRight => "\u{F703}",
        Key::Home => "\u{F729}",
        Key::End => "\u{F72B}",
        Key::PageUp => "\u{F72C}",
        Key::PageDown => "\u{F72D}",
        Key::F1 => "\u{F704}",
        Key::F2 => "\u{F705}",
        Key::F3 => "\u{F706}",
        Key::F4 => "\u{F707}",
        Key::F5 => "\u{F708}",
        Key::F6 => "\u{F709}",
        Key::F7 => "\u{F70A}",
        Key::F8 => "\u{F70B}",
        Key::F9 => "\u{F70C}",
        Key::F10 => "\u{F70D}",
        Key::F11 => "\u{F70E}",
        Key::F12 => "\u{F70F}",
        // Letters / digits / arbitrary chars: lowercase single character.
        other => return other.to_char().map(|c| c.to_string()).unwrap_or_default(),
    };
    special.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_i18n::LocalizedString;

    fn labels_of(node: &NativeMenuNode) -> &teksilo_platform::native_menu::StandardLabels {
        match node {
            NativeMenuNode::Standard { labels, .. } => labels,
            _ => panic!("expected a standard menu node"),
        }
    }

    fn quit_of(node: &NativeMenuNode) -> Option<&StandardRoutedItem> {
        match node {
            NativeMenuNode::Standard { quit_item, .. } => quit_item.as_ref(),
            _ => panic!("expected a standard menu node"),
        }
    }

    fn settings_of(node: &NativeMenuNode) -> Option<&StandardRoutedItem> {
        match node {
            NativeMenuNode::Standard { settings_item, .. } => settings_item.as_ref(),
            _ => panic!("expected a standard menu node"),
        }
    }

    fn quit_item_of(node: &NativeMenuNode) -> Option<MenuItemId> {
        quit_of(node).map(|r| r.id)
    }

    fn settings_item_of(node: &NativeMenuNode) -> Option<MenuItemId> {
        settings_of(node).map(|r| r.id)
    }

    /// An app that registered no shortcuts at all.
    fn no_shortcuts(_: &str) -> Option<KeyStroke> {
        None
    }

    /// A registry holding exactly one chord, under `id`.
    fn only(id: &'static str, ks: KeyStroke) -> impl Fn(&str) -> Option<KeyStroke> {
        move |asked| (asked == id).then_some(ks)
    }

    /// A chord as the platform would advertise it: `(key, command, shift)`.
    fn chord(item: Option<&StandardRoutedItem>) -> Option<(String, bool, bool)> {
        item?
            .key_equiv
            .as_ref()
            .map(|k| (k.key.clone(), k.command, k.shift))
    }

    /// Installs a model into the native menu from its `build`, as
    /// `MenuBar` does on macOS, and keeps the binding alive. Calling
    /// [`install`] directly is what lets this run on every OS: `MenuBar` only
    /// calls it on macOS.
    struct Installer {
        model: MenuModel,
        binding: std::cell::RefCell<Option<NativeMenuBinding>>,
    }

    impl std::fmt::Debug for Installer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Installer")
        }
    }

    impl teksilo_core::widget::Widget for Installer {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<teksilo_core::widget_id::WidgetId> {
            *self.binding.borrow_mut() = install(&self.model, ctx);
            Vec::new()
        }

        fn layout_response(
            &self,
            proposal: teksilo_canvas::SizeProposal,
            _ctx: &teksilo_core::widget::LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    /// Install `model` for window 1 through a recording backend, and return
    /// the backend and the handle, with the tree that keeps the installation
    /// alive.
    fn install_recorded(
        model: MenuModel,
    ) -> (
        teksilo_platform::native_menu::MemoryNativeMenuBackend,
        NativeMenuHandle,
        teksilo_core::widget_tree::WidgetTree,
    ) {
        use std::any::{Any, TypeId};
        use std::sync::Arc;
        use teksilo_core::AppEventPoster;
        use teksilo_core::window::state::WindowStateInit;
        use teksilo_core::window::{TeksiloWindowId, WindowPlacement, WindowState};

        struct NullPoster;
        impl AppEventPoster for NullPoster {
            fn post_subscription_event(
                &self,
                _: teksilo_core::SubscriptionId,
                _: Box<dyn Any + Send>,
            ) {
            }
            fn post_external(&self, _: Box<dyn Any + Send>) {}
        }

        let backend = teksilo_platform::native_menu::MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        let mut app_state: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
        app_state.insert(TypeId::of::<NativeMenuHandle>(), Box::new(handle.clone()));
        let poster: Arc<dyn AppEventPoster> = Arc::new(NullPoster);
        let mut tree = teksilo_core::widget_tree::WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light());
        tree.set_window_state(WindowState::new(WindowStateInit {
            id: TeksiloWindowId::new(1),
            string_id: None,
            placement: WindowPlacement::Floating,
            title: String::new(),
            size: (800, 600),
            position: (0, 0),
            focused: true,
            resizable: true,
            always_on_top: false,
        }));
        tree.set_app_context(std::rc::Rc::new(
            teksilo_core::event_source::TreeAppContext::empty()
                .with_app_state(app_state)
                .with_poster(poster),
        ));
        tree.add(Installer {
            model,
            binding: Default::default(),
        });
        tree.layout(teksilo_canvas::SizeProposal::exact(800.0, 600.0));
        (backend, handle, tree)
    }

    /// The App menu is what the app keeps once its last window has closed:
    /// the window's App menu alone, wherever the model declared it, its
    /// routed rows still routed and nothing else. With none, ⌘Q went dead
    /// in an app left open with no window.
    #[test]
    fn the_app_menu_is_the_windows_app_menu_alone_with_its_routed_rows() {
        use crate::menu::{MenuEntry, StandardMenu};
        use teksilo_i18n::lit;

        let model = MenuModel::new()
            .menu(lit!("File"), |m| {
                m.item(MenuEntry::new(lit!("New")).intent("app.new"))
            })
            .standard_menu(
                StandardMenu::app()
                    .quit_intent("app.quit")
                    .settings_intent("app.settings"),
            );
        let (backend, handle, _tree) = install_recorded(model);

        let window = backend
            .menu_for(teksilo_core::window::TeksiloWindowId::new(1))
            .expect("the window's menu");
        let app = backend.app_menu().expect("an app menu is set");
        assert_eq!(app.roots.len(), 1, "the App menu and nothing else");
        let app_node = window
            .roots
            .iter()
            .find(|node| {
                matches!(
                    node,
                    NativeMenuNode::Standard {
                        role: StandardMenuRole::App,
                        ..
                    }
                )
            })
            .expect("the window's App menu");
        let quit = quit_item_of(app_node).expect("quit id");
        let settings = settings_item_of(app_node).expect("settings id");
        assert_eq!(quit_item_of(&app.roots[0]), Some(quit));
        assert_eq!(settings_item_of(&app.roots[0]), Some(settings));

        assert_eq!(handle.app_quit_item(), Some(quit));
        let intent = |id| handle.app_activation(id).and_then(|a| a.intent);
        assert_eq!(intent(quit), Some("app.quit"));
        assert_eq!(intent(settings), Some("app.settings"));
        let (new, _, _) = items_of(&window, "File")[0].clone();
        assert!(
            handle.app_activation(new).is_none(),
            "a window's own rows are not in the app menu"
        );
    }

    /// A model with no App menu gets the injected one in the app menu too,
    /// with Quit left to the platform.
    #[test]
    fn an_app_menu_from_a_model_without_one_leaves_quit_to_the_platform() {
        use crate::menu::MenuEntry;
        use teksilo_i18n::lit;

        let model = MenuModel::new().menu(lit!("File"), |m| {
            m.item(MenuEntry::new(lit!("New")).intent("app.new"))
        });
        let (backend, handle, _tree) = install_recorded(model);
        let app = backend.app_menu().expect("an app menu is set");
        assert_eq!(app.roots.len(), 1);
        assert!(matches!(
            app.roots[0],
            NativeMenuNode::Standard {
                role: StandardMenuRole::App,
                quit_item: None,
                ..
            }
        ));
        assert_eq!(handle.app_quit_item(), None);
    }

    /// The first menu named `menu`'s items, as `(id, title, visible)`.
    fn items_of(snapshot: &NativeMenuSnapshot, menu: &str) -> Vec<(MenuItemId, String, bool)> {
        snapshot
            .roots
            .iter()
            .find_map(|node| match node {
                NativeMenuNode::Submenu { title, children } if title == menu => Some(children),
                _ => None,
            })
            .expect("the menu is in the snapshot")
            .iter()
            .filter_map(|node| match node {
                NativeMenuNode::Item {
                    id, title, visible, ..
                } => Some((*id, title.clone(), *visible)),
                _ => None,
            })
            .collect()
    }

    /// A hidden row is installed hidden, in its place, and its signal shows
    /// and hides it in the native menu with no rebuild. It used to be left
    /// out of the native menu and come back only when something rebuilt it.
    #[test]
    fn a_hidden_row_is_installed_hidden_and_follows_its_signal() {
        use crate::menu::MenuEntry;
        use teksilo_i18n::lit;

        let shown = Signal::new(false);
        let model = MenuModel::new().menu(lit!("File"), |m| {
            m.item(MenuEntry::new(lit!("New")))
                .item(MenuEntry::new(lit!("Recent")).visible(shown.clone()))
                .item(MenuEntry::new(lit!("Never")).visible(false))
        });
        let (backend, _, _tree) = install_recorded(model);

        let snapshot = backend
            .menu_for(teksilo_core::window::TeksiloWindowId::new(1))
            .expect("the menu was installed");
        let items = items_of(&snapshot, "File");
        let titles: Vec<(&str, bool)> = items
            .iter()
            .map(|(_, title, visible)| (title.as_str(), *visible))
            .collect();
        assert_eq!(
            titles,
            [("New", true), ("Recent", false), ("Never", false)],
            "hidden rows keep their place, hidden"
        );
        let recent = items[1].0;

        let visibility = |backend: &teksilo_platform::native_menu::MemoryNativeMenuBackend| {
            backend
                .deltas()
                .into_iter()
                .filter_map(|(id, delta)| delta.visible.map(|visible| (id, visible)))
                .collect::<Vec<_>>()
        };
        assert!(visibility(&backend).is_empty(), "nothing changed yet");
        shown.set(true);
        assert_eq!(visibility(&backend), [(recent, true)]);
        shown.set(false);
        assert_eq!(visibility(&backend), [(recent, true), (recent, false)]);
    }

    /// Settings has no `terminate:`-style fallback: no platform opens an
    /// arbitrary app's settings on its own. So an unset route must omit the row
    /// rather than render one that does nothing when chosen.
    #[test]
    fn a_standard_app_menu_has_no_settings_row_by_default() {
        let mut activations = HashMap::new();
        let node = resolve_standard(&StandardMenu::app(), &mut activations, no_shortcuts);
        assert_eq!(settings_item_of(&node), None);
    }

    /// A settings intent mints an id and routes it, exactly like a quit intent.
    #[test]
    fn a_settings_intent_becomes_a_routed_item_with_an_activation() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app().settings_intent("app.settings"),
            &mut activations,
            no_shortcuts,
        );
        let id = settings_item_of(&node).expect("a routed settings carries an item id");
        assert_eq!(
            activations.get(&id).map(|a| a.intent),
            Some(Some("app.settings"))
        );
    }

    /// Both slots on one App menu must get distinct ids, or choosing Settings
    /// would fire Quit.
    #[test]
    fn quit_and_settings_are_routed_under_distinct_ids() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app()
                .quit_intent("app.quit")
                .settings_intent("app.settings"),
            &mut activations,
            no_shortcuts,
        );
        let quit = quit_item_of(&node).expect("quit id");
        let settings = settings_item_of(&node).expect("settings id");
        assert_ne!(quit, settings);
        assert_eq!(activations.len(), 2);
        assert_eq!(activations[&quit].intent, Some("app.quit"));
        assert_eq!(activations[&settings].intent, Some("app.settings"));
    }

    /// Same stability guarantee as the quit id: minted with the model, so a
    /// later `update_item` delta still addresses a live menu item.
    #[test]
    fn the_routed_settings_id_is_stable_across_installs() {
        let menu = StandardMenu::app().settings_intent("app.settings");
        let mut first = HashMap::new();
        let mut second = HashMap::new();
        assert_eq!(
            settings_item_of(&resolve_standard(&menu, &mut first, no_shortcuts)),
            settings_item_of(&resolve_standard(&menu, &mut second, no_shortcuts)),
        );
    }

    /// The label rides the same i18n path as the rest of the App menu chrome,
    /// so the platform crate never sees an English literal it did not get from
    /// the widget layer.
    #[test]
    fn the_settings_label_resolves_through_the_widget_layer() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app().settings(LocalizedString::literal("Réglages…")),
            &mut activations,
            no_shortcuts,
        );
        assert_eq!(labels_of(&node).settings, "Réglages…");
    }

    /// The default is the platform's own Quit. An app that declared no handler
    /// still gets a working ⌘Q out of `terminate:`, and that guarantee is what
    /// the auto-injected App menu rests on.
    #[test]
    fn a_standard_app_menu_routes_nothing_by_default() {
        let mut activations = HashMap::new();
        let node = resolve_standard(&StandardMenu::app(), &mut activations, no_shortcuts);
        assert_eq!(quit_item_of(&node), None);
        assert!(
            activations.is_empty(),
            "an unrouted standard menu owns no activation"
        );
    }

    /// With a quit intent the item carries an id, and that id resolves to the
    /// intent — the whole point being that ⌘Q reaches the app instead of
    /// terminating past it.
    #[test]
    fn a_quit_intent_becomes_a_routed_item_with_an_activation() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app().quit_intent("app.quit"),
            &mut activations,
            no_shortcuts,
        );
        let id = quit_item_of(&node).expect("a routed quit carries an item id");
        let activation = activations
            .get(&id)
            .expect("the routed id resolves to an activation");
        assert_eq!(activation.intent, Some("app.quit"));
        assert!(
            activation.action.is_none(),
            "routing by name only — no closure to run on the side"
        );
    }

    /// The id is minted with the model, not with the snapshot. A fresh one per
    /// install would still route (the map is rebuilt alongside it), but any
    /// `update_item` delta held from an earlier build would address a menu item
    /// that no longer exists.
    #[test]
    fn the_routed_quit_id_is_stable_across_installs() {
        let menu = StandardMenu::app().quit_intent("app.quit");
        let mut first = HashMap::new();
        let mut second = HashMap::new();
        assert_eq!(
            quit_item_of(&resolve_standard(&menu, &mut first, no_shortcuts)),
            quit_item_of(&resolve_standard(&menu, &mut second, no_shortcuts)),
        );
    }

    /// Two App menus — one per window, as a multi-window app builds them — must
    /// not share an id, or the second window's activation map overwrites the
    /// first's and closing either window unroutes both.
    #[test]
    fn two_app_menus_get_distinct_routed_ids() {
        let mut activations = HashMap::new();
        let a = resolve_standard(
            &StandardMenu::app().quit_intent("app.quit"),
            &mut activations,
            no_shortcuts,
        );
        let b = resolve_standard(
            &StandardMenu::app().quit_intent("app.quit"),
            &mut activations,
            no_shortcuts,
        );
        assert_ne!(quit_item_of(&a), quit_item_of(&b));
        assert_eq!(activations.len(), 2);
    }

    /// Routing changes what Quit *does*, never what it says: the label still
    /// comes from the app's i18n layer, as every other standard label does.
    #[test]
    fn routing_leaves_the_localized_labels_alone() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app()
                .quit(LocalizedString::literal("Quitter"))
                .quit_intent("app.quit"),
            &mut activations,
            no_shortcuts,
        );
        assert_eq!(labels_of(&node).quit, "Quitter");
    }

    // ── The chord a routed row advertises ───────────────────────────────

    /// With no shortcut named, the row falls back to the chord a Mac user
    /// reaches for. This is the case every app gets without thinking about it,
    /// so it has to be the conventional one.
    #[test]
    fn an_unnamed_shortcut_falls_back_to_the_conventional_chord() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app()
                .quit_intent("app.quit")
                .settings_intent("app.settings"),
            &mut activations,
            no_shortcuts,
        );
        assert_eq!(chord(quit_of(&node)), Some(("q".into(), true, false)));
        assert_eq!(chord(settings_of(&node)), Some((",".into(), true, false)));
    }

    /// Named, the chord comes from the registry — which is the whole point.
    /// A `Ctrl` declaration has already been rewritten to the primary
    /// accelerator by the time it reaches here, so it arrives as ⌘.
    #[test]
    fn a_named_shortcut_supplies_the_chord() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app()
                .quit_intent("app.quit")
                .quit_shortcut("app.quit"),
            &mut activations,
            only("app.quit", KeyStroke::command(Key::Q)),
        );
        assert_eq!(chord(quit_of(&node)), Some(("q".into(), true, false)));
    }

    /// The case the fallback cannot serve: a user who rebound Quit. The row
    /// must advertise — and therefore fire — the new chord, not the old one.
    /// Left hardcoded, ⌘Q stays live after the user moved the command away from
    /// it, *and* shadows wherever they moved it to, since the platform
    /// dispatches a main-menu key equivalent before the responder chain.
    #[test]
    fn a_rebound_shortcut_moves_the_rows_chord_with_it() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app()
                .quit_intent("app.quit")
                .quit_shortcut("app.quit"),
            &mut activations,
            only("app.quit", KeyStroke::command_shift(Key::Q)),
        );
        assert_eq!(
            chord(quit_of(&node)),
            Some(("q".into(), true, true)),
            "the row follows the rebind rather than keeping the convention"
        );
    }

    /// Naming a shortcut that resolves to nothing — unregistered, or unbound by
    /// the user — leaves the row with no key equivalent. Falling back to the
    /// convention here would resurrect a chord the user deliberately cleared,
    /// which is the same defect as never having read the registry.
    #[test]
    fn a_named_but_unbound_shortcut_leaves_the_row_chordless() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app()
                .quit_intent("app.quit")
                .quit_shortcut("app.quit"),
            &mut activations,
            no_shortcuts,
        );
        assert!(quit_of(&node).is_some(), "the row is still there");
        assert_eq!(chord(quit_of(&node)), None, "it just has no chord");
    }

    /// The two rows read their own ids, not each other's.
    #[test]
    fn each_row_reads_its_own_shortcut() {
        let mut activations = HashMap::new();
        let node = resolve_standard(
            &StandardMenu::app()
                .quit_intent("app.quit")
                .quit_shortcut("app.quit")
                .settings_intent("app.settings")
                .settings_shortcut("app.settings"),
            &mut activations,
            only("app.settings", KeyStroke::command(Key::Character(','))),
        );
        assert_eq!(chord(quit_of(&node)), None, "quit's id resolves to nothing");
        assert_eq!(chord(settings_of(&node)), Some((",".into(), true, false)));
    }
}
