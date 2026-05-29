//! MenuBar — a horizontal menu bar with dropdown menus.
//!
//! # Bastyde
//! ```ignore
//! MenuBar::new()
//!     .menu(lit!("File"), || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new(lit!("New")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::New)))
//!             .separator()
//!             .item(MenuItem::new(lit!("Quit")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Quit)))
//!     ))
//!     .menu(lit!("Edit"), || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new(lit!("Cut")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Cut)))
//!             .item(MenuItem::new(lit!("Copy")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Copy)))
//!     ))
//!     .trailing_slot(Button::new(lit!("Settings")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Settings)))
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_core::window::{MenubarAction, MenubarDispatcher, MenubarGuard, MenubarKeyEvent};
use bastyde_tokens::{SurfaceRole, TextStyleRole};

use crate::menu_context::MenuContext;
use crate::menu_item::MenuLabel;
use crate::menu_item::ParsedMnemonic;
use crate::menu_item::parse_mnemonic;
use crate::primitives::{HStack, Padding, RectWidget, Spacer, ZStack};
use bastyde_i18n::LocalizedString;

// ---------------------------------------------------------------------------
// MenuBarEntry — pending menu definition
// ---------------------------------------------------------------------------

struct MenuBarEntry {
    label: LocalizedString,
    factory: Box<dyn Fn() -> Box<dyn Widget>>,
}

// ---------------------------------------------------------------------------
// MenuBar — public widget
// ---------------------------------------------------------------------------

/// A horizontal menu bar with dropdown menus.
///
/// Supports the Slot system (architecture Section 5.3):
/// - `leading_slot`: content before the menu buttons (e.g., app icon)
/// - `trailing_slot`: content after the menu buttons (e.g., search, user avatar)
pub struct MenuBar {
    entries: Vec<MenuBarEntry>,
    leading_slot: Vec<PendingChild>,
    trailing_slot: Vec<PendingChild>,
    root_child_id: Option<WidgetId>,
    /// Window-state guard for the per-window menubar key dispatcher
    /// (F10, Alt+letter, bare-Alt-tap). Owned by the MenuBar so the
    /// slot is cleared on rebuild / unmount.
    menubar_guard: RefCell<Option<MenubarGuard>>,
    /// When `true` (the default), `build()` installs a
    /// [`MenubarDispatcher`] into the window-state slot so this
    /// MenuBar receives F10 / Alt+letter / Alt-tap routing. Set to
    /// `false` via [`MenuBar::no_dispatcher_install`] for showcase /
    /// demo MenuBars that share a window with a primary one — the
    /// window-state slot is single-occupancy and a second install
    /// `debug_assert!`s otherwise.
    install_dispatcher: bool,
}

impl MenuBar {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            leading_slot: Vec::new(),
            trailing_slot: Vec::new(),
            root_child_id: None,
            menubar_guard: RefCell::new(None),
            install_dispatcher: true,
        }
    }

    /// Skip the window-state dispatcher install. The MenuBar still
    /// renders, intercepts mouse clicks, and supports keyboard
    /// navigation when its triggers have focus — only F10 /
    /// Alt+letter / Alt-tap routing through the window-level slot is
    /// disabled. Use this for demo / showcase MenuBars that share a
    /// window with a primary functional MenuBar — the slot is
    /// single-occupancy and a second install would `debug_assert!`.
    pub fn no_dispatcher_install(mut self) -> Self {
        self.install_dispatcher = false;
        self
    }

    pub fn menu(
        mut self,
        label: impl Into<LocalizedString>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        let ls: LocalizedString = label.into();
        self.entries.push(MenuBarEntry {
            label: ls,
            factory: Box::new(factory),
        });
        self
    }

    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.leading_slot
            .push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot
            .push(PendingChild::Deferred(Box::new(widget)));
        self
    }
}

impl Default for MenuBar {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MenuBarDispatcher — window-level F10 / Alt+letter / Alt-tap handler
// ---------------------------------------------------------------------------

/// `MenubarDispatcher` impl backed by the live trigger ids and
/// mnemonic table from the most recent `MenuBar::build`.
struct MenuBarDispatcher {
    /// All top-level trigger ids, in declaration order.
    trigger_ids: Vec<WidgetId>,
    /// Lower-cased mnemonic char → trigger array index.
    mnemonic_table: HashMap<char, usize>,
}

impl MenubarDispatcher for MenuBarDispatcher {
    fn try_handle(&self, event: &MenubarKeyEvent) -> Option<MenubarAction> {
        // F10 (no modifiers): focus the first trigger without
        // opening any menu — matches Win32 / GTK F10 behaviour.
        // Works on every platform (F10 is not transformed by any OS
        // input layer the way Alt+letter is on macOS).
        if event.modifiers == Modifiers::NONE && matches!(event.key, Key::F10) {
            return self
                .trigger_ids
                .first()
                .map(|&id| MenubarAction::FocusTrigger { trigger_id: id });
        }
        // Alt+<letter> mnemonics. On macOS, Option+letter is
        // intercepted by the OS to compose accented characters
        // (Option+E -> ´, Option+F -> ƒ, …) *before* winit sees the
        // keystroke. The app receives the post-composition character
        // (`ƒ`), not the typed letter (`F`), so the mnemonic table
        // can never match. Worse, returning `Intercept` here would
        // silently swallow legitimate accented text input. Skip the
        // entire branch on macOS — F10 + Alt-tap + in-menu
        // bare-letter activation cover the macOS menu-keyboard
        // story instead.
        #[cfg(not(target_os = "macos"))]
        if event.modifiers == Modifiers::ALT {
            // Strict per-OS contract — `Alt+letter` is reserved for
            // menu mnemonics on Win32 / GTK and must be intercepted
            // even when nothing matches, so the chord doesn't
            // appear as garbled text input in a focused text field.
            let lookup_char = match event.key {
                Key::Character(c) => Some(c.to_ascii_lowercase()),
                _ => {
                    let c = event.key.to_char()?;
                    Some(c.to_ascii_lowercase())
                }
            };
            if let Some(c) = lookup_char {
                if let Some(&idx) = self.mnemonic_table.get(&c) {
                    if let Some(&tid) = self.trigger_ids.get(idx) {
                        return Some(MenubarAction::OpenMenu { trigger_id: tid });
                    }
                }
                // Letter-with-Alt that doesn't match any mnemonic —
                // intercept silently so the chord doesn't leak into
                // focused text input as garbled chars.
                return Some(MenubarAction::Intercept);
            }
        }
        // Suppress an unused-warning on macOS where the Alt branch
        // above is compiled out.
        let _ = &self.mnemonic_table;
        None
    }

    fn on_alt_tap(&self) -> Option<MenubarAction> {
        // Bare-Alt-tap (no other key during the hold) → focus the
        // first trigger in menubar-active mode (no menu opens until
        // ArrowDown / Enter / Space).
        self.trigger_ids
            .first()
            .map(|&id| MenubarAction::FocusTrigger { trigger_id: id })
    }
}

impl std::fmt::Debug for MenuBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuBar")
            .field("entries", &self.entries.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// MenuBarTrigger — internal trigger label
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct MenuBarTrigger {
    label: LocalizedString,
    /// Mnemonic-stripped label name used for `AccessNodeBuilder::set_name`.
    /// Captured from the parsed label so screen readers announce "File",
    /// not "ampersand-File". Set in `build()`.
    stripped_name: String,
    /// Mnemonic letter (lowercase) for AT `set_access_key` annotation.
    /// `None` for triggers whose label carries no un-escaped `&`.
    mnemonic_key: Option<char>,
    index: usize,
    menu_ctx: MenuContext,
    root_child_id: Option<WidgetId>,
}

impl Widget for MenuBarTrigger {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme();
        let radius_control = theme.shape.radius_control;
        use crate::styles::recipe_menu_item_style as menu;
        let index = self.index;
        let menu_ctx = self.menu_ctx.clone();

        // Background role: `AccentSubtle` when open (the Int UI token for
        // highlighted menu-bar entries) or `Transparent` at rest. Replaces
        // the previous hand-mixed `accent.with_alpha(0.12)` wash.
        let bg_role = menu_ctx.open_index.map(move |open| {
            if *open == Some(index) {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Transparent
            }
        });

        // Text color can't collapse to a pure role: the at-rest state is
        // `text_primary.with_alpha(0.8)` (dimmed primary — distinct from
        // TextRole::Secondary, which is a different hue). Keep a direct
        // `theme_signal` map for the blended case.
        let theme_signal = ctx.theme_signal();
        let text_color = menu_ctx
            .open_index
            .zip(&theme_signal)
            .map(move |(open, t)| {
                if *open == Some(index) {
                    t.colors.text_primary
                } else {
                    t.colors.text_primary.with_alpha(0.8)
                }
            });

        // Label. Uses `MenuLabel` so a single `&` in the trigger
        // string acts as a mnemonic marker — stripped from the
        // visible text and underlined when the window's `alt_down`
        // signal is true.
        let alt_down = ctx
            .window()
            .map(|w| w.alt_down().clone())
            .unwrap_or_else(|| Signal::new(false));
        let label_source: bastyde_core::signal::Prop<String> = self.label.clone().into();
        let label_id = ctx.add(MenuLabel::new(
            label_source,
            alt_down,
            text_color,
            TextStyleRole::Small,
        ));

        let padding =
            Padding::symmetric(4.0, menu::MENU_ITEM_PADDING_HORIZONTAL).child_id(label_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new()
            .bind_background(bg_role)
            .corner_radius(bastyde_tokens::CornerRadius::uniform(radius_control));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let handler_set = HandlerSet::new()
            .on_tap({
                let menu_ctx = menu_ctx.clone();
                move |_pos, ctx: &mut EventContext| {
                    if menu_ctx.open_index.get() == Some(index) {
                        menu_ctx.close(ctx);
                    } else {
                        menu_ctx.open_at(index, ctx);
                    }
                }
            })
            .on_hover({
                let menu_ctx = menu_ctx.clone();
                move |entered: bool, ctx: &mut EventContext| {
                    if entered {
                        // If another menu is open, switch immediately (no delay)
                        let current = menu_ctx.open_index.get();
                        if current.is_some() && current != Some(index) {
                            menu_ctx.open_at(index, ctx);
                        }
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown | Key::Enter | Key::Space,
                            ..
                        } => {
                            menu_ctx.open_at(index, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            menu_ctx.navigate(-1, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            menu_ctx.navigate(1, ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        // Re-query accessibility when this trigger's open/closed state flips so
        // `set_expanded` stays in sync with the open menu index.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.menu_ctx.open_index.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 28.0)),
            None => proposal.resolve(60.0, 28.0),
        }
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
        builder.set_role(bastyde_core::accesskit::Role::MenuItem);
        // Stripped name — set in `build()` from the parsed mnemonic.
        // Falls back to a fresh resolve if the trigger has not been
        // built yet (rare; AT walks always happen post-build).
        if !self.stripped_name.is_empty() {
            builder.set_name(self.stripped_name.clone());
        } else {
            builder.set_name(parse_mnemonic(&self.label.resolve_now()).stripped);
        }
        // Every top-level menu bar entry opens a dropdown Menu.
        builder.set_has_popup(bastyde_core::accesskit::HasPopup::Menu);
        builder.set_expanded(self.menu_ctx.open_index.get() == Some(self.index));
        // Mnemonic — announced by Windows Narrator as "Access key: F".
        if let Some(k) = self.mnemonic_key {
            builder
                .inner_mut()
                .set_access_key(k.to_ascii_uppercase().to_string());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// MenuOverlayHost — wraps dropdown content, handles focus + cross-menu keys
// ---------------------------------------------------------------------------

/// Wraps dropdown menu content (typically a MenuList). Responsibilities:
/// - Resets `open_index` when focus is lost (overlay dismissed)
/// - Handles ArrowLeft/Right for cross-menu navigation (bubbles up from MenuList)
#[derive(Debug)]
struct MenuOverlayHost {
    inner: Option<Box<dyn Widget>>,
    menu_ctx: MenuContext,
    menu_index: usize,
    inner_id: Option<WidgetId>,
}

impl Widget for MenuOverlayHost {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let inner_widget = self.inner.take().expect("MenuOverlayHost built twice");
        let id = ctx.add_boxed(inner_widget);
        self.inner_id = Some(id);

        // Register inner widget as the focus target for this menu index
        self.menu_ctx.set_focus_id(self.menu_index, id);

        let menu_ctx = self.menu_ctx.clone();
        let menu_index = self.menu_index;
        let handler_set = HandlerSet::new()
            .on_focus({
                let menu_ctx = menu_ctx.clone();
                move |gained: bool, ctx: &mut EventContext| {
                    if !gained && menu_ctx.open_index.get() == Some(menu_index) {
                        // Overlay was dismissed — close the menu and restore focus
                        menu_ctx.close(ctx);
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    // These keys bubble up from the inner MenuList when it returns Ignored
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            menu_ctx.navigate(-1, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            menu_ctx.navigate(1, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Escape, ..
                        } => {
                            menu_ctx.close(ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            });
        // NOT focusable — the inner MenuList receives focus directly.
        // ArrowLeft/Right and FocusLost bubble from MenuList through here.
        ctx.apply_self_handlers(handler_set);

        vec![id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.inner_id
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
        // The inner widget (typically `MenuList`) owns the `Role::Menu`
        // semantics. A second Menu role here would nest two Menu nodes
        // per dropdown, confusing screen readers that look for a single
        // Menu per popup. `GenericContainer` is the ARIA `none`/`presentation`
        // equivalent: the host is kept in the tree for focus/key routing
        // but is ignored by assistive tech.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.inner_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// MenuBar Widget impl
// ---------------------------------------------------------------------------

impl Widget for MenuBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme_signal = ctx.theme_signal();

        let open_index: Signal<Option<usize>> = ctx.signal(None);
        let menu_ctx = MenuContext::new(open_index);

        // Build the full row: [leading_slot | triggers... | Spacer | trailing_slot]
        let mut row = HStack::new().spacing(2.0);

        // Leading slot
        for pending in self.leading_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }

        // Menu triggers + content
        let mut trigger_ids = Vec::new();
        let mut content_ids = Vec::new();
        // Mnemonic table built alongside triggers: `lowercase char →
        // trigger array index`. Drives the window-level dispatcher
        // for Alt+letter activation.
        let mut mnemonic_table: HashMap<char, usize> = HashMap::new();

        let entries = std::mem::take(&mut self.entries);
        for (i, entry) in entries.into_iter().enumerate() {
            let parsed: ParsedMnemonic = parse_mnemonic(&entry.label.resolve_now());

            // Wrap factory output in MenuOverlayHost for focus/key handling
            let host = MenuOverlayHost {
                inner: Some((entry.factory)()),
                menu_ctx: menu_ctx.clone(),
                menu_index: i,
                inner_id: None,
            };
            let content_id = ctx.add(host);
            ctx.set_dormant(content_id);

            let trigger = MenuBarTrigger {
                label: entry.label,
                stripped_name: parsed.stripped.clone(),
                mnemonic_key: parsed.key_lower,
                index: i,
                menu_ctx: menu_ctx.clone(),
                root_child_id: None,
            };
            let trigger_id = ctx.add(trigger);
            row = row.add_child(trigger_id);

            if let Some(k) = parsed.key_lower {
                if let Some(prev) = mnemonic_table.insert(k, i) {
                    debug_assert!(
                        false,
                        "MenuBar: duplicate mnemonic {:?} (triggers {} and {})",
                        k, prev, i
                    );
                }
            }

            trigger_ids.push(trigger_id);
            content_ids.push(content_id);
        }

        // Register all trigger/content IDs in the context.
        // focus_id is initially content_id; MenuOverlayHost::build() will
        // overwrite it with the actual inner MenuList ID.
        for (i, (&tid, &cid)) in trigger_ids.iter().zip(content_ids.iter()).enumerate() {
            menu_ctx.register(i, tid, cid, cid);
        }

        // Spacer pushes triggers left, trailing slot right
        row = row.child(Spacer::new());

        // Trailing slot
        for pending in self.trailing_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }

        let row_id = ctx.add(row);

        let bg = RectWidget::new()
            .background(SurfaceRole::Main)
            .bind_border_color(theme_signal.map(|t| t.colors.border.with_alpha(0.2)))
            .bind_border_width(0.0_f32);
        let bg_id = ctx.add(bg);

        let padding = Padding::symmetric(0.0, 2.0).child_id(row_id);
        let padding_id = ctx.add(padding);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        // Window-level menubar key dispatcher (F10 / Alt+letter /
        // Alt-tap). Installed on every platform — `MenuBar` is an
        // in-window widget menu, not the OS system menu, so the
        // dispatcher's job is to wire framework menus to keyboard
        // accelerators regardless of host OS.
        //
        // **macOS**: the dispatcher's `Alt+letter` branch is compiled
        // out (see `MenuBarDispatcher::try_handle`) because the OS
        // rewrites Option+letter for accented character composition
        // before the app sees the keystroke. F10 and bare-Alt-tap
        // continue to fire on macOS through this same dispatcher.
        //
        // Drop the previous guard BEFORE installing the new one so
        // the slot is empty when `install_menubar_dispatcher` runs
        // its `debug_assert!(slot.is_none())`. Otherwise a rebuild
        // of `MenuBar` (e.g. when a composing ancestor rebuilds)
        // trips the assert in debug builds and would over-write the
        // slot under another live guard in release.
        if self.install_dispatcher
            && let Some(window) = ctx.window()
        {
            *self.menubar_guard.borrow_mut() = None;
            let dispatcher: Rc<dyn MenubarDispatcher> = Rc::new(MenuBarDispatcher {
                trigger_ids: trigger_ids.clone(),
                mnemonic_table,
            });
            let guard = window.install_menubar_dispatcher(dispatcher);
            *self.menubar_guard.borrow_mut() = Some(guard);
        }

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => {
                let content_proposal = SizeProposal {
                    width: proposal.width,
                    height: None,
                };
                let size = ctx
                    .child_size(id, content_proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(proposal.width.unwrap_or(size.width), size.height)
            }
            None => proposal.resolve(0.0, 0.0),
        }
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
        builder.set_role(bastyde_core::accesskit::Role::MenuBar);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_list::MenuList;
    use bastyde_core::accesskit::Role;
    use bastyde_core::widget_id::WidgetId;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_core::window::state::WindowStateInit;
    use bastyde_core::window::{BastydeWindowId, WindowPlacement, WindowState};
    use bastyde_i18n::lit;

    fn tree_with_window() -> WidgetTree {
        let mut t = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        t.set_window_state(WindowState::new(WindowStateInit {
            id: BastydeWindowId::new(1),
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

    #[test]
    fn menubar_emits_role_menubar() {
        let mut t = tree_with_window();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new())),
        );
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
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
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
        assert_eq!(triggers.len(), 2);
        // The stripped name "File" / "Edit", NOT "&File" / "&Edit".
        let info0 = t.accessibility_node(triggers[0]);
        let info1 = t.accessibility_node(triggers[1]);
        assert_eq!(info0.name(), Some("File"));
        assert_eq!(info1.name(), Some("Edit"));
    }

    #[test]
    fn dispatcher_installed_on_every_platform() {
        let mut t = tree_with_window();
        t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new())),
        );
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
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
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        assert!(t.window_state().unwrap().menubar_dispatcher().is_some());
        // Force a rebuild and confirm the dispatcher install path
        // doesn't crash (debug builds) or silently overwrite a live
        // guard (release builds).
        t.arena_mark_needs_rebuild_for_testing(mb);
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        assert!(
            t.window_state().unwrap().menubar_dispatcher().is_some(),
            "after rebuild the dispatcher slot must still point at \
             the most-recently-installed dispatcher"
        );
    }

    #[test]
    fn windowstate_dispatcher_slot_reinstall_after_guard_drop() {
        // Direct unit test of the WindowState slot lifecycle —
        // installing a second dispatcher after dropping the first
        // guard must succeed without a debug_assert.
        use bastyde_core::window::{MenubarAction, MenubarDispatcher, MenubarKeyEvent};

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
    /// `bastyde-core`'s signal / overlay tests.
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
            Some(MenubarAction::FocusTrigger { trigger_id }) if trigger_id == fake_id(10)
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
            Some(MenubarAction::OpenMenu { trigger_id }) if trigger_id == fake_id(10)
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
            Some(MenubarAction::FocusTrigger { trigger_id }) if trigger_id == fake_id(10)
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
}
