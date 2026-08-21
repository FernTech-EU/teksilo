// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Bridge from a [`MenuModel`] to the platform native menu (`teksilo-platform`'s
//! [`NativeMenuHandle`]).
//!
//! Resolves the model into a plain [`NativeMenuSnapshot`] (titles localized +
//! mnemonic-stripped, shortcuts resolved to key equivalents), installs it for
//! the current window, and wires reactive `Signal`s so a toggled check or a
//! disabled item updates the native item in place.

use std::collections::HashMap;

use teksilo_core::MenuItemId;
use teksilo_core::ObserverHandle;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::Key;
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::signal::Prop;
use teksilo_data::CheckState;
use teksilo_platform::native_menu::{
    MenuItemDelta, NativeCheck, NativeKeyEquivalent, NativeMenuActivation, NativeMenuHandle,
    NativeMenuNode, NativeMenuSnapshot, StandardMenuRole,
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
            },
        );
    }
    let snapshot = NativeMenuSnapshot { roots };

    handle.set_window_menu(window_id, snapshot, activations, poster);

    // Wire reactive per-item updates (enabled / check / radio).
    let mut observers = Vec::new();
    for item in reactive {
        if let Prop::Bound(sig) = item.enabled {
            let h = handle.clone();
            let id = item.id;
            observers.push(sig.observe(move |v| {
                h.update_item(
                    id,
                    MenuItemDelta {
                        enabled: Some(*v),
                        ..Default::default()
                    },
                );
            }));
        }
        match item.state {
            MenuItemState::Plain => {}
            // Two-way and reflect-only both mirror the signal into the native
            // checkmark; they differ only in the in-window click behavior.
            MenuItemState::Check(sig) | MenuItemState::ReflectCheck(sig) => {
                let h = handle.clone();
                let id = item.id;
                observers.push(sig.observe(move |v| {
                    h.update_item(
                        id,
                        check_delta(if *v {
                            NativeCheck::On
                        } else {
                            NativeCheck::Off
                        }),
                    );
                }));
            }
            MenuItemState::TriCheck(sig) => {
                let h = handle.clone();
                let id = item.id;
                observers.push(sig.observe(move |v| {
                    h.update_item(id, check_delta(tri_to_native(*v)));
                }));
            }
            MenuItemState::Radio { value, selected } => {
                let h = handle.clone();
                let id = item.id;
                observers.push(selected.observe(move |sel| {
                    let check = if *sel == value {
                        NativeCheck::On
                    } else {
                        NativeCheck::Off
                    };
                    h.update_item(id, check_delta(check));
                }));
            }
        }
    }

    Some(NativeMenuBinding {
        _observers: observers,
    })
}

/// One item's reactive sources, gathered during resolution.
struct ReactiveItem {
    id: MenuItemId,
    enabled: Prop<bool>,
    state: MenuItemState,
}

fn resolve_node(
    node: &MenuNode,
    ctx: &BuildContext,
    activations: &mut HashMap<MenuItemId, NativeMenuActivation>,
    reactive: &mut Vec<ReactiveItem>,
) -> Option<NativeMenuNode> {
    match node {
        MenuNode::Separator => Some(NativeMenuNode::Separator),
        MenuNode::Standard(sm) => Some(resolve_standard(sm, activations)),
        MenuNode::Submenu {
            title, children, ..
        } => Some(NativeMenuNode::Submenu {
            title: strip_title(&title.resolve_now()),
            children: children
                .iter()
                .filter_map(|n| resolve_node(n, ctx, activations, reactive))
                .collect(),
        }),
        // A currently-hidden item is omitted from the native snapshot. (It
        // reappears on the next menu rebuild; for fully-dynamic native menus use
        // `MenuModel::remove` / `push_item`.)
        MenuNode::Item(entry) if !entry.visible.get() => None,
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
                MenuItemState::Radio { value, selected } => {
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

            activations.insert(
                entry.id,
                NativeMenuActivation {
                    intent: entry.intent,
                    action: entry.action.clone(),
                },
            );
            reactive.push(ReactiveItem {
                id: entry.id,
                enabled: entry.enabled.clone(),
                state: entry.state.clone(),
            });

            Some(NativeMenuNode::Item {
                id: entry.id,
                title: strip_title(&entry.title.resolve_now()),
                key_equiv,
                enabled: entry.enabled.get(),
                check,
            })
        }
    }
}

/// Resolve one platform-standard menu into its boundary node.
///
/// Split out of [`resolve_node`] because it needs no [`BuildContext`]: a
/// standard menu carries labels and, optionally, a routed Quit, and the
/// platform fills in the rest. That makes it the one part of the native bridge
/// a test can exercise on any OS — everything around it is behind the macOS
/// gate in `MenuBar::build`, so a routing bug would otherwise only be
/// observable on the platform it breaks.
fn resolve_standard(
    sm: &StandardMenu,
    activations: &mut HashMap<MenuItemId, NativeMenuActivation>,
) -> NativeMenuNode {
    // A routed Quit is an ordinary activation under an id the model minted
    // once, so it survives every rebuild — unlike the rest of a standard menu,
    // which the platform fills in from labels alone.
    if let Some((intent, id)) = sm.quit_route() {
        activations.insert(
            id,
            NativeMenuActivation {
                intent: Some(intent),
                action: None,
            },
        );
    }
    NativeMenuNode::Standard {
        role: sm.role(),
        labels: sm.resolve_labels(),
        quit_item: sm.quit_route().map(|(_, id)| id),
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
/// Modifier convention (matches Qt's `Qt::CTRL` → ⌘ on macOS): the primary
/// accelerator modifier — `Ctrl` *or* `Super` in a Teksilo shortcut — maps to
/// the platform's Command flag, so an app that writes `KeyStroke::ctrl(Key::S)`
/// gets ⌘S in the native menu. `Alt`/`Shift` map straight through.
fn native_key_equiv(ks: KeyStroke) -> NativeKeyEquivalent {
    NativeKeyEquivalent {
        key: key_to_equiv(ks.key),
        command: ks.modifiers.super_key() || ks.modifiers.ctrl(),
        shift: ks.modifiers.shift(),
        alt: ks.modifiers.alt(),
        control: false,
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

    fn quit_item_of(node: &NativeMenuNode) -> Option<MenuItemId> {
        match node {
            NativeMenuNode::Standard { quit_item, .. } => *quit_item,
            _ => panic!("expected a standard menu node"),
        }
    }

    /// The default is the platform's own Quit. An app that declared no handler
    /// still gets a working ⌘Q out of `terminate:`, and that guarantee is what
    /// the auto-injected App menu rests on.
    #[test]
    fn a_standard_app_menu_routes_nothing_by_default() {
        let mut activations = HashMap::new();
        let node = resolve_standard(&StandardMenu::app(), &mut activations);
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
            quit_item_of(&resolve_standard(&menu, &mut first)),
            quit_item_of(&resolve_standard(&menu, &mut second)),
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
        );
        let b = resolve_standard(
            &StandardMenu::app().quit_intent("app.quit"),
            &mut activations,
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
        );
        assert_eq!(labels_of(&node).quit, "Quitter");
    }
}
