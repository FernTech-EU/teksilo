// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The application menu: one [`MenuModel`] for the whole application,
//! declared in the application tree.
//!
//! The application tree (`TeksiloAppBuilder::app_root`) declares it from its
//! build with [`set_application_menu`](ApplicationMenuExt::set_application_menu).
//! A window shows it in its own bar with
//! [`MenuBar::application`](crate::menu_bar::MenuBar::application), or shows
//! a menu of its own instead with
//! [`MenuBar::from_model`](crate::menu_bar::MenuBar::from_model). On macOS it
//! is the global menu bar whenever the key window has no menu of its own, and
//! while no window is open; on Windows and Linux it appears only in the
//! windows whose bar asks for it.
//!
//! Its rows are application commands. A row with an
//! [`intent`](crate::menu::MenuEntry::intent) sends it from the focus of the
//! window it was chosen in, as any menu row does, so an action of that window
//! takes it (Edit ▸ Copy copies in the focused field) and the application
//! tree's actions take what the window leaves. A row with an
//! [`on_activate`](crate::menu::MenuEntry::on_activate) closure runs it in the
//! application tree, where the signals it captured live, then sends its
//! intent, if it has one, from the window's focus. A shortcut a row names
//! works in every window: `teksilo-app` registers the application tree's
//! declaration of it in each window's shortcut registry.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use std::sync::Arc;

use teksilo_core::AppEventPoster;
use teksilo_core::build_context::BuildContext;
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::signal::Signal;
use teksilo_core::widget::EventContext;
use teksilo_core::window::TeksiloWindowId;
use teksilo_platform::native_menu::{MenuActionFn, NativeMenuHandle};

use super::model::{MenuModel, MenuNode};

/// The application menu's holder, shared by the application tree and every
/// window through app-state. `teksilo-app` registers one in every
/// application; apps reach it through
/// [`ApplicationMenuExt::set_application_menu`] and
/// [`MenuBar::application`](crate::menu_bar::MenuBar::application).
#[doc(hidden)]
#[derive(Clone)]
pub struct ApplicationMenu {
    inner: Rc<Inner>,
}

impl Default for ApplicationMenu {
    fn default() -> Self {
        Self {
            inner: Rc::new(Inner {
                model: RefCell::new(None),
                generation: Signal::new(0),
                pending: RefCell::new(Vec::new()),
                warned: Cell::new(false),
                native: RefCell::new(None),
                redeclared: RefCell::new(RedeclareWatch::default()),
            }),
        }
    }
}

struct Inner {
    model: RefCell<Option<MenuModel>>,
    /// Bumped by every [`ApplicationMenu::set`], so a bar showing the
    /// application menu rebuilds when the application declares another.
    generation: Signal<u64>,
    /// Rows chosen in a window whose closure runs in the application tree.
    pending: RefCell<Vec<ApplicationMenuActivation>>,
    /// Whether a bar has already said that no application menu is declared.
    warned: Cell<bool>,
    /// The native application menu installed last, with what it was
    /// installed from: [`ApplicationMenu::revision`] and the application
    /// tree's shortcut registry version.
    native: RefCell<Option<NativeInstall>>,
    /// How often another model was declared lately: see [`RedeclareWatch`].
    redeclared: RefCell<RedeclareWatch>,
}

/// Notices an application menu declared again and again with another model,
/// which is what a model built inline in a build that runs often does: every
/// declaration rebuilds every bar showing it and installs the native menu
/// again. Says so once, in a debug build, past
/// [`LIMIT`](Self::LIMIT) new declarations within [`SPAN`](Self::SPAN).
#[derive(Default)]
struct RedeclareWatch {
    /// When the current run of declarations started, and how many it holds.
    run: Option<(std::time::Instant, u32)>,
    warned: bool,
}

impl RedeclareWatch {
    /// More new declarations than this within [`SPAN`](Self::SPAN) is a
    /// model built where it is declared. A few are normal: the application
    /// may change its menu, and the first layouts rebuild the tree.
    const LIMIT: u32 = 4;
    const SPAN: std::time::Duration = std::time::Duration::from_secs(10);

    /// Note a new declaration at `now`; true, once, when it should be said.
    fn note(&mut self, now: std::time::Instant) -> bool {
        let count = match self.run {
            Some((start, count)) if now.duration_since(start) < Self::SPAN => count + 1,
            _ => {
                self.run = Some((now, 1));
                1
            }
        };
        if let Some(run) = self.run.as_mut() {
            run.1 = count;
        }
        if count > Self::LIMIT && !self.warned {
            self.warned = true;
            return true;
        }
        false
    }
}

/// What [`ApplicationMenu::sync_native`] installed, and from what.
struct NativeInstall {
    source: (u64, u64, u64),
    _binding: super::native::NativeMenuBinding,
}

/// A row of the application menu chosen in a window, whose closure runs in
/// the application tree: `teksilo-app` runs [`action`](Self::action) there,
/// then sends [`intent`](Self::intent) from the focus of
/// [`window`](Self::window), or in the application tree when that window is
/// gone or none was named.
#[doc(hidden)]
#[derive(Clone)]
pub struct ApplicationMenuActivation {
    /// The window the row was chosen in.
    pub window: Option<TeksiloWindowId>,
    /// The intent the row sends after its action.
    pub intent: Option<&'static str>,
    /// The row's closure, with the state write a check or radio row makes.
    pub action: Option<MenuActionFn>,
}

impl std::fmt::Debug for ApplicationMenuActivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApplicationMenuActivation")
            .field("window", &self.window)
            .field("intent", &self.intent)
            .field("action", &self.action.as_ref().map(|_| "<closure>"))
            .finish()
    }
}

impl ApplicationMenu {
    /// An empty holder: no application menu declared.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare `model` as the application menu, replacing any earlier one.
    /// The model already declared (the same handle, as a rebuild of the
    /// application tree's root declares it again) changes nothing. Another
    /// handle, even with equal content, is a new declaration: a debug build
    /// says so when that keeps happening (a model built where it is
    /// declared).
    pub fn set(&self, model: MenuModel) {
        if self
            .inner
            .model
            .borrow()
            .as_ref()
            .is_some_and(|declared| declared.same(&model))
        {
            return;
        }
        model.warn_second_app_menu();
        if self
            .inner
            .redeclared
            .borrow_mut()
            .note(std::time::Instant::now())
        {
            #[cfg(debug_assertions)]
            eprintln!(
                "teksilo: the application menu was declared again with another MenuModel {} \
                 times within {} s: every declaration rebuilds every bar showing it and \
                 installs the native menu again. Build the model once (in the application \
                 root's constructor, say), keep it, and declare a clone of that handle; change \
                 it with its mutators.",
                RedeclareWatch::LIMIT + 1,
                RedeclareWatch::SPAN.as_secs(),
            );
        }
        *self.inner.model.borrow_mut() = Some(model);
        let generation = self.inner.generation.get();
        self.inner.generation.set(generation.wrapping_add(1));
    }

    /// The application menu, if the application declared one.
    pub fn model(&self) -> Option<MenuModel> {
        self.inner.model.borrow().clone()
    }

    /// Bumped whenever [`set`](Self::set) declares a menu. A change *inside*
    /// the declared model bumps its own [`MenuModel::version`] instead.
    pub fn generation(&self) -> Signal<u64> {
        self.inner.generation.clone()
    }

    /// What identifies the menu as it is now: the generation and the
    /// declared model's version. Two equal answers mean nothing changed.
    pub fn revision(&self) -> (u64, u64) {
        let version = self
            .inner
            .model
            .borrow()
            .as_ref()
            .map_or(0, |model| model.version().get());
        (self.inner.generation.get(), version)
    }

    /// Every shortcut id a row of the application menu names, in tree
    /// order, each once: its entries' [`shortcut`](crate::menu::MenuEntry::shortcut)
    /// ids and the routed Quit and Settings rows' of its App menu.
    pub fn shortcut_ids(&self) -> Vec<&'static str> {
        fn walk(nodes: &[MenuNode], ids: &mut Vec<&'static str>) {
            for node in nodes {
                let found: Vec<&'static str> = match node {
                    MenuNode::Item(entry) => entry.shortcut_id.into_iter().collect(),
                    MenuNode::Submenu { children, .. } => {
                        walk(children, ids);
                        Vec::new()
                    }
                    MenuNode::Standard(menu) => menu
                        .quit_shortcut_id()
                        .into_iter()
                        .chain(menu.settings_shortcut_id())
                        .collect(),
                    MenuNode::Separator => Vec::new(),
                };
                for id in found {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
        }
        let mut ids = Vec::new();
        if let Some(model) = self.inner.model.borrow().as_ref() {
            walk(&model.nodes(), &mut ids);
        }
        ids
    }

    /// Install the application menu into the native menu bar through
    /// `handle`, unless what it would install from has not changed since
    /// the last call: the menu (or, with none declared, a default App menu
    /// whose Quit asks the application to quit), its key equivalents looked
    /// up with `shortcut` in the application tree's registry, whose version
    /// is `registry_version`. `teksilo-app` calls it after every turn.
    pub fn sync_native(
        &self,
        handle: &NativeMenuHandle,
        poster: Arc<dyn AppEventPoster>,
        registry_version: u64,
        shortcut: &dyn Fn(&str) -> Option<KeyStroke>,
    ) {
        let (generation, version) = self.revision();
        let source = (generation, version, registry_version);
        if self
            .inner
            .native
            .borrow()
            .as_ref()
            .is_some_and(|installed| installed.source == source)
        {
            return;
        }
        let model = self.model();
        let binding = super::native::install_application(model.as_ref(), handle, poster, shortcut);
        *self.inner.native.borrow_mut() = Some(NativeInstall {
            source,
            _binding: binding,
        });
    }

    /// Keep a row chosen in a window for the application tree to run.
    pub fn queue(&self, activation: ApplicationMenuActivation) {
        self.inner.pending.borrow_mut().push(activation);
    }

    /// Whether a row chosen in a window waits for the application tree.
    pub fn has_pending(&self) -> bool {
        !self.inner.pending.borrow().is_empty()
    }

    /// Take the rows chosen in a window since the last call, oldest first.
    pub fn take_pending(&self) -> Vec<ApplicationMenuActivation> {
        std::mem::take(&mut *self.inner.pending.borrow_mut())
    }

    /// Say once per process, in a debug build, that a bar asked for the
    /// application menu in a tree that belongs to no application: no holder
    /// in its app state (a test, a tree built by hand), so each build would
    /// otherwise make a fresh one and say it again.
    pub(crate) fn warn_no_holder() {
        static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
            #[cfg(test)]
            UNDECLARED_SAID.with(|said| said.set(said.get() + 1));
            #[cfg(debug_assertions)]
            eprintln!(
                "teksilo: MenuBar::application() shows nothing: this tree belongs to no \
                 application, so it has no application menu (teksilo-app registers its \
                 holder in every application)."
            );
        }
    }

    /// Say once, in a debug build, that a bar asked for the application
    /// menu and the application declared none.
    pub(crate) fn warn_undeclared(&self) {
        if !self.inner.warned.replace(true) {
            #[cfg(test)]
            UNDECLARED_SAID.with(|said| said.set(said.get() + 1));
            #[cfg(debug_assertions)]
            eprintln!(
                "teksilo: MenuBar::application() shows nothing: the application declared no \
                 application menu. Call ctx.set_application_menu(model) from the build of the \
                 root given to TeksiloAppBuilder::app_root."
            );
        }
    }
}

#[cfg(test)]
thread_local! {
    /// How many times [`ApplicationMenu::warn_undeclared`] spoke on this
    /// thread, for the tests.
    pub(crate) static UNDECLARED_SAID: Cell<u32> = const { Cell::new(0) };
}

impl std::fmt::Debug for ApplicationMenu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApplicationMenu")
            .field("declared", &self.inner.model.borrow().is_some())
            .field("pending", &self.inner.pending.borrow().len())
            .finish()
    }
}

/// Declare the application menu: `ctx.set_application_menu(model)`.
///
/// Call it from the application tree, the tree of the root given to
/// `TeksiloAppBuilder::app_root`, usually from that root's build. The menu
/// replaces any declared before it, and every window's
/// [`MenuBar::application`](crate::menu_bar::MenuBar::application) and the
/// macOS menu bar follow; so do they when the model changes
/// ([`MenuModel::push_item`] and the other mutators).
///
/// What the application tree registers with
/// [`register_action`](BuildContext::register_action) or
/// [`register_action_global`](BuildContext::register_action_global) is the
/// application's action scope: the intents of the menu's rows that no window
/// takes go there. A shortcut a row names, registered in the application
/// tree with `register_shortcut_global`, works in every window.
///
/// Called from a window's tree it does nothing, and a debug build panics:
/// a window shows a menu of its own with
/// [`MenuBar::from_model`](crate::menu_bar::MenuBar::from_model).
///
/// Build the model once and declare a clone of that handle, as below: the
/// same handle declared again changes nothing, but a model built where it
/// is declared is a new one at every build of the root, and every bar
/// showing it rebuilds and the macOS menu is installed again each time (a
/// debug build says so when it keeps happening). Change a declared menu with
/// its mutators ([`MenuModel::push_item`] and the others) or its entries'
/// signals.
///
/// ```ignore
/// use teksilo::prelude::*;
/// use teksilo::widgets::menu::{MenuEntry, MenuModel, StandardMenu};
///
/// struct AppRoot {
///     menu: MenuModel,
/// }
///
/// impl AppRoot {
///     fn new() -> Self {
///         let menu = MenuModel::new()
///             .standard_menu(StandardMenu::app().title(tr!(app_name())).quit_intent("app.quit"))
///             .menu(tr!(file()), |m| m.item(MenuEntry::new(tr!(new())).intent("app.new").shortcut("app.new")));
///         Self { menu }
///     }
/// }
///
/// impl Widget for AppRoot {
///     fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
///         ctx.register_action_global(Action::new("app.new").on_invoke(|_, ctx| open_document(ctx)));
///         ctx.set_application_menu(self.menu.clone());
///         vec![]
///     }
/// }
/// ```
pub trait ApplicationMenuExt {
    /// Declare `model` as the application menu. See [`ApplicationMenuExt`].
    ///
    /// Pass a clone of a model built once and kept (a field of the
    /// application root): declaring that same handle again from a later
    /// build changes nothing, while a model built here would be a new
    /// declaration at every build, rebuilding every bar that shows it.
    fn set_application_menu(&mut self, model: MenuModel);
}

/// What [`try_declare`] did with a menu.
#[derive(Debug, PartialEq, Eq)]
enum Declared {
    /// Declared: it is the application menu.
    Set,
    /// Nothing: called from a window's tree.
    FromWindow,
    /// Nothing: the tree belongs to no application.
    NoHolder,
}

/// Declare `model` in `holder` unless the call comes from a window's tree
/// or there is no holder, and say which. The debug assertion is the
/// caller's ([`declare`]), so a test can check that a window's call changes
/// nothing.
fn try_declare(in_window: bool, holder: Option<&ApplicationMenu>, model: MenuModel) -> Declared {
    if in_window {
        return Declared::FromWindow;
    }
    match holder {
        Some(holder) => {
            holder.set(model);
            Declared::Set
        }
        None => Declared::NoHolder,
    }
}

/// The one implementation behind both contexts: [`try_declare`], which a
/// debug build refuses loudly from a window's tree.
fn declare(in_window: bool, holder: Option<&ApplicationMenu>, model: MenuModel) {
    match try_declare(in_window, holder, model) {
        Declared::Set => {}
        Declared::FromWindow => debug_assert!(
            false,
            "set_application_menu called from a window's tree: declare the application menu \
             from the application tree (TeksiloAppBuilder::app_root), and give a window a menu \
             of its own with MenuBar::from_model"
        ),
        Declared::NoHolder => {
            #[cfg(debug_assertions)]
            eprintln!(
                "teksilo: set_application_menu has nowhere to put the menu: this tree belongs \
                 to no application (teksilo-app registers the application menu's holder)"
            );
        }
    }
}

impl ApplicationMenuExt for BuildContext<'_> {
    fn set_application_menu(&mut self, model: MenuModel) {
        declare(
            self.window().is_some(),
            self.app_state::<ApplicationMenu>(),
            model,
        );
    }
}

impl ApplicationMenuExt for EventContext<'_> {
    fn set_application_menu(&mut self, model: MenuModel) {
        declare(
            self.window().is_some(),
            self.app_state::<ApplicationMenu>(),
            model,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Declaring the same model again (a rebuild of the application tree's
    /// root, say) changes nothing: no bar rebuilds, nothing is installed
    /// again. Another model, even an equal one, is a new declaration.
    #[test]
    fn redeclaring_the_same_model_changes_nothing() {
        let holder = ApplicationMenu::new();
        let model = MenuModel::new();
        holder.set(model.clone());
        let generation = holder.generation().get();
        holder.set(model.clone());
        assert_eq!(holder.generation().get(), generation, "the same handle");
        holder.set(MenuModel::new());
        assert_ne!(holder.generation().get(), generation, "another model");
    }

    /// A model declared again and again under a new handle is said once,
    /// past a few within the span; a slow trickle, or the same handle, is
    /// not.
    #[test]
    fn a_menu_declared_again_and_again_is_said_once() {
        let start = std::time::Instant::now();
        let at = |secs: u64| start + std::time::Duration::from_secs(secs);
        let mut watch = RedeclareWatch::default();
        let said: Vec<bool> = (0..8).map(|i| watch.note(at(i))).collect();
        let limit = RedeclareWatch::LIMIT as usize;
        assert!(said[..limit].iter().all(|said| !said), "a few are normal");
        assert!(said[limit], "one more within the span is said");
        assert!(said[limit + 1..].iter().all(|said| !said), "once");

        let mut slow = RedeclareWatch::default();
        let span = RedeclareWatch::SPAN.as_secs();
        assert!(
            (0..20).all(|i| !slow.note(at(i * span))),
            "one a span is a menu that changes"
        );

        let holder = ApplicationMenu::new();
        let model = MenuModel::new();
        for _ in 0..10 {
            holder.set(model.clone());
        }
        assert!(
            !holder.inner.redeclared.borrow().warned,
            "the same handle is not a declaration"
        );
        for _ in 0..10 {
            holder.set(MenuModel::new());
        }
        assert!(holder.inner.redeclared.borrow().warned);
    }

    /// A window's tree cannot declare the application menu: the call
    /// changes nothing (a debug build also panics, in [`declare`], which
    /// `teksilo-app`'s `a_window_cannot_declare_the_application_menu`
    /// checks). The application tree's call declares it.
    #[test]
    fn a_windows_declaration_changes_nothing() {
        let holder = ApplicationMenu::new();
        let generation = holder.generation().get();
        assert_eq!(
            try_declare(true, Some(&holder), MenuModel::new()),
            Declared::FromWindow
        );
        assert!(holder.model().is_none(), "nothing was declared");
        assert_eq!(holder.generation().get(), generation, "no bar rebuilds");

        assert_eq!(
            try_declare(false, Some(&holder), MenuModel::new()),
            Declared::Set
        );
        assert!(holder.model().is_some());
        assert_eq!(
            try_declare(false, None, MenuModel::new()),
            Declared::NoHolder
        );
    }
}
