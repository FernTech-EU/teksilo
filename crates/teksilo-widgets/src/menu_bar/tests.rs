// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for [`MenuBar`].
use super::*;
use crate::MenuItem;
use crate::menu_list::MenuList;
use teksilo_core::accesskit::Role;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::window::state::WindowStateInit;
use teksilo_core::window::{TeksiloWindowId, WindowPlacement, WindowState};
use teksilo_i18n::lit;

fn tree_with_window() -> WidgetTree {
    let mut t = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    t.set_window_state(WindowState::new(WindowStateInit {
        id: TeksiloWindowId::new(1),
        string_id: Some("test".to_string()),
        placement: WindowPlacement::Floating,
        title: "Test".to_string(),
        size: (800, 600),
        position: (0, 0),
        focused: false,
        resizable: true,
        always_on_top: false,
    }));
    t
}

/// Total active widgets whose concrete type name contains `needle`.
fn count_by_type(t: &WidgetTree, needle: &str) -> u32 {
    t.widget_type_histogram()
        .iter()
        .filter(|(name, _)| name.contains(needle))
        .map(|(_, n)| *n)
        .sum()
}

/// Distinctly-typed leaf used to prove a leading/trailing slot's
/// content survives a rebuild (its type can't collide with the bar's
/// own internal widgets).
#[derive(Debug)]
struct SlotMarker;
impl Widget for SlotMarker {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(12.0, 12.0).into()
    }
}

/// Slot leaf that records how many times it was built and its widget id —
/// to prove a stateful slot is *preserved* (built once, same instance),
/// not rebuilt, across a MenuBar rebuild.
#[derive(Debug)]
struct CountingSlot {
    builds: std::rc::Rc<std::cell::Cell<u32>>,
    id_out: std::rc::Rc<std::cell::Cell<Option<WidgetId>>>,
}
impl Widget for CountingSlot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.builds.set(self.builds.get() + 1);
        self.id_out.set(Some(ctx.self_id()));
        vec![]
    }
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(12.0, 12.0).into()
    }
}

fn first_descendant_with_role(t: &WidgetTree, from: WidgetId, role: Role) -> Option<WidgetId> {
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(from);
    while let Some(id) = queue.pop_front() {
        if t.accessibility_node(id).role() == role {
            return Some(id);
        }
        for child in t.children(id) {
            queue.push_back(child);
        }
    }
    None
}

fn collect_descendants_with_role(t: &WidgetTree, from: WidgetId, role: Role) -> Vec<WidgetId> {
    let mut queue = std::collections::VecDeque::new();
    let mut out = Vec::new();
    queue.push_back(from);
    while let Some(id) = queue.pop_front() {
        if t.accessibility_node(id).role() == role {
            out.push(id);
        }
        for child in t.children(id) {
            queue.push_back(child);
        }
    }
    out
}

/// Collect the RGB (0..=255) of every glyph painted by a one-pass
/// render of a light-themed MenuBar carrying `&File` / `&Edit`. In a
/// bare tree the only text is the two trigger labels, so the returned
/// colours ARE the trigger label colours. `use_model` switches between
/// the direct `.menu()` builder and the `from_model` path.
fn light_menubar_trigger_glyph_rgb(use_model: bool) -> Vec<[u32; 3]> {
    let mut t = WidgetTree::new()
        .with_theme(teksilo_core::presets::intui::light())
        .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
            teksilo_canvas::MockTextBackend::new(),
        )));
    t.set_window_state(WindowState::new(WindowStateInit {
        id: TeksiloWindowId::new(1),
        string_id: Some("test".to_string()),
        placement: WindowPlacement::Floating,
        title: "Test".to_string(),
        size: (800, 600),
        position: (0, 0),
        focused: false,
        resizable: true,
        always_on_top: false,
    }));
    if use_model {
        let model = crate::menu::MenuModel::new()
            .menu(lit!("&File"), |m| m)
            .menu(lit!("&Edit"), |m| m);
        t.add(MenuBar::from_model(model));
    } else {
        t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new())),
        );
    }
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    let frame = t.render();
    frame
        .glyphs
        .iter()
        .map(|g| {
            [
                (g.color[0] * 255.0).round() as u32,
                (g.color[1] * 255.0).round() as u32,
                (g.color[2] * 255.0).round() as u32,
            ]
        })
        .collect()
}

/// Regression: the top-level trigger labels must paint in the ACTIVE
/// theme's `text_primary`, never a stale constructor-default theme.
///
/// Historical bug: a light-launched app rendered the "File" / "Edit"
/// trigger labels in the *dark* theme's grey `text_primary` (#DFE1E5),
/// invisible on a light bar, while the dropdowns rendered fine. Cause:
/// the trigger colour is a `theme_signal.map(...)` derived signal, and
/// `WidgetTree::with_theme` updated the cached `Theme` (seen by
/// `ctx.theme()` / role resolution) but NOT `theme_signal`, which stayed
/// at the constructor default. The first `set_theme` (e.g. a dark→light
/// toggle) re-aligned the signal, which is why the bug self-healed on a
/// theme switch. Fixed by keeping `theme` + `theme_signal` in lockstep
/// and defaulting the constructor to light. Covers both trigger build
/// paths.
#[test]
fn trigger_labels_paint_in_active_theme_color() {
    let rgb_of = |c: teksilo_tokens::Color| {
        let a = c.to_array();
        [
            (a[0] * 255.0).round() as u32,
            (a[1] * 255.0).round() as u32,
            (a[2] * 255.0).round() as u32,
        ]
    };
    let light_rgb = rgb_of(teksilo_core::presets::intui::light().colors.text_primary);
    let dark_rgb = rgb_of(teksilo_core::presets::intui::dark().colors.text_primary);
    assert_ne!(
        light_rgb, dark_rgb,
        "presets must differ for this test to mean anything"
    );

    for use_model in [false, true] {
        let glyphs = light_menubar_trigger_glyph_rgb(use_model);
        assert!(
            !glyphs.is_empty(),
            "expected trigger label glyphs (use_model={use_model})"
        );
        for rgb in &glyphs {
            assert_eq!(
                *rgb, light_rgb,
                "trigger label glyph must use the active (light) theme's text_primary, \
                 not a stale constructor-default theme (use_model={use_model})"
            );
        }
    }
}

/// A `MockTextBackend` wrapper that models the typesetter's glyph-cache
/// eviction: while `evicted` is set, `ensure_glyphs` returns nothing
/// (as the real bridge does once a cached layout's glyphs are dropped),
/// and `layout_single_line` clears the flag (re-shaping repopulates the
/// cache, mirroring the real bridge). Lets a headless test reproduce the
/// "menu labels vanish under atlas pressure" bug deterministically.
struct EvictingTextBackend {
    inner: teksilo_canvas::MockTextBackend,
    evicted: std::rc::Rc<std::cell::Cell<bool>>,
}

impl teksilo_canvas::TextBackend for EvictingTextBackend {
    fn layout_single_line(
        &mut self,
        text: &str,
        style: &teksilo_tokens::TextStyle,
        max_width: Option<f32>,
    ) -> teksilo_canvas::TextLayout {
        // Re-shaping repopulates the glyph cache → no longer evicted.
        self.evicted.set(false);
        self.inner.layout_single_line(text, style, max_width)
    }

    fn ensure_glyphs(
        &mut self,
        layout: &teksilo_canvas::TextLayout,
    ) -> Vec<teksilo_canvas::GlyphQuad> {
        if self.evicted.get() {
            Vec::new()
        } else {
            self.inner.ensure_glyphs(layout)
        }
    }
}

/// Regression: a trigger label must keep rendering after the typesetter
/// evicts its cached layout's glyphs. Under atlas pressure (a text-heavy
/// window) the renderer's eviction-recovery path clears the bridge's
/// glyph cache and re-paints WITHOUT re-laying-out, so `MenuLabel`'s
/// retained `TextLayout` no longer resolves and `draw_text_layout` draws
/// nothing — the labels silently vanished until the next relayout (a
/// theme switch). The fix re-shapes through `draw_text` when the cached
/// draw produces no glyphs.
#[test]
fn trigger_labels_survive_glyph_cache_eviction() {
    let evicted = std::rc::Rc::new(std::cell::Cell::new(false));
    let backend = EvictingTextBackend {
        inner: teksilo_canvas::MockTextBackend::new(),
        evicted: evicted.clone(),
    };
    let mut t = WidgetTree::new()
        .with_theme(teksilo_core::presets::intui::light())
        .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(backend)));
    t.set_window_state(WindowState::new(WindowStateInit {
        id: TeksiloWindowId::new(1),
        string_id: Some("test".to_string()),
        placement: WindowPlacement::Floating,
        title: "Test".to_string(),
        size: (800, 600),
        position: (0, 0),
        focused: false,
        resizable: true,
        always_on_top: false,
    }));
    t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new())),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    let glyphs_initial = t.render().glyphs.len();
    assert!(glyphs_initial > 0, "trigger labels must render initially");

    // Mimic the eviction-recovery path: the bridge's glyph cache is
    // cleared (so the retained layout's glyphs are gone) and the tree
    // is re-painted WITHOUT a relayout.
    evicted.set(true);
    t.invalidate_all_paints();
    let glyphs_after = t.render().glyphs.len();
    assert!(
        glyphs_after > 0,
        "trigger labels must survive glyph-cache eviction (re-shape fallback); \
         got {glyphs_after} glyphs after eviction"
    );
}

#[test]
fn menubar_emits_role_menubar() {
    let mut t = tree_with_window();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new())),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert_eq!(t.accessibility_node(mb).role(), Role::MenuBar);
}

#[test]
fn trigger_uses_stripped_name_in_at() {
    let mut t = tree_with_window();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new())),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
    assert_eq!(triggers.len(), 2);
    // The stripped name "File" / "Edit", NOT "&File" / "&Edit".
    let info0 = t.accessibility_node(triggers[0]);
    let info1 = t.accessibility_node(triggers[1]);
    assert_eq!(info0.name(), Some("File"));
    assert_eq!(info1.name(), Some("Edit"));
}

#[test]
fn trigger_arrow_navigation_ltr_right_goes_to_next() {
    let mut t = tree_with_window();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .menu(lit!("&View"), || Box::new(MenuList::new())),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
    assert_eq!(triggers.len(), 3);

    // From the File trigger, ArrowRight opens the next (Edit) menu in LTR.
    t.focus(triggers[0]);
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert!(t.accessibility_node(triggers[1]).is_expanded());
    assert!(!t.accessibility_node(triggers[0]).is_expanded());
}

#[test]
fn trigger_arrow_navigation_rtl_right_goes_to_previous() {
    let mut t = tree_with_window();
    t.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .menu(lit!("&View"), || Box::new(MenuList::new())),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
    assert_eq!(triggers.len(), 3);

    // Under RTL the bar runs right-to-left, so ArrowRight moves to the
    // *previous* menu — from File (index 0) that wraps to View (index 2).
    t.focus(triggers[0]);
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert!(t.accessibility_node(triggers[2]).is_expanded());
    assert!(!t.accessibility_node(triggers[0]).is_expanded());
}

#[test]
fn dispatcher_installed_on_every_platform() {
    let mut t = tree_with_window();
    t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new())),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    let window = t.window_state().expect("window state attached");
    assert!(
        window.menubar_dispatcher().is_some(),
        "MenuBar should install the window-level dispatcher on every \
         platform — framework menus aren't the OS system menu and need \
         keyboard accelerators wired regardless of host OS"
    );
}

#[test]
fn rebuilding_menubar_does_not_double_install_dispatcher() {
    // Regression: `install_menubar_dispatcher` debug_asserts that
    // the slot is empty before installing. The old `MenuBar::build`
    // implementation called install while the previous build's
    // `MenubarGuard` was still alive in `self.menubar_guard`,
    // which tripped the assert on every rebuild. Fixed by
    // dropping the old guard FIRST.
    let mut t = tree_with_window();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new())),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert!(t.window_state().unwrap().menubar_dispatcher().is_some());
    // An empty bar still has a (now-empty) dispatcher, so assert the
    // menus themselves are present — the dispatcher check alone would
    // pass straight through a bar that self-emptied on rebuild.
    assert_eq!(
        count_by_type(&t, "MenuBarTrigger"),
        2,
        "two menus before rebuild"
    );
    // Force a rebuild and confirm the dispatcher install path
    // doesn't crash (debug builds) or silently overwrite a live
    // guard (release builds).
    t.arena_mark_needs_rebuild_for_testing(mb);
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert!(
        t.window_state().unwrap().menubar_dispatcher().is_some(),
        "after rebuild the dispatcher slot must still point at \
         the most-recently-installed dispatcher"
    );
    assert_eq!(
        count_by_type(&t, "MenuBarTrigger"),
        2,
        "classic .menu() bar must keep its menus across a rebuild \
         (regression: build() used to mem::take the entries, leaving \
         an empty bar on the next theme/locale rebuild)"
    );
}

#[test]
fn menubar_slots_survive_rebuild() {
    // Regression: leading_slot / trailing_slot were drain(..)-ed on
    // every build, so a bar with an app icon (leading) or a search /
    // avatar (trailing) lost those slots on a theme / locale / model
    // rebuild — even for reactive model-based bars, which otherwise
    // re-derive their menus correctly.
    let mut t = tree_with_window();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .leading_slot(SlotMarker)
            .trailing_slot(SlotMarker),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert_eq!(
        count_by_type(&t, "SlotMarker"),
        2,
        "both slots before rebuild"
    );
    assert_eq!(count_by_type(&t, "MenuBarTrigger"), 1);

    t.arena_mark_needs_rebuild_for_testing(mb);
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert_eq!(
        count_by_type(&t, "SlotMarker"),
        2,
        "leading + trailing slots must survive a rebuild"
    );
    assert_eq!(count_by_type(&t, "MenuBarTrigger"), 1, "menu survives too");
}

#[test]
fn model_menubar_slots_survive_first_layout_self_rebuild() {
    // A `from_model` bar binds `model.version()` at `BindingLevel::Rebuild`,
    // so it re-runs build() once during the very first layout pass. With the
    // old drain(..) slots, that self-rebuild emptied them before the first
    // frame ever painted — a model bar's leading/trailing slots rendered for
    // zero frames. A single layout must leave both slots present.
    let file = teksilo_core::MenuItemId::next();
    let model = crate::menu::MenuModel::new().menu_with_id(file, lit!("File"), |m| {
        m.item(crate::menu::MenuEntry::new(lit!("New")))
    });
    let mut t = tree_with_window();
    let _mb = t.add(
        MenuBar::from_model(model.clone())
            .leading_slot(SlotMarker)
            .trailing_slot(SlotMarker),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert_eq!(
        count_by_type(&t, "SlotMarker"),
        2,
        "model bar's slots must survive the self-rebuild on first layout"
    );

    // And they survive a subsequent model mutation (another rebuild).
    model.push_item(file, crate::menu::MenuEntry::new(lit!("Open")));
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert_eq!(
        count_by_type(&t, "SlotMarker"),
        2,
        "slots survive a model-mutation rebuild too"
    );
}

#[test]
fn model_menubar_preserves_stateful_slot_across_rebuild() {
    // The follow-on capability: a stateful slot control keeps its identity
    // (built once, same WidgetId) across a model-driven rebuild — the
    // memoized slot is re-attached, not reconstructed, so its internal
    // state (focus, caret, scroll) is preserved. Adding a second top-level
    // menu proves the bar genuinely rebuilt (trigger count 1 → 2) while the
    // slot's build count stays 1.
    let builds = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let id_out = std::rc::Rc::new(std::cell::Cell::new(None));
    let file = teksilo_core::MenuItemId::next();
    let model = crate::menu::MenuModel::new().menu_with_id(file, lit!("File"), |m| {
        m.item(crate::menu::MenuEntry::new(lit!("New")))
    });
    let mut t = tree_with_window();
    t.add(
        MenuBar::from_model(model.clone()).leading_slot(CountingSlot {
            builds: builds.clone(),
            id_out: id_out.clone(),
        }),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    let first_id = id_out.get().expect("slot built");
    assert_eq!(builds.get(), 1, "slot built exactly once initially");
    assert_eq!(count_by_type(&t, "MenuBarTrigger"), 1);

    // Structural model change → MenuBar rebuild.
    model.push_menu(lit!("Edit"), |m| {
        m.item(crate::menu::MenuEntry::new(lit!("Undo")))
    });
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));

    assert_eq!(
        count_by_type(&t, "MenuBarTrigger"),
        2,
        "the bar rebuilt (a second menu trigger appeared)"
    );
    assert_eq!(
        builds.get(),
        1,
        "the stateful slot was preserved, not rebuilt, across the rebuild"
    );
    assert_eq!(
        id_out.get(),
        Some(first_id),
        "the slot kept its identity (same widget instance)"
    );
}

#[test]
fn windowstate_dispatcher_slot_reinstall_after_guard_drop() {
    // Direct unit test of the WindowState slot lifecycle —
    // installing a second dispatcher after dropping the first
    // guard must succeed without a debug_assert.
    use teksilo_core::window::{MenubarAction, MenubarDispatcher, MenubarKeyEvent};

    struct Noop;
    impl MenubarDispatcher for Noop {
        fn try_handle(&self, _ev: &MenubarKeyEvent) -> Option<MenubarAction> {
            None
        }
    }

    let mut t = tree_with_window();
    let window = t.window_state().unwrap().clone();
    let guard_a = window.install_menubar_dispatcher(Rc::new(Noop));
    assert!(window.menubar_dispatcher().is_some());
    drop(guard_a);
    assert!(
        window.menubar_dispatcher().is_none(),
        "dropping the guard must clear the slot"
    );
    let _guard_b = window.install_menubar_dispatcher(Rc::new(Noop));
    assert!(
        window.menubar_dispatcher().is_some(),
        "second install after first guard's drop must succeed without an assert"
    );
    let _ = &mut t;
}

// --- Pure-function dispatcher tests (platform-independent) ---

/// Fabricate a `WidgetId` from a numeric tag for tests that don't
/// need a real arena. Mirrors the convention used across
/// `teksilo-core`'s signal / overlay tests.
fn fake_id(n: u64) -> WidgetId {
    slotmap::KeyData::from_ffi(n).into()
}

fn make_dispatcher() -> MenuBarDispatcher {
    let mut mnemonic_table = HashMap::new();
    mnemonic_table.insert('f', 0);
    mnemonic_table.insert('e', 1);
    mnemonic_table.insert('v', 2);
    MenuBarDispatcher {
        trigger_ids: vec![fake_id(10), fake_id(11), fake_id(12)],
        mnemonic_table,
    }
}

#[test]
fn dispatcher_f10_focuses_first_trigger() {
    let d = make_dispatcher();
    let action = d.try_handle(&MenubarKeyEvent {
        key: Key::F10,
        modifiers: Modifiers::NONE,
    });
    assert!(matches!(
        action,
        Some(MenubarAction::FocusTrigger { trigger_id, .. }) if trigger_id == fake_id(10)
    ));
}

#[test]
fn dispatcher_f10_with_modifier_ignored() {
    let d = make_dispatcher();
    let action = d.try_handle(&MenubarKeyEvent {
        key: Key::F10,
        modifiers: Modifiers::CTRL,
    });
    assert!(action.is_none());
}

// Alt+letter is intentionally unwired on macOS — the OS rewrites
// Option+letter for accented input before the app sees the
// keystroke, so the dispatcher's Alt branch is compiled out
// there. These tests assert the Win32 / GTK semantic.
#[cfg(not(target_os = "macos"))]
#[test]
fn dispatcher_alt_letter_opens_matching_menu() {
    let d = make_dispatcher();
    let action = d.try_handle(&MenubarKeyEvent {
        key: Key::F,
        modifiers: Modifiers::ALT,
    });
    assert!(matches!(
        action,
        Some(MenubarAction::OpenMenu { trigger_id, .. }) if trigger_id == fake_id(10)
    ));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn dispatcher_alt_letter_no_match_intercepts() {
    let d = make_dispatcher();
    let action = d.try_handle(&MenubarKeyEvent {
        key: Key::Q,
        modifiers: Modifiers::ALT,
    });
    assert!(matches!(action, Some(MenubarAction::Intercept)));
}

#[test]
fn dispatcher_alt_unrelated_key_ignored() {
    // Modifier != bare Alt → no menubar action. We use Modifiers::CTRL
    // here because constructing a multi-modifier value isn't part
    // of the public Modifiers API; the dispatcher relies on exact
    // equality with `Modifiers::ALT`.
    let d = make_dispatcher();
    let action = d.try_handle(&MenubarKeyEvent {
        key: Key::F,
        modifiers: Modifiers::CTRL,
    });
    assert!(action.is_none());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn dispatcher_case_insensitive_alt_letter() {
    let d = make_dispatcher();
    // Lowercase 'f' and uppercase 'F' both open the matching menu.
    let lower = d.try_handle(&MenubarKeyEvent {
        key: Key::Character('f'),
        modifiers: Modifiers::ALT,
    });
    let upper = d.try_handle(&MenubarKeyEvent {
        key: Key::Character('F'),
        modifiers: Modifiers::ALT,
    });
    assert!(matches!(lower, Some(MenubarAction::OpenMenu { .. })));
    assert!(matches!(upper, Some(MenubarAction::OpenMenu { .. })));
}

#[cfg(target_os = "macos")]
#[test]
fn dispatcher_alt_letter_does_not_intercept_on_macos() {
    // macOS-specific: the dispatcher must NOT intercept Alt+letter
    // because the OS rewrites it for accented character input;
    // intercepting would silently break text input.
    let d = make_dispatcher();
    let action = d.try_handle(&MenubarKeyEvent {
        key: Key::F,
        modifiers: Modifiers::ALT,
    });
    assert!(
        action.is_none(),
        "macOS: Alt+letter must fall through to focus dispatch \
         so accented character input still works in text fields"
    );
}

#[test]
fn dispatcher_alt_tap_focuses_first_trigger() {
    let d = make_dispatcher();
    let action = d.on_alt_tap();
    assert!(matches!(
        action,
        Some(MenubarAction::FocusTrigger { trigger_id, .. }) if trigger_id == fake_id(10)
    ));
}

#[test]
fn dispatcher_alt_tap_with_no_triggers_is_none() {
    let d = MenuBarDispatcher {
        trigger_ids: Vec::new(),
        mnemonic_table: HashMap::new(),
    };
    assert!(d.on_alt_tap().is_none());
    assert!(
        d.try_handle(&MenubarKeyEvent {
            key: Key::F10,
            modifiers: Modifiers::NONE,
        })
        .is_none()
    );
}

// ── Collapsible (hamburger) mode ─────────────────────────────────────

fn collapsible_tree() -> WidgetTree {
    let mut t = WidgetTree::new()
        .with_theme(teksilo_core::presets::intui::light())
        .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
            teksilo_canvas::MockTextBackend::new(),
        )));
    t.set_window_state(WindowState::new(WindowStateInit {
        id: TeksiloWindowId::new(1),
        string_id: Some("test".to_string()),
        placement: WindowPlacement::Floating,
        title: "Test".to_string(),
        size: (800, 600),
        position: (0, 0),
        focused: false,
        resizable: true,
        always_on_top: false,
    }));
    t
}

#[test]
fn collapsible_always_shows_hamburger() {
    let mut t = collapsible_tree();
    let mb_widget = MenuBar::new()
        .menu(lit!("&File"), || Box::new(MenuList::new()))
        .menu(lit!("&Edit"), || Box::new(MenuList::new()))
        .collapse_policy(CollapsePolicy::Always);
    let collapsed = mb_widget.is_collapsed();
    let mb = t.add(mb_widget);
    // Two passes: pass 1 sets `collapsed`, pass 2 settles visibility.
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(collapsed.get(), "Always policy must collapse to hamburger");
    let children = t.children(mb);
    assert_eq!(children.len(), 2, "[bar, hamburger]");
    let (bar, hamburger) = (children[0], children[1]);
    assert!(t.is_active(hamburger), "hamburger active when collapsed");
    assert!(
        !t.is_active(bar),
        "bar dormant when collapsed and not revealed"
    );
}

/// The hamburger keeps a constant (intrinsic) width even when a
/// stretching parent hands the collapsed MenuBar a much wider slot.
#[test]
fn collapsible_hamburger_keeps_constant_width_in_wide_slot() {
    use crate::primitives::FixedSize;
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always),
    );
    // FixedSize fills its child to 600px wide.
    let _slot = t.add(FixedSize::new().width(600.0_f32).child_id(mb));
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));

    let hamburger = t.children(mb)[1];
    let hw = t.bounds(hamburger).width;
    assert!(
        hw > 0.0 && hw < 200.0,
        "hamburger width {hw} must stay compact, not fill the 600px slot"
    );
}

#[test]
fn collapsible_responsive_collapses_when_narrow() {
    let mut t = collapsible_tree();
    let mb_widget = MenuBar::new()
        .menu(lit!("&File"), || Box::new(MenuList::new()))
        .menu(lit!("&Edit"), || Box::new(MenuList::new()))
        .menu(lit!("&View"), || Box::new(MenuList::new()))
        .collapsible();
    let collapsed = mb_widget.is_collapsed();
    let _mb = t.add(mb_widget);
    t.layout(SizeProposal::exact(40.0, 100.0));
    t.layout(SizeProposal::exact(40.0, 100.0));
    assert!(collapsed.get(), "narrow width must collapse to hamburger");
}

#[test]
fn collapsible_responsive_expands_when_wide() {
    let mut t = collapsible_tree();
    let mb_widget = MenuBar::new()
        .menu(lit!("&File"), || Box::new(MenuList::new()))
        .menu(lit!("&Edit"), || Box::new(MenuList::new()))
        .collapsible();
    let collapsed = mb_widget.is_collapsed();
    let mb = t.add(mb_widget);
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(!collapsed.get(), "wide width must show the inline bar");
    let children = t.children(mb);
    assert!(t.is_active(children[0]), "bar active inline when wide");
    assert!(
        !t.is_active(children[1]),
        "hamburger dormant when bar is inline"
    );
}

/// A collapsible MenuBar wide enough on its own becomes a hamburger
/// once its allotted width drops below the bar's intrinsic width.
#[test]
fn collapsible_responsive_toggles_with_width() {
    let mut t = collapsible_tree();
    let mb_widget = MenuBar::new()
        .menu(lit!("&File"), || Box::new(MenuList::new()))
        .menu(lit!("&Edit"), || Box::new(MenuList::new()))
        .menu(lit!("&View"), || Box::new(MenuList::new()))
        .collapsible();
    let collapsed = mb_widget.is_collapsed();
    let _mb = t.add(mb_widget);

    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(!collapsed.get(), "wide → inline");

    t.layout(SizeProposal::exact(30.0, 100.0));
    t.layout(SizeProposal::exact(30.0, 100.0));
    assert!(collapsed.get(), "narrow → hamburger");

    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(!collapsed.get(), "wide again → inline");
}

#[test]
fn collapsible_click_hamburger_reveals_bar_overlay() {
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let children = t.children(mb);
    let (bar, hamburger) = (children[0], children[1]);
    assert!(!t.is_active(bar), "bar hidden before reveal");

    t.click(hamburger);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(t.is_active(bar), "clicking the hamburger reveals the bar");
}

#[test]
fn collapsible_reveal_focuses_first_trigger() {
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let hamburger = t.children(mb)[1];

    t.click(hamburger);
    t.layout(SizeProposal::exact(800.0, 100.0));

    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
    assert_eq!(triggers.len(), 2);
    assert_eq!(
        t.focused(),
        Some(triggers[0]),
        "revealing the bar focuses the first menu trigger"
    );
}

/// Regression: ArrowLeft must navigate to the PREVIOUS top-level menu
/// in the revealed bar, not close the current one. The bar is itself a
/// host overlay, so the dispatch-level "overlay back" key (ArrowLeft in
/// LTR) must not mistake an open top-level menu for a nested submenu.
#[test]
fn collapsible_revealed_bar_left_navigates_not_closes() {
    let menu = |label: &'static str| {
        move || -> Box<dyn Widget> {
            Box::new(MenuList::new().item(crate::menu_item::MenuItem::new(lit!(label))))
        }
    };
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), menu("New"))
            .menu(lit!("&Edit"), menu("Undo"))
            .menu(lit!("&View"), menu("Zoom"))
            .collapse_policy(CollapsePolicy::Always),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let hamburger = t.children(mb)[1];
    t.click(hamburger);
    t.layout(SizeProposal::exact(800.0, 100.0));
    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
    let expanded = |t: &WidgetTree| -> Vec<bool> {
        triggers
            .iter()
            .map(|&id| t.accessibility_node(id).is_expanded())
            .collect()
    };

    // Open File → Edit → View via ArrowRight.
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert_eq!(expanded(&t), vec![false, false, true], "RIGHT reached View");

    // ArrowLeft must move to Edit (the previous menu), NOT close View.
    t.press_key(Key::ArrowLeft, Modifiers::NONE);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert_eq!(
        expanded(&t),
        vec![false, true, false],
        "LEFT navigates to the previous menu (Edit), not closes"
    );

    // And once more to File.
    t.press_key(Key::ArrowLeft, Modifiers::NONE);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert_eq!(
        expanded(&t),
        vec![true, false, false],
        "LEFT again reaches File"
    );
}

/// Accessibility: the hamburger is a `Role::Button` whose `expanded`
/// state tracks whether the bar is revealed (the ARIA disclosure
/// pattern), and dismissing the bar restores focus to the hamburger.
#[test]
fn collapsible_hamburger_accessibility() {
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let hamburger = t.children(mb)[1];

    // Collapsed: a button that is NOT expanded.
    let info = t.accessibility_node(hamburger);
    assert_eq!(info.role(), Role::Button);
    assert!(
        !info.is_expanded(),
        "collapsed hamburger reports expanded=false"
    );

    // Revealed: expanded flips to true; the bar is a MenuBar landmark.
    t.click(hamburger);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(
        t.accessibility_node(hamburger).is_expanded(),
        "revealed hamburger reports expanded=true"
    );
    assert!(first_descendant_with_role(&t, mb, Role::MenuBar).is_some());

    // Dismiss with Escape: expanded back to false, focus restored to
    // the hamburger (not lost in the now-hidden bar). The dismissal is
    // deferred for the roll-back tween, so advance past it first.
    t.press_key(Key::Escape, Modifiers::NONE);
    t.advance_time(std::time::Duration::from_secs(1));
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(
        !t.accessibility_node(hamburger).is_expanded(),
        "collapsed again after Escape"
    );
    assert_eq!(
        t.focused(),
        Some(hamburger),
        "focus returns to the hamburger after the bar is dismissed"
    );
}

/// Regression: arrow-navigating between menus must NOT tear down the
/// revealed bar. `MenuContext::open_at` calls `dismiss_all_except_hosts`;
/// the bar overlay is marked `Role::MenuBar` (a host) via an access-role
/// override, so it must survive — and its triggers keep valid (non-zero)
/// bounds so dropdowns anchor under them, not at the window origin.
#[test]
fn collapsible_revealed_bar_survives_arrow_navigation() {
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .menu(lit!("&View"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let (bar, hamburger) = (t.children(mb)[0], t.children(mb)[1]);

    t.click(hamburger);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(t.is_active(bar), "bar revealed");
    // Let the unroll tween finish so the bar reaches full width and
    // its triggers settle at their on-screen positions.
    t.tick_animations(std::time::Duration::from_millis(500));
    t.layout(SizeProposal::exact(800.0, 100.0));

    // Arrow-navigate to the next menu.
    t.press_key(Key::ArrowRight, Modifiers::NONE);
    t.layout(SizeProposal::exact(800.0, 100.0));

    assert!(
        t.is_active(bar),
        "bar must stay visible while navigating between menus"
    );
    // Triggers remain laid out inside the floating bar (offset from the
    // origin), so the opened dropdown anchors under a trigger.
    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
    let b = t.bounds(triggers[1]);
    assert!(
        b.width > 0.0 && (b.x > 0.0 || b.y > 0.0),
        "trigger stays laid out in the floating bar, not collapsed to the origin: {b:?}"
    );
}

#[test]
fn revealed_bar_height_matches_hamburger() {
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always)
            .hamburger_size(IconButtonSize::Toolbar),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let (bar, hamburger) = (t.children(mb)[0], t.children(mb)[1]);

    t.click(hamburger);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(t.is_active(bar), "bar revealed");

    let ham_h = t.bounds(hamburger).height;
    let bar_h = t.bounds(bar).height;
    assert!(ham_h > 0.0, "hamburger laid out: {ham_h}");
    assert!(
        (bar_h - ham_h).abs() < 0.5,
        "floating bar height ({bar_h}) matches the hamburger ({ham_h})"
    );
}

#[test]
fn revealed_bar_unrolls_open_and_defers_close() {
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .menu(lit!("&View"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let (bar, hamburger) = (t.children(mb)[0], t.children(mb)[1]);

    // Open: starts rolled up (~0 width), then unrolls to full width.
    t.click(hamburger);
    t.layout(SizeProposal::exact(800.0, 100.0));
    let just_opened = t.bounds(bar).width;
    t.tick_animations(std::time::Duration::from_millis(500));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let unrolled = t.bounds(bar).width;
    assert!(
        unrolled > just_opened + 1.0,
        "bar unrolls wider after the tween: {just_opened} -> {unrolled}"
    );

    // Close: the bar stays alive (rolling back) immediately after the
    // dismiss; it only goes dormant once the deferred tween completes.
    t.press_key(Key::Escape, Modifiers::NONE);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(
        t.is_active(bar),
        "bar stays active while rolling back on close"
    );
    t.advance_time(std::time::Duration::from_secs(1));
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(
        !t.is_active(bar),
        "bar dormant after the roll-back finishes"
    );
}

#[test]
fn collapsible_escape_hides_revealed_bar() {
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let children = t.children(mb);
    let (bar, hamburger) = (children[0], children[1]);

    t.click(hamburger);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(t.is_active(bar));

    t.press_key(Key::Escape, Modifiers::NONE);
    // The close rolls the bar back into the hamburger before tearing
    // down; advance past the tween so the deferred dismissal fires.
    t.advance_time(std::time::Duration::from_secs(1));
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(!t.is_active(bar), "Escape hides the revealed bar");
}

#[test]
fn collapsible_click_outside_hides_revealed_bar() {
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    let children = t.children(mb);
    let (bar, hamburger) = (children[0], children[1]);

    t.click(hamburger);
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(t.is_active(bar));

    // Click well below the top bar strip — outside the overlay.
    t.pointer_down_button(
        teksilo_canvas::Point::new(400.0, 400.0),
        teksilo_core::event::PointerButton::Primary,
    );
    // Advance past the roll-back tween so the deferred dismissal fires.
    t.advance_time(std::time::Duration::from_secs(1));
    t.layout(SizeProposal::exact(800.0, 100.0));
    assert!(!t.is_active(bar), "click outside hides the revealed bar");
}

#[test]
fn collapsible_bar_carries_menubar_role() {
    let mut t = collapsible_tree();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .collapsible(),
    );
    t.layout(SizeProposal::exact(800.0, 100.0));
    t.layout(SizeProposal::exact(800.0, 100.0));
    // In collapsible mode the MenuBar landmark moves onto the bar
    // content node (so it travels into the floating overlay and is
    // treated as a host surface). It is still reachable as a descendant.
    assert!(
        first_descendant_with_role(&t, mb, Role::MenuBar).is_some(),
        "the bar content node carries Role::MenuBar"
    );
}

#[test]
fn collapsible_dispatcher_injects_reveal_only_when_collapsed() {
    let collapsed = Signal::new(true);
    let reveal: MenubarReveal = std::rc::Rc::new(|_| {});
    let d = CollapsibleMenuBarDispatcher {
        inner: MenuBarDispatcher {
            trigger_ids: vec![fake_id(10)],
            mnemonic_table: HashMap::new(),
        },
        collapsed: collapsed.clone(),
        reveal,
    };

    let action = d.try_handle(&MenubarKeyEvent {
        key: Key::F10,
        modifiers: Modifiers::NONE,
    });
    assert!(
        matches!(
            action,
            Some(MenubarAction::FocusTrigger {
                reveal: Some(_),
                ..
            })
        ),
        "collapsed → reveal attached"
    );

    collapsed.set(false);
    let action = d.try_handle(&MenubarKeyEvent {
        key: Key::F10,
        modifiers: Modifiers::NONE,
    });
    assert!(
        matches!(
            action,
            Some(MenubarAction::FocusTrigger { reveal: None, .. })
        ),
        "expanded → no reveal (classic inline behaviour)"
    );
}

#[test]
fn from_model_builds_in_window_triggers() {
    use crate::menu::{MenuEntry, MenuModel};
    let model = MenuModel::new()
        .menu(lit!("&File"), |m| {
            m.item(MenuEntry::new(lit!("&New")).intent("app.new"))
                .separator()
                .item(MenuEntry::new(lit!("&Quit")).intent("app.quit"))
        })
        .menu(lit!("&Edit"), |m| {
            m.item(MenuEntry::new(lit!("Cu&t")).intent("app.cut"))
        });

    let mut t = tree_with_window();
    let mb = t.add(MenuBar::from_model(model));
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));

    // Two top-level menus → two triggers, names mnemonic-stripped.
    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
    assert_eq!(triggers.len(), 2);
    assert_eq!(t.accessibility_node(triggers[0]).name(), Some("File"));
    assert_eq!(t.accessibility_node(triggers[1]).name(), Some("Edit"));
}

#[test]
fn runtime_model_mutation_rebuilds_in_window_bar() {
    use crate::menu::{MenuEntry, MenuModel};
    let model = MenuModel::new().menu(lit!("&File"), |m| m.item(MenuEntry::new(lit!("&New"))));
    let model_handle = model.clone();

    let mut t = tree_with_window();
    let mb = t.add(MenuBar::from_model(model));
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert_eq!(
        collect_descendants_with_role(&t, mb, Role::MenuItem).len(),
        1
    );

    // Add a top-level menu at runtime → version bump → Rebuild binding →
    // the next layout re-derives the in-window triggers.
    model_handle.push_menu(lit!("&Edit"), |m| m.item(MenuEntry::new(lit!("Cu&t"))));
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert_eq!(
        collect_descendants_with_role(&t, mb, Role::MenuItem).len(),
        2
    );

    // Remove it again.
    let nodes_ids: Vec<_> = {
        model_handle
            .nodes()
            .iter()
            .filter_map(|n| match n {
                crate::menu::MenuNode::Submenu { id, title, .. }
                    if title.resolve_now().contains("Edit") =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect()
    };
    assert!(model_handle.remove(nodes_ids[0]));
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));
    assert_eq!(
        collect_descendants_with_role(&t, mb, Role::MenuItem).len(),
        1
    );
}

#[test]
fn native_suppress_hides_in_window_bar_on_macos() {
    use crate::menu::{MenuEntry, MenuModel, NativeMenuMode};
    let model = MenuModel::new().menu(lit!("&File"), |m| {
        m.item(MenuEntry::new(lit!("&New")).intent("app.new"))
    });
    let mut t = tree_with_window();
    let mb = t.add(MenuBar::from_model(model).native_on_macos(NativeMenuMode::Suppress));
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));

    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
    if cfg!(target_os = "macos") {
        // Suppressed: the OS menu bar carries the menus, no in-window triggers.
        assert!(
            triggers.is_empty(),
            "macOS Suppress renders no in-window triggers"
        );
    } else {
        // Other platforms ignore the flag and render the in-window bar.
        assert_eq!(triggers.len(), 1);
    }
}

/// End-to-end coverage of the model→native bridge (`menu::native::install`)
/// via the recording `MemoryNativeMenuBackend` — the testable half of the
/// native path (the `NSMenu` core itself needs a live AppKit loop). Verifies
/// title stripping, check state, the auto-injected localized app menu, and
/// that standard-menu labels go through i18n (no hardcoded English).
#[cfg(target_os = "macos")]
#[test]
fn native_install_records_localized_snapshot() {
    use crate::menu::{MenuEntry, MenuModel, NativeMenuMode, StandardMenu};
    use std::any::{Any, TypeId};
    use std::collections::HashMap;
    use std::sync::Arc;
    use teksilo_core::AppEventPoster;
    use teksilo_platform::native_menu::{
        MemoryNativeMenuBackend, NativeCheck, NativeMenuHandle, NativeMenuNode, StandardMenuRole,
    };

    struct NullPoster;
    impl AppEventPoster for NullPoster {
        fn post_subscription_event(&self, _: teksilo_core::SubscriptionId, _: Box<dyn Any + Send>) {
        }
        fn post_external(&self, _: Box<dyn Any + Send>) {}
    }

    let grid = Signal::new(true);
    let model = MenuModel::new()
        // App menu with a localized Quit — must NOT be hardcoded English.
        .standard_menu(StandardMenu::app().quit(lit!("Quitter")))
        .menu(lit!("&File"), |m| {
            m.item(MenuEntry::new(lit!("&New")).intent("app.new"))
                .item(MenuEntry::new(lit!("Show &Grid")).checkable(grid.clone()))
        });

    let backend = MemoryNativeMenuBackend::new();
    let handle = NativeMenuHandle::new(backend.clone());
    let mut app_state: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
    app_state.insert(TypeId::of::<NativeMenuHandle>(), Box::new(handle));
    let poster: Arc<dyn AppEventPoster> = Arc::new(NullPoster);

    let mut t = tree_with_window();
    t.set_app_context(Rc::new(
        teksilo_core::event_source::TreeAppContext::empty()
            .with_app_state(app_state)
            .with_poster(poster),
    ));
    let _mb = t.add(MenuBar::from_model(model).native_on_macos(NativeMenuMode::Coexist));
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 100.0));

    let snap = backend
        .menu_for(TeksiloWindowId::new(1))
        .expect("native snapshot recorded for the window");

    // App menu is first, with the localized Quit label (not "Quit"), and an
    // unrouted Quit — this model set no quit intent, so the item stays on
    // the platform's `terminate:` selector.
    match &snap.roots[0] {
        NativeMenuNode::Standard {
            role: StandardMenuRole::App,
            labels,
            quit_item,
            ..
        } => {
            assert_eq!(labels.quit, "Quitter", "Quit label routes through i18n");
            assert_eq!(labels.about, "About", "default About label resolved");
            assert!(quit_item.is_none(), "no quit intent declared, no routing");
        }
        other => panic!("expected leading App menu, got {other:?}"),
    }

    // File submenu: mnemonics stripped, checkable reflects the bound signal.
    let file = snap
        .roots
        .iter()
        .find_map(|n| match n {
            NativeMenuNode::Submenu { title, children } if title == "File" => Some(children),
            _ => None,
        })
        .expect("File submenu in snapshot");
    assert!(
        file.iter()
            .any(|n| matches!(n, NativeMenuNode::Item { title, .. } if title == "New")),
        "New item present, '&' stripped"
    );
    assert!(
        file.iter().any(|n| matches!(
            n,
            NativeMenuNode::Item { title, check: NativeCheck::On, .. } if title == "Show Grid"
        )),
        "checkable item reflects the bound signal (On) with stripped title"
    );
}

/// A bare focusable leaf, so a bar has somewhere to Tab *to*. The menu
/// tests need a destination outside the bar to tell "focus moved on" from
/// "focus went nowhere".
#[derive(Debug)]
struct FocusableLeaf;
impl Widget for FocusableLeaf {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(HandlerSet::new().focusable(true));
        vec![]
    }
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(12.0, 12.0).into()
    }
}

/// Tab is an *exit* gesture for a menu, not a navigation one.
///
/// ARIA APG's Menu pattern is unqualified about it: Tab "moves focus out
/// of the menu or menubar, and closes all menus and submenus". Only the
/// arrows navigate within. So one Tab must do two things — close the
/// dropdown, and leave focus past the trigger it belongs to. Focus landing
/// anywhere *behind* a still-open menu is WCAG 2.2 SC 2.4.11 (Focus Not
/// Obscured), and focus landing on an arbitrary widget decided by arena
/// insertion order is the same bug wearing a different hat.
#[test]
fn arrow_down_then_tab_closes_dropdown_and_lands_past_trigger() {
    let mut t = tree_with_window();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || {
                Box::new(
                    MenuList::new()
                        .item(MenuItem::new(lit!("New")))
                        .item(MenuItem::new(lit!("Open"))),
                )
            })
            .menu(lit!("&Edit"), || {
                Box::new(MenuList::new().item(MenuItem::new(lit!("Cut"))))
            }),
    );
    // A sibling root *after* the bar — the honest Tab destination.
    let after = t.add(FocusableLeaf);
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 600.0));

    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
    assert_eq!(triggers.len(), 2, "two top-level triggers");

    // ArrowDown on a focused trigger opens its dropdown.
    t.focus(triggers[0]);
    t.press_key(Key::ArrowDown, Modifiers::NONE);
    assert!(
        t.accessibility_node(triggers[0]).is_expanded(),
        "precondition: ArrowDown opens the File dropdown"
    );
    assert_ne!(
        t.focused(),
        Some(triggers[0]),
        "precondition: opening moves focus off the trigger, into the menu"
    );

    t.press_key(Key::Tab, Modifiers::NONE);

    // The load-bearing assertion. `is_expanded` only reflects
    // `MenuContext::open_index`, which `MenuOverlayHost`'s blur handler
    // clears on *any* FocusLost — so it goes false whether or not the
    // panel itself was actually taken down. Ask the overlay stack, or this
    // test passes with the focus-out rule entirely disabled and the
    // dropdown still on screen, which is precisely the WCAG 2.2 SC 2.4.11
    // failure it exists to catch.
    assert!(
        t.active_overlays().is_empty(),
        "the dropdown panel itself must not survive Tab"
    );
    assert!(
        !t.accessibility_node(triggers[0]).is_expanded(),
        "and the trigger must stop announcing itself as expanded"
    );
    assert_eq!(
        t.focused(),
        Some(after),
        "Tab must land on the first stop past the trigger"
    );
}

/// Sideways bar navigation keeps exactly one menu up, with focus in it.
///
/// `MenuContext::navigate` moves focus to the outgoing trigger and then
/// opens the next menu, queueing two `request_focus` calls on one
/// `EventContext` — only the last survives the drain — after an
/// `EventContext::dismiss_all_except_hosts`. That ordering is why the
/// focus-out rule cannot see this transition at all: the dismissal has
/// already cleared focus by the time any focus move reaches
/// `dismiss_overlays_left_by_focus`.
///
/// So this is a **guard, not a probe** — it passes with the focus-out rule
/// disabled, and is here to catch a future change that lets the rule reach
/// this path and eat the menu `navigate` just opened. Stated plainly so
/// nobody reads a green tick here as evidence the mechanism works; the
/// tests that actually exercise it are in `focus_impl.rs`'s
/// `tests_focus_out_dismissal` and `menu_list.rs`.
#[test]
fn sideways_navigate_does_not_orphan_focus() {
    let mut t = tree_with_window();
    let mb = t.add(
        MenuBar::new()
            .menu(lit!("&File"), || {
                Box::new(MenuList::new().item(MenuItem::new(lit!("New"))))
            })
            .menu(lit!("&Edit"), || {
                Box::new(MenuList::new().item(MenuItem::new(lit!("Cut"))))
            }),
    );
    t.layout(teksilo_canvas::SizeProposal::exact(800.0, 600.0));
    let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);

    t.focus(triggers[0]);
    t.press_key(Key::ArrowDown, Modifiers::NONE);
    assert!(t.accessibility_node(triggers[0]).is_expanded());

    t.press_key(Key::ArrowRight, Modifiers::NONE);
    assert!(
        t.accessibility_node(triggers[1]).is_expanded(),
        "ArrowRight must leave the Edit menu open, not dismissed by the focus-out rule"
    );
    assert!(
        !t.accessibility_node(triggers[0]).is_expanded(),
        "and must close the File menu it navigated away from"
    );
    assert_eq!(
        t.active_overlays().len(),
        1,
        "exactly one menu overlay stays up across sideways navigation"
    );
    // The orphan the name warns about: an open menu nobody is standing in.
    assert!(
        t.focused().is_some_and(|f| f != triggers[0]),
        "focus must land in the menu that was just opened, not be left behind"
    );
}
