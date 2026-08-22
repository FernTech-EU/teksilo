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
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::signal::Prop;
use teksilo_data::CheckState;
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
