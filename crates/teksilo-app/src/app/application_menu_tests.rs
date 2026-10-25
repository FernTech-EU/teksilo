// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The application menu in a headless application: declared in the
//! application tree, shown in a window's `MenuBar::application()`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::shortcut::{KeyStroke, Shortcut};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::{Action, BuildContext, Key, Modifiers, NoopWindowOps, WidgetId};
use teksilo_i18n::lit;
use teksilo_widgets::menu::{ApplicationMenuExt, MenuEntry, MenuModel};
use teksilo_widgets::{MenuBar, primitives::VStack};

use super::{HeadlessApp, TeksiloAppBuilder};
use crate::WindowConfig;

type Heard = Rc<RefCell<Vec<&'static str>>>;

const WINDOW: SizeProposal = SizeProposal {
    width: Some(800.0),
    height: Some(600.0),
};

/// The application menu the tests declare: File ▸ New (`app.new`, Ctrl+N),
/// File ▸ Recent (a closure that records where it runs, then `edit.copy`),
/// Edit ▸ Copy (`edit.copy`).
fn application_menu(heard: &Heard) -> MenuModel {
    let recent = heard.clone();
    MenuModel::new()
        .menu(lit!("File"), |m| {
            m.item(
                MenuEntry::new(lit!("New"))
                    .intent("app.new")
                    .shortcut("app.new"),
            )
            .item(
                MenuEntry::new(lit!("Recent"))
                    .intent("edit.copy")
                    .on_activate(move |ctx| {
                        recent.borrow_mut().push(if ctx.window().is_none() {
                            "recent runs in the application tree"
                        } else {
                            "recent runs in a window"
                        });
                    }),
            )
        })
        .menu(lit!("Edit"), |m| {
            m.item(MenuEntry::new(lit!("Copy")).intent("edit.copy"))
        })
}

/// The application tree's root: takes `app.new`, binds Ctrl+N to it, and
/// declares `model` as the application menu.
struct AppRoot {
    heard: Heard,
    model: MenuModel,
}

impl std::fmt::Debug for AppRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppRoot")
    }
}

impl Widget for AppRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let heard = self.heard.clone();
        ctx.register_action_global(Action::new("app.new").on_invoke(move |_, ctx| {
            assert!(ctx.window().is_none(), "in the application tree");
            heard
                .borrow_mut()
                .push("the application makes a new document");
        }));
        ctx.register_shortcut_global(
            Shortcut::new("app.new")
                .primary(KeyStroke::ctrl(Key::N))
                .build(),
        );
        ctx.set_application_menu(self.model.clone());
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// A focusable editor that takes `edit.copy`.
#[derive(Debug)]
struct Editor(Heard);

impl Widget for Editor {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let heard = self.0.clone();
        ctx.register_action(
            Action::new("edit.copy")
                .on_invoke(move |_, _| heard.borrow_mut().push("the editor copies")),
        );
        ctx.apply_self_handlers(HandlerSet::new().focusable(true));
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(400.0, 300.0).into()
    }
}

/// A window: the application bar over the editor.
#[derive(Debug)]
struct Window {
    heard: Heard,
    editor: Rc<Cell<Option<WidgetId>>>,
}

impl Widget for Window {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let editor = ctx.add(Editor(self.heard.clone()));
        self.editor.set(Some(editor));
        vec![ctx.add(VStack::new().child(MenuBar::application()).child(editor))]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(800.0, 600.0).into()
    }

    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }
}

struct Fixture {
    app: HeadlessApp,
    heard: Heard,
    model: MenuModel,
    editor: WidgetId,
}

/// An application whose application tree declares [`application_menu`],
/// with one window showing it, the editor focused.
fn fixture() -> Fixture {
    let heard = Heard::default();
    let model = application_menu(&heard);
    let editor = Rc::new(Cell::new(None));
    let mut app = TeksiloAppBuilder::new()
        .app_root({
            let heard = heard.clone();
            let model = model.clone();
            move |tree| tree.add(AppRoot { heard, model })
        })
        .initial_window(WindowConfig::new().root({
            let heard = heard.clone();
            let editor = editor.clone();
            move |tree, _state| tree.add(Window { heard, editor })
        }))
        .build_headless();
    app.tree.layout(WINDOW);
    let editor = editor.get().expect("the window was built");
    app.tree.focus(editor);
    Fixture {
        app,
        heard,
        model,
        editor,
    }
}

impl Fixture {
    /// Open the menu titled `menu` with a click and click its row `row`,
    /// then route what crosses between the trees.
    fn choose(&mut self, menu: &str, row: &str) {
        let trigger = self
            .app
            .tree
            .find_by_label(menu)
            .expect("the menu's trigger");
        self.app.tree.click(trigger);
        self.app.tree.layout(WINDOW);
        let row = self.app.tree.find_by_label(row).expect("the menu's row");
        self.app.tree.click(row);
        self.app.tree.layout(WINDOW);
        self.app.route_unclaimed_intents();
        self.app.tree.layout(WINDOW);
    }

    fn heard(&self) -> Vec<&'static str> {
        self.heard.borrow().clone()
    }
}

#[test]
fn a_windows_application_bar_shows_the_menu_the_application_tree_declared() {
    let f = fixture();
    assert!(f.app.tree.find_by_label("File").is_some());
    assert!(f.app.tree.find_by_label("Edit").is_some());
}

#[test]
fn a_change_to_the_application_menu_reaches_the_windows_bar() {
    let mut f = fixture();
    f.model
        .push_menu(lit!("View"), |m| m.item(MenuEntry::new(lit!("Zoom"))));
    f.app.tree.layout(WINDOW);
    assert!(
        f.app.tree.find_by_label("View").is_some(),
        "a model changed in place"
    );

    // Declared again, from a handler of the application tree.
    let app_tree = f.app.app_tree.as_mut().expect("an application tree");
    app_tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.set_application_menu(
            MenuModel::new().menu(lit!("Help"), |m| m.item(MenuEntry::new(lit!("About")))),
        );
    });
    f.app.tree.layout(WINDOW);
    assert!(f.app.tree.find_by_label("Help").is_some());
    assert!(f.app.tree.find_by_label("File").is_none(), "replaced");
}

#[test]
fn an_application_menu_row_is_taken_by_the_window_first_then_the_application() {
    let mut f = fixture();
    f.choose("Edit", "Copy");
    assert_eq!(f.heard(), ["the editor copies"]);
    assert_eq!(f.app.tree.focused(), Some(f.editor));
    f.choose("File", "New");
    assert_eq!(
        f.heard(),
        ["the editor copies", "the application makes a new document"]
    );
}

#[test]
fn an_application_menu_rows_closure_runs_in_the_application_tree_then_its_intent_in_the_window() {
    let mut f = fixture();
    let trigger = f.app.tree.find_by_label("File").expect("File");
    f.app.tree.click(trigger);
    f.app.tree.layout(WINDOW);
    let row = f.app.tree.find_by_label("Recent").expect("Recent");
    f.app.tree.click(row);
    f.app.tree.layout(WINDOW);
    assert!(f.heard().is_empty(), "nothing runs in the window");
    f.app.route_unclaimed_intents();
    assert_eq!(
        f.heard(),
        ["recent runs in the application tree", "the editor copies"]
    );
}

#[test]
fn a_shortcut_an_application_menu_row_names_works_in_a_window() {
    let mut f = fixture();
    assert!(
        f.app
            .tree
            .shortcut_registry()
            .get_default("app.new")
            .is_some(),
        "the window holds the application's Ctrl+N"
    );
    f.app.tree.press_key(Key::N, Modifiers::COMMAND);
    f.app.route_unclaimed_intents();
    assert_eq!(f.heard(), ["the application makes a new document"]);
}

/// A rebind of an application-menu shortcut in a window's `ShortcutSettings`
/// (which writes the window's registry) applies to the application: the
/// application tree's registry holds it, so every window's copy and the
/// native menu bar follow, and the new chord works in the window.
#[test]
fn a_rebind_in_a_window_applies_to_the_application() {
    let mut f = fixture();
    f.app
        .tree
        .run_with_event_context(&mut NoopWindowOps, |ctx| {
            ctx.rebind_shortcut_primary("app.new", Some(KeyStroke::ctrl(Key::M)));
        });
    f.app.route_unclaimed_intents();
    let app_tree = f.app.app_tree.as_ref().expect("an application tree");
    assert_eq!(
        app_tree
            .shortcut_registry()
            .effective("app.new")
            .and_then(|shortcut| shortcut.primary),
        Some(KeyStroke::ctrl(Key::M)),
        "the application tree's registry holds the rebind"
    );
    // A user's rebind is literal: Ctrl+M is the physical Control key on
    // macOS too.
    f.app.tree.press_key(Key::M, Modifiers::CTRL);
    f.app.route_unclaimed_intents();
    assert_eq!(f.heard(), ["the application makes a new document"]);
}

#[test]
fn an_application_bar_shows_nothing_when_no_application_menu_is_declared() {
    let heard = Heard::default();
    let editor = Rc::new(Cell::new(None));
    let mut app = TeksiloAppBuilder::new()
        .initial_window(WindowConfig::new().root({
            let editor = editor.clone();
            move |tree, _state| tree.add(Window { heard, editor })
        }))
        .build_headless();
    app.tree.layout(WINDOW);
    assert!(editor.get().is_some(), "the window was built");
    assert!(app.tree.find_by_label("File").is_none());
}

/// A window's tree that tries to declare the application menu.
#[derive(Debug)]
struct Declares;

impl Widget for Declares {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.set_application_menu(MenuModel::new());
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// A debug build refuses the call loudly, which this checks there; a
/// release build ignores it, which this checks there. That the call changes
/// nothing in either is `teksilo-widgets`'
/// `menu::application::tests::a_windows_declaration_changes_nothing`.
#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "from a window's tree"))]
fn a_window_cannot_declare_the_application_menu() {
    let app = TeksiloAppBuilder::new()
        .initial_window(WindowConfig::new().root(|tree, _state| tree.add(Declares)))
        .build_headless();
    let holder = app
        .tree
        .app_context()
        .app_state::<teksilo_widgets::menu::ApplicationMenu>()
        .cloned()
        .expect("every application has the holder");
    assert!(holder.model().is_none(), "nothing was declared");
}
