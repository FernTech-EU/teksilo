//! Tabbed-container widgets.
//!
//! Two public entry points:
//!
//! - [`TabBar<T>`] — a header strip driven by a `ListModel<T>` /
//!   [`ListDataSource`](bastyde_data::ListDataSource) and a
//!   [`TabDelegate<T>`]. Use it stand-alone when you want only the
//!   tab strip (e.g., a document tab strip whose content lives in a
//!   different panel or window).
//!
//! - [`TabWidget`] — the all-in-one composition: bar above, content
//!   `Switcher` below, sharing one selection signal. Two
//!   construction flavors:
//!     - [`static_tab(info, content)`](TabWidget::static_tab) —
//!       fixed tabs accumulated at construction.
//!     - [`dynamic_tab::<S>(kind, factory)`](TabWidget::dynamic_tab) +
//!       [`dynamic_model(model)`](TabWidget::dynamic_model) — apps
//!       register a content factory per tab `kind` (`"plain-text-doc"`,
//!       `"image"`, …); the live tab list is a mutable
//!       `ListModel<TabHandle>` mutated at runtime (open / close /
//!       reorder).
//!
//! Static tabs always render first, in declaration order; dynamic
//! tabs follow. Selection is by stable [`TabId`] — drag-reorder and
//! model mutations never silently send the active selection to a
//! different tab.
//!
//! ## Accessibility
//!
//! Both [`TabWidget`] and [`TabBar`] emit `Role::TabList` on the bar
//! and `Role::Tab` on each header. ARIA APG ([tabs
//! pattern](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/))
//! recommends providing an accessible name for the tab list
//! whenever a page hosts more than one — call
//! [`.access_label(tr!(editor_tabs()))`](bastyde_core::widget_builder::WidgetBuilder::access_label)
//! on the widget so screen readers can distinguish "editor tabs"
//! from "tool tabs":
//!
//! ```ignore
//! TabWidget::new(selected)
//!     .static_tab(TabInfo::new().title(tr!(welcome())), welcome_panel)
//!     // ...
//!     .access_label(tr!(editor_tabs()))
//! ```
//!
//! Panels with no focusable descendants (a static text-only "About"
//! tab, a chart-only metrics tab) are unreachable by Tab key unless
//! opted in via [`TabInfo::focusable_panel(true)`](TabInfo::focusable_panel).

use bastyde_i18n::lit;
use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::drag_payload::DragPayload;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_data::ListModel;

use crate::primitives::{Expand, Switcher, VStack};

mod bar;
mod delegate;
mod handle;
mod header;
mod id;
mod info;

#[cfg(test)]
mod a11y_tests;
#[cfg(test)]
mod tests;

pub use bar::{
    DEFAULT_BAR_SLOT_SPACING, DEFAULT_MAX_TAB_WIDTH, DEFAULT_MIN_TAB_WIDTH,
    DEFAULT_PINNED_TAB_WIDTH, DEFAULT_TAB_SPACING, TabBar,
};
pub use delegate::{ContextMenuFactory, TabBarOrientation, TabDelegate, TabSizing};
pub use handle::{STATIC_KIND, TabHandle};
pub use id::TabId;
pub use info::{IconFactory, TabInfo};

// ─── Static + dynamic content factory types ─────────────────────────

/// Closure that builds a static tab's content widget. Called once
/// per static tab — on the [`TabWidget`]'s first build that includes
/// it. The resulting pane is then memoized: rebuilds caused by
/// adjacent dynamic-model mutations reuse the same pane WidgetId, so
/// internal state (focus, scroll, animation progress, …) survives.
pub type StaticContentFactory = Rc<dyn Fn(&TabHandle) -> Box<dyn Widget>>;

/// Closure that builds a dynamic tab's content widget from its
/// handle and downcast typed payload. Internal — apps register via
/// [`TabWidget::dynamic_tab::<S>`](TabWidget::dynamic_tab) which
/// hides the `Any` downcast behind the type parameter.
pub(crate) type DynamicContentFactory = Rc<dyn Fn(&TabHandle, &dyn Any) -> Box<dyn Widget>>;

// ─── Static-tab content shapes ──────────────────────────────────────

/// One static tab's content + presentation. Three shapes:
///
/// - `Owned`: a one-shot `Box<dyn Widget>` from `static_tab(impl Widget)`.
///   Consumed on the slot's first registration.
/// - `Factory`: a `Fn(&TabHandle) -> Box<dyn Widget>` from
///   `static_tab_factory`. Called once on the slot's first
///   registration.
/// - `PreId`: a pre-registered `WidgetId` from `static_tab_id`,
///   wrapped in an alias on first registration. Stable for the
///   widget's lifetime.
enum StaticContentSource {
    Owned(Option<Box<dyn Widget>>),
    Factory(StaticContentFactory),
    PreId(Option<WidgetId>),
}

impl StaticContentSource {
    #[allow(clippy::wrong_self_convention)]
    fn into_widget(&mut self, handle: &TabHandle) -> Box<dyn Widget> {
        match self {
            StaticContentSource::Owned(opt) => opt
                .take()
                .expect("static tab content has already been consumed"),
            StaticContentSource::Factory(f) => f(handle),
            StaticContentSource::PreId(opt) => {
                let id = opt
                    .take()
                    .expect("static tab pre-registered id has already been consumed");
                Box::new(AliasWidget {
                    target: Some(id),
                    child_id: None,
                })
            }
        }
    }
}

/// One static tab slot. The `pane_id` is `None` until the slot's
/// first build and stable thereafter — that's what makes static
/// content survive sibling rebuilds.
struct StaticTabSlot {
    handle: TabHandle,
    source: StaticContentSource,
    pane_id: Option<WidgetId>,
}

/// One bar slot (leading or trailing). Memoized: registered on
/// first build via [`Self::resolve`], reused on subsequent builds.
struct BarSlot {
    pending: Option<PendingChild>,
    resolved: Option<WidgetId>,
}

impl BarSlot {
    fn new(child: PendingChild) -> Self {
        Self {
            pending: Some(child),
            resolved: None,
        }
    }

    /// Resolve the slot to a stable WidgetId, registering the pending
    /// widget on first call. Subsequent calls return the same id.
    fn resolve(&mut self, ctx: &mut BuildContext) -> WidgetId {
        if let Some(id) = self.resolved {
            return id;
        }
        let id = match self
            .pending
            .take()
            .expect("bar slot already resolved without id")
        {
            PendingChild::Id(id) => id,
            PendingChild::Deferred(w) => ctx.add_boxed(w),
        };
        self.resolved = Some(id);
        id
    }
}

// ─── TabWidget — the public composition ─────────────────────────────

/// All-in-one tabbed container. Builds a [`TabBar`] above a
/// `Switcher` of content panes, sharing one selection signal.
pub struct TabWidget {
    selected_id: Signal<Option<TabId>>,
    /// Internal index signal driving the inner `Switcher`'s
    /// visibility. Self-owned (persists across rebuilds) and kept in
    /// sync with `selected_id` via a single one-way effect installed
    /// in [`build`](Widget::build) — the bar manages its own id↔index
    /// bridge for keyboard / click / scroll, so this is just the
    /// content-pane mirror.
    switcher_index: Signal<usize>,

    /// Bar orientation — **reactive**. `Horizontal` (default) places
    /// the bar above the content; `Vertical` places it on the leading
    /// edge with content on the trailing side. Bound at
    /// [`BindingLevel::Rebuild`](bastyde_core::binding::BindingLevel::Rebuild)
    /// in [`build`](Widget::build), so flipping it from outside the
    /// widget re-runs the build with the new layout (the inner
    /// content panes are memoized across this rebuild — their
    /// internal state is preserved).
    orientation: Signal<TabBarOrientation>,

    static_tabs: Vec<StaticTabSlot>,
    dynamic_registry: HashMap<&'static str, DynamicContentFactory>,
    dynamic_model: Option<ListModel<TabHandle>>,

    /// Lazily-populated map from a dynamic tab's stable [`TabId`] to
    /// its content-pane WidgetId. Lets pane widgets (with their
    /// internal mutable state — focus, scroll, animation, …) survive
    /// across rebuilds caused by reorder, pin/unpin toggles, or
    /// adjacent insertions / removals. Pruned every build to drop
    /// entries whose tab is no longer in the model.
    dyn_pane_ids: HashMap<TabId, WidgetId>,

    // Bar configuration — forwarded to the inner TabBar.
    /// Reactive sizing strategy. `None` until `.tab_sizing(...)`
    /// or `.sizing_signal(...)` is called; defaulted by the bar
    /// (`TabSizing::Shared`) otherwise. When a signal is bound,
    /// the [`TabWidget`] also binds it at
    /// [`BindingLevel::Rebuild`](bastyde_core::binding::BindingLevel::Rebuild)
    /// so toggling the signal swaps Shared ↔ Independent live.
    sizing: Option<Signal<TabSizing>>,
    /// Surface color/role applied to **every** tab — selected, idle,
    /// and hovered all use this one value, so the strip reads as
    /// visually uniform. Set via [`Self::tab_surface_role`].
    /// `None` (default) means transparent.
    tab_surface_role: Option<bastyde_core::color_prop::ColorProp>,
    /// Text role used for the label (and matching icon tint) on the
    /// selected tab. Set via [`Self::selected_text_role`]. `None`
    /// defaults to [`bastyde_tokens::TextRole::Primary`] (Int UI
    /// editor-strip convention).
    selected_text_role: Option<bastyde_tokens::TextRole>,
    /// Text role used for the label (and matching icon tint) on idle
    /// tabs. Set via [`Self::idle_text_role`]. `None` defaults to
    /// [`bastyde_tokens::TextRole::Secondary`].
    idle_text_role: Option<bastyde_tokens::TextRole>,
    min_tab_width: Option<f32>,
    max_tab_width: Option<f32>,
    pinned_tab_width: Option<f32>,
    show_scroll_arrows: Option<bool>,
    show_overflow_dropdown: Option<bool>,
    reorderable: bool,
    on_close: Option<Rc<dyn Fn(TabId, &mut EventContext)>>,
    on_reorder: Option<Rc<dyn Fn(TabId, usize, &mut EventContext)>>,
    on_pin_toggle: Option<Rc<dyn Fn(TabId, bool, &mut EventContext)>>,
    /// Cross-bar transfer opt-in. Enables this `TabWidget` to both
    /// hand its (dynamic) tabs to other accepting `TabWidget`s and
    /// receive tabs from them.
    accept_external_tabs: bool,
    /// Target-side override: insert a received tab. Receives the moved
    /// [`TabHandle`] and the insertion index *within the dynamic
    /// region*. Defaults to inserting into [`dynamic_model`](Self::dynamic_model).
    on_tab_received: Option<Rc<dyn Fn(TabHandle, usize, &mut EventContext)>>,
    /// Source-side override: one of this widget's tabs was accepted by
    /// another `TabWidget`. Receives the transferred [`TabId`].
    /// Defaults to removing it from [`dynamic_model`](Self::dynamic_model).
    on_transfer_out: Option<Rc<dyn Fn(TabId, &mut EventContext)>>,
    /// Handler for **non-tab** drops (an in-app foreign drag carrying
    /// app data, or an OS file/text/URL drop). Receives the raw
    /// payload and the insertion index *within the dynamic region*.
    on_external_drop: Option<Rc<dyn Fn(&DragPayload, usize, &mut EventContext) -> bool>>,
    bar_leading_slot: Option<BarSlot>,
    bar_trailing_slot: Option<BarSlot>,

    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for TabWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabWidget")
            .field("selected", &self.selected_id.get())
            .field("static_tabs", &self.static_tabs.len())
            .field(
                "dynamic_registry",
                &self.dynamic_registry.keys().collect::<Vec<_>>(),
            )
            .field("has_dynamic_model", &self.dynamic_model.is_some())
            .finish()
    }
}

impl TabWidget {
    /// Construct an empty `TabWidget`. Selection is `None` until
    /// the first `static_tab(...)` / `dynamic_model(...)` adds a
    /// tab and the framework activates it.
    pub fn new(selected: Signal<Option<TabId>>) -> Self {
        Self {
            selected_id: selected,
            switcher_index: Signal::new(0_usize),
            orientation: Signal::new(TabBarOrientation::Horizontal),
            static_tabs: Vec::new(),
            dynamic_registry: HashMap::new(),
            dynamic_model: None,
            dyn_pane_ids: HashMap::new(),
            sizing: None,
            tab_surface_role: None,
            selected_text_role: None,
            idle_text_role: None,
            min_tab_width: None,
            max_tab_width: None,
            pinned_tab_width: None,
            show_scroll_arrows: None,
            show_overflow_dropdown: None,
            reorderable: false,
            on_close: None,
            on_reorder: None,
            on_pin_toggle: None,
            accept_external_tabs: false,
            on_tab_received: None,
            on_transfer_out: None,
            on_external_drop: None,
            bar_leading_slot: None,
            bar_trailing_slot: None,
            root_child_id: None,
        }
    }

    /// Configure the bar to render vertically — pills stacked
    /// top-to-bottom on the leading edge, content fills the trailing
    /// area (sidebar / IDE-perspective convention). Equivalent to
    /// `self.orientation_signal().set(TabBarOrientation::Vertical)`.
    pub fn vertical(self) -> Self {
        self.orientation.set(TabBarOrientation::Vertical);
        self
    }

    /// Configure the bar to render horizontally — pills laid out
    /// left-to-right above the content (browser tab convention).
    /// This is the default.
    pub fn horizontal(self) -> Self {
        self.orientation.set(TabBarOrientation::Horizontal);
        self
    }

    /// Replace the internal orientation signal with an external one
    /// — lets a parent widget toggle orientation reactively (e.g. a
    /// "View → Vertical Tabs" toolbar button) without recreating the
    /// `TabWidget`.
    pub fn orientation_signal(mut self, signal: Signal<TabBarOrientation>) -> Self {
        self.orientation = signal;
        self
    }

    /// Add a static tab — fixed for the widget's lifetime, with a
    /// pre-built content widget. The content is registered in the
    /// arena on the [`TabWidget`]'s first build and **memoized** —
    /// subsequent rebuilds (caused by adjacent dynamic-model
    /// mutations) reuse the same pane WidgetId, preserving any
    /// internal state the content owns.
    pub fn static_tab(mut self, info: TabInfo, content: impl Widget + 'static) -> Self {
        let handle = TabHandle::static_handle(TabId::fresh(), info);
        self.static_tabs.push(StaticTabSlot {
            handle,
            source: StaticContentSource::Owned(Some(Box::new(content))),
            pane_id: None,
        });
        self
    }

    /// Ergonomic shorthand for a title-only static tab:
    /// `tab(label, content)` is `static_tab(TabInfo::new().title(label),
    /// content)`. `label` accepts `tr!(...)` (translated) or `lit!(...)`.
    /// This is the method the `bati!` `tab:` slot lowers to
    /// (`tab: lit!("Overview"), Card { … }`).
    pub fn tab(
        self,
        label: impl Into<bastyde_i18n::LocalizedString>,
        content: impl Widget + 'static,
    ) -> Self {
        self.static_tab(TabInfo::new().title(label), content)
    }

    /// `WidgetId` twin of [`tab`](Self::tab) — `tab_id(label, id)` is
    /// `static_tab_id(TabInfo::new().title(label), id)`. This is what the
    /// `bati!` `tab:` slot lowers to when its content is an id binding
    /// (`#{…}` / `name = Element`).
    pub fn tab_id(self, label: impl Into<bastyde_i18n::LocalizedString>, id: WidgetId) -> Self {
        self.static_tab_id(TabInfo::new().title(label), id)
    }

    /// Add a static tab whose content is constructed by a factory
    /// closure. The factory is called once — on the slot's first
    /// build — and the resulting pane is memoized just like
    /// [`static_tab`](Self::static_tab).
    pub fn static_tab_factory(
        mut self,
        info: TabInfo,
        factory: impl Fn(&TabHandle) -> Box<dyn Widget> + 'static,
    ) -> Self {
        let handle = TabHandle::static_handle(TabId::fresh(), info);
        self.static_tabs.push(StaticTabSlot {
            handle,
            source: StaticContentSource::Factory(Rc::new(factory)),
            pane_id: None,
        });
        self
    }

    /// Element-valued slot variant for the `bati!` DSL — accepts a
    /// pre-registered widget id rather than a `Box<dyn Widget>`.
    /// Equivalent to [`static_tab`](Self::static_tab) with an
    /// already-built child; the id is wrapped in a tab pane on
    /// first build and the pane id is memoized thereafter.
    pub fn static_tab_id(mut self, info: TabInfo, content_id: WidgetId) -> Self {
        let handle = TabHandle::static_handle(TabId::fresh(), info);
        self.static_tabs.push(StaticTabSlot {
            handle,
            source: StaticContentSource::PreId(Some(content_id)),
            pane_id: None,
        });
        self
    }

    /// Add a static tab with a caller-provided [`TabId`] — useful
    /// when external code (an app-event handler, a session-restore
    /// path, a deep link) needs to flip selection to this tab by id.
    /// The pane is memoized like [`static_tab`](Self::static_tab).
    pub fn static_tab_with_id(
        mut self,
        id: TabId,
        info: TabInfo,
        content: impl Widget + 'static,
    ) -> Self {
        let handle = TabHandle::static_handle(id, info);
        self.static_tabs.push(StaticTabSlot {
            handle,
            source: StaticContentSource::Owned(Some(Box::new(content))),
            pane_id: None,
        });
        self
    }

    /// Factory variant of [`static_tab_with_id`](Self::static_tab_with_id).
    pub fn static_tab_factory_with_id(
        mut self,
        id: TabId,
        info: TabInfo,
        factory: impl Fn(&TabHandle) -> Box<dyn Widget> + 'static,
    ) -> Self {
        let handle = TabHandle::static_handle(id, info);
        self.static_tabs.push(StaticTabSlot {
            handle,
            source: StaticContentSource::Factory(Rc::new(factory)),
            pane_id: None,
        });
        self
    }

    /// Register a dynamic-tab content factory keyed by `kind`. The
    /// `<S>` type parameter pins the payload type — the framework
    /// downcasts `handle.payload` to `S` before calling the
    /// factory and panics with a clear message on kind/payload
    /// mismatch, so `Any` never leaks into app code.
    pub fn dynamic_tab<S: Any + 'static>(
        mut self,
        kind: &'static str,
        factory: impl Fn(&TabHandle, &S) -> Box<dyn Widget> + 'static,
    ) -> Self {
        assert!(
            kind != STATIC_KIND,
            "tab kind '{}' is reserved by the framework for static tabs",
            STATIC_KIND
        );
        debug_assert!(
            !self.dynamic_registry.contains_key(kind),
            "dynamic_tab kind '{kind}' is already registered — duplicate registration"
        );
        let kind_for_panic = kind;
        let typed_factory: DynamicContentFactory = Rc::new(move |handle, payload| {
            let typed = payload.downcast_ref::<S>().unwrap_or_else(|| {
                panic!(
                    "tab kind '{}' was registered for {} but the handle's \
                     payload has a different type",
                    kind_for_panic,
                    std::any::type_name::<S>(),
                )
            });
            factory(handle, typed)
        });
        self.dynamic_registry.insert(kind, typed_factory);
        self
    }

    /// Connect the dynamic-tab data source. Mutations rebuild the
    /// dynamic-tab subtree; static tabs are unaffected.
    pub fn dynamic_model(mut self, model: ListModel<TabHandle>) -> Self {
        self.dynamic_model = Some(model);
        self
    }

    // ── Bar configuration (forwarded to inner TabBar) ──────────────

    /// Set the per-tab sizing strategy as a static value. Internally
    /// stores it as a `Signal<TabSizing>` so the widget can be
    /// retrofitted to reactive control via [`Self::sizing_signal`]
    /// without breaking existing call sites.
    pub fn tab_sizing(mut self, mode: TabSizing) -> Self {
        self.sizing = Some(Signal::new(mode));
        self
    }

    /// Bind the per-tab sizing strategy to an external signal —
    /// flipping the signal swaps Shared ↔ Independent live, with no
    /// rebuild on the parent's part. The signal is bound at
    /// `BindingLevel::Rebuild` inside [`build`](Widget::build);
    /// memoized panes survive the rebuild so per-tab state is
    /// preserved.
    pub fn sizing_signal(mut self, signal: Signal<TabSizing>) -> Self {
        self.sizing = Some(signal);
        self
    }

    /// Set the surface color/role applied to every tab in the strip.
    /// Accepts any `Color`, `SurfaceRole`, or `Signal<Color>` (via
    /// [`ColorProp`](bastyde_core::color_prop::ColorProp)). All tabs —
    /// selected, idle, and hovered — render the same surface, so
    /// selection is conveyed only by the accent indicator and the
    /// label-color shift (Int UI editor-strip convention). Default
    /// is transparent.
    pub fn tab_surface_role(
        mut self,
        color: impl Into<bastyde_core::color_prop::ColorProp>,
    ) -> Self {
        self.tab_surface_role = Some(color.into());
        self
    }

    /// Set the text role used for the label (and matching icon tint)
    /// on the **selected** tab. Default: [`bastyde_tokens::TextRole::Primary`]
    /// — the Int UI editor-strip convention. Override to e.g.
    /// [`bastyde_tokens::TextRole::Accent`] when the strip sits over a
    /// tinted surface.
    pub fn selected_text_role(mut self, role: bastyde_tokens::TextRole) -> Self {
        self.selected_text_role = Some(role);
        self
    }

    /// Set the text role used for the label (and matching icon tint)
    /// on **idle** tabs (not selected, not disabled). Default:
    /// [`bastyde_tokens::TextRole::Secondary`]. Disabled tabs always read
    /// as [`bastyde_tokens::TextRole::Disabled`] regardless of this
    /// setting.
    pub fn idle_text_role(mut self, role: bastyde_tokens::TextRole) -> Self {
        self.idle_text_role = Some(role);
        self
    }
    pub fn min_tab_width(mut self, dp: f32) -> Self {
        self.min_tab_width = Some(dp);
        self
    }
    pub fn max_tab_width(mut self, dp: f32) -> Self {
        self.max_tab_width = Some(dp);
        self
    }
    pub fn pinned_tab_width(mut self, dp: f32) -> Self {
        self.pinned_tab_width = Some(dp);
        self
    }
    pub fn show_scroll_arrows(mut self, on: bool) -> Self {
        self.show_scroll_arrows = Some(on);
        self
    }
    pub fn show_overflow_dropdown(mut self, on: bool) -> Self {
        self.show_overflow_dropdown = Some(on);
        self
    }
    pub fn reorderable(mut self, on: bool) -> Self {
        self.reorderable = on;
        self
    }

    /// Install a close-tab handler. Receives the [`TabId`] of the
    /// closed tab (not its index — indices are presentation-only)
    /// and the firing [`EventContext`]. The latter lets the handler
    /// open a confirmation dialog
    /// (`ctx.present_modal(MessageBox::confirm(...))`), dispatch an
    /// intent, or otherwise route the close request before mutating
    /// the underlying model. To veto, do nothing in the handler; to
    /// confirm-then-close, only call the model mutator on accept.
    ///
    /// If unset, the default behavior is to remove the tab from
    /// [`dynamic_model`](Self::dynamic_model) without a prompt
    /// (static tabs cannot be closed by default).
    pub fn on_close(mut self, f: impl Fn(TabId, &mut EventContext) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// Install a reorder handler. Receives `(moved_tab_id,
    /// destination_index, ctx)` in the unified static-then-dynamic
    /// ordering. The firing [`EventContext`] lets the handler
    /// confirm or dispatch the reorder via a dialog / intent
    /// before mutating the model. If unset, the default behavior
    /// is to reorder within the dynamic region of
    /// [`dynamic_model`](Self::dynamic_model). Implies
    /// [`reorderable(true)`](Self::reorderable).
    pub fn on_reorder(mut self, f: impl Fn(TabId, usize, &mut EventContext) + 'static) -> Self {
        self.on_reorder = Some(Rc::new(f));
        self.reorderable = true;
        self
    }

    /// Install a pin-toggle handler — receives `(tab_id,
    /// new_pinned_flag, ctx)` when the user drags a tab across the
    /// pinned ↔ unpinned boundary. The firing [`EventContext`]
    /// lets the handler confirm or dispatch the transition via a
    /// dialog / intent. Apps decide whether to actually mutate the
    /// tab's `info.pinned`.
    pub fn on_pin_toggle(mut self, f: impl Fn(TabId, bool, &mut EventContext) + 'static) -> Self {
        self.on_pin_toggle = Some(Rc::new(f));
        self
    }

    /// Opt into cross-`TabWidget` tab transfer (app-internal
    /// drag-and-drop between two tabbed containers). When enabled,
    /// this widget's **dynamic** tabs can be dragged out to any other
    /// accepting `TabWidget`, and it accepts tabs dragged in from one,
    /// painting an insertion-line indicator between its tabs.
    ///
    /// The dragged [`TabHandle`] moves intact — its `Rc<dyn Any>`
    /// payload (the heavy per-tab state) is preserved, not rebuilt —
    /// so the receiving widget must register a content factory for the
    /// tab's `kind` via [`dynamic_tab`](Self::dynamic_tab).
    ///
    /// **Static tabs are excluded**: they have no factory on a
    /// receiving widget, so they can never be transferred out (they
    /// still reorder in place if [`reorderable`](Self::reorderable)).
    ///
    /// By default, accepting a tab inserts it into this widget's
    /// [`dynamic_model`](Self::dynamic_model) and transferring one out
    /// removes it from this widget's model. Override either side with
    /// [`on_tab_received`](Self::on_tab_received) /
    /// [`on_transfer_out`](Self::on_transfer_out). Default: off.
    pub fn accept_external_tabs(mut self, on: bool) -> Self {
        self.accept_external_tabs = on;
        self
    }

    /// Override the target-side behaviour when a foreign tab is
    /// dropped onto this widget. Receives `(handle, insertion_index,
    /// ctx)` where `insertion_index` is within the **dynamic** tab
    /// region. The app inserts the handle into its own model. Implies
    /// [`accept_external_tabs(true)`](Self::accept_external_tabs).
    ///
    /// If unset, the default inserts the handle into
    /// [`dynamic_model`](Self::dynamic_model) at the drop position.
    pub fn on_tab_received(
        mut self,
        f: impl Fn(TabHandle, usize, &mut EventContext) + 'static,
    ) -> Self {
        self.on_tab_received = Some(Rc::new(f));
        self.accept_external_tabs = true;
        self
    }

    /// Override the source-side behaviour after one of this widget's
    /// tabs has been accepted by another `TabWidget`. Receives the
    /// transferred [`TabId`]; the app removes it from its own model.
    /// Implies [`accept_external_tabs(true)`](Self::accept_external_tabs).
    ///
    /// If unset, the default removes the tab from
    /// [`dynamic_model`](Self::dynamic_model).
    pub fn on_transfer_out(mut self, f: impl Fn(TabId, &mut EventContext) + 'static) -> Self {
        self.on_transfer_out = Some(Rc::new(f));
        self.accept_external_tabs = true;
        self
    }

    /// Accept **non-tab** drops onto the tab bar — an in-app foreign
    /// drag (e.g. a file dragged from a `TreeView`, carrying app data)
    /// or an OS file/text/URL drop. The bar shows an insertion-line
    /// indicator while such a payload hovers; on drop, `f` runs with
    /// the raw [`DragPayload`], the insertion index *within the dynamic
    /// region*, and the firing context. Inspect the payload
    /// (`get_typed::<T>()` / `files()` / `text()` / `uris()`) and, e.g.,
    /// push a new `TabHandle` into your [`dynamic_model`](Self::dynamic_model);
    /// return `true` if accepted.
    ///
    /// This is the "open a dropped file as a tab" hook (VS Code style).
    /// Independent of [`accept_external_tabs`](Self::accept_external_tabs).
    /// OS drops also require `BastydeAppBuilder::install_external_dnd()`.
    pub fn on_external_drop(
        mut self,
        f: impl Fn(&DragPayload, usize, &mut EventContext) -> bool + 'static,
    ) -> Self {
        self.on_external_drop = Some(Rc::new(f));
        self
    }

    pub fn bar_leading_slot(mut self, w: impl Widget + 'static) -> Self {
        self.bar_leading_slot = Some(BarSlot::new(PendingChild::Deferred(Box::new(w))));
        self
    }
    pub fn bar_trailing_slot(mut self, w: impl Widget + 'static) -> Self {
        self.bar_trailing_slot = Some(BarSlot::new(PendingChild::Deferred(Box::new(w))));
        self
    }

    /// Element-valued variant of
    /// [`bar_leading_slot`](Self::bar_leading_slot) accepting a
    /// pre-registered `WidgetId` (for the `bati!` DSL).
    pub fn bar_leading_slot_id(mut self, id: WidgetId) -> Self {
        self.bar_leading_slot = Some(BarSlot::new(PendingChild::Id(id)));
        self
    }
    /// Element-valued variant of
    /// [`bar_trailing_slot`](Self::bar_trailing_slot).
    pub fn bar_trailing_slot_id(mut self, id: WidgetId) -> Self {
        self.bar_trailing_slot = Some(BarSlot::new(PendingChild::Id(id)));
        self
    }
}

impl TabWidget {
    // ── build() helpers ────────────────────────────────────────────
    //
    // `build()` is decomposed into three self-contained steps so the
    // method body reads as orchestration rather than implementation.
    // Each helper captures only `&self` (plus the build-local lookup
    // tables it needs) and has no side effects beyond the arena
    // registrations it performs through `ctx`.

    /// Translate `TabInfo` fields into the [`TabDelegate`]'s
    /// closure-shaped accessors. Pure — captures nothing from the
    /// surrounding `build()`.
    fn build_delegate(&self) -> TabDelegate<TabHandle> {
        let mut delegate =
            TabDelegate::new(|_, h: &TabHandle| h.info.title.clone().unwrap_or_else(|| lit!("")))
                .icon(|_, h: &TabHandle| h.info.icon.as_ref().map(|f| f()))
                .closable(|_, h: &TabHandle| h.info.closable)
                .pinned(|_, h: &TabHandle| h.info.pinned)
                .enabled(|_, h: &TabHandle| h.info.initial_enabled)
                .tooltip(|_, h: &TabHandle| {
                    // Pinned tabs render icon-only; promote `title` to the
                    // tooltip if the caller didn't set one explicitly so
                    // the user can still identify the tab on hover.
                    if h.info.pinned
                        && h.info.tooltip.is_none()
                        && h.info.rich_tooltip.is_none()
                        && h.info.composite_tooltip.is_none()
                    {
                        h.info.title.clone()
                    } else {
                        h.info.tooltip.clone()
                    }
                });
        // Bypass the tooltip-clearing setters here: TabInfo already
        // enforces mutual exclusion across plain / rich / composite,
        // so each closure returns `Some` only for its flavor.
        delegate.rich_tooltip_key = Some(Box::new(|_, h: &TabHandle| match &h.info.rich_tooltip {
            Some(crate::tooltip::RichTooltipSource::Key(k)) => Some(k.clone()),
            _ => None,
        }));
        delegate.rich_tooltip_content =
            Some(Box::new(|_, h: &TabHandle| match &h.info.rich_tooltip {
                Some(crate::tooltip::RichTooltipSource::Content(c)) => Some(c.clone()),
                _ => None,
            }));
        delegate.composite_tooltip = Some(Box::new(|_, h: &TabHandle| {
            h.info.composite_tooltip.as_ref().map(|factory| factory())
        }));
        delegate
    }

    /// Wrap the bar's index-shaped callbacks (close / reorder / pin /
    /// cross-bar transfer / non-tab drop) into the app's id-shaped
    /// callbacks, translating at the boundary via `index_to_id` and
    /// `saturating_sub(static_count)` for the unified→dynamic index map.
    fn wire_bar_callbacks(
        &self,
        mut bar: TabBar<TabHandle>,
        index_to_id: &Rc<Vec<TabId>>,
        static_count: usize,
    ) -> TabBar<TabHandle> {
        // Wrap callbacks: bar speaks in indices, app speaks in
        // TabIds. We translate at the boundary using the
        // `index_to_id` lookup captured at build time.
        let close_cb = self.on_close.clone();
        let dyn_model_for_close = self.dynamic_model.clone();
        let idx_to_id_for_close = index_to_id.clone();
        bar = bar.on_close(move |i: usize, ctx: &mut EventContext| {
            if let Some(&id) = idx_to_id_for_close.get(i) {
                if let Some(ref f) = close_cb {
                    f(id, ctx);
                } else if i >= static_count {
                    // Default: remove from dynamic_model. Static
                    // tabs are not auto-closable.
                    if let Some(ref model) = dyn_model_for_close {
                        let dyn_idx = i - static_count;
                        if dyn_idx < model.len() {
                            let _ = model.remove(dyn_idx);
                        }
                    }
                }
            }
        });

        // `on_reorder(...)` setter sets `reorderable = true`, so the
        // single `self.reorderable` flag is the only gate we need.
        let reorder_cb = self.on_reorder.clone();
        let dyn_model_for_reorder = self.dynamic_model.clone();
        let idx_to_id_for_reorder = index_to_id.clone();
        if self.reorderable {
            bar = bar.on_reorder(move |from: usize, to: usize, ctx: &mut EventContext| {
                if let Some(&id) = idx_to_id_for_reorder.get(from) {
                    if let Some(ref f) = reorder_cb {
                        f(id, to, ctx);
                    } else if from >= static_count && to >= static_count {
                        // Default: reorder within the dynamic region
                        // only. Static tabs are pinned in place.
                        if let Some(ref model) = dyn_model_for_reorder {
                            let from_dyn = from - static_count;
                            let to_dyn = to - static_count;
                            if from_dyn < model.len() && to_dyn < model.len() {
                                model.move_item(from_dyn, to_dyn);
                            }
                        }
                    } else {
                        // Cross-boundary reorder: silently rejected
                        // by the default handler. Surface it once
                        // per process so developers don't chase a
                        // ghost — install an explicit `on_reorder`
                        // to interleave static and dynamic tabs.
                        warn_cross_boundary_reorder_once(from, to, static_count);
                    }
                }
            });
        }

        if let Some(f) = self.on_pin_toggle.clone() {
            let idx_to_id = index_to_id.clone();
            bar = bar.on_pin_toggle(move |i: usize, pinned: bool, ctx: &mut EventContext| {
                if let Some(&id) = idx_to_id.get(i) {
                    f(id, pinned, ctx);
                }
            });
        }

        // Cross-bar transfer wiring. The bar speaks in unified model
        // indices (static tabs first, then dynamic); the app speaks in
        // dynamic-region indices and TabIds. Static tabs are excluded
        // from transfer — they have no factory on a receiving widget.
        if self.accept_external_tabs {
            bar = bar
                .accept_external_tabs(true)
                .with_transferable_predicate(|_, h: &TabHandle| h.kind != STATIC_KIND);

            // Target side: insert the received handle. The bar's
            // insertion index is in unified model space; translate to
            // a dynamic-region index for the app / default model.
            let received_cb = self.on_tab_received.clone();
            let dyn_model_for_recv = self.dynamic_model.clone();
            bar = bar.on_tab_received_rc(Rc::new(
                move |handle: TabHandle, to_model: usize, ctx: &mut EventContext| {
                    let dyn_index = to_model.saturating_sub(static_count);
                    if let Some(ref f) = received_cb {
                        f(handle, dyn_index, ctx);
                    } else if let Some(ref model) = dyn_model_for_recv {
                        let idx = dyn_index.min(model.len());
                        model.insert(idx, handle);
                    }
                },
            ));

            // Source side: remove the transferred tab by id.
            let transfer_out_cb = self.on_transfer_out.clone();
            let dyn_model_for_out = self.dynamic_model.clone();
            bar = bar.on_transfer_out_rc(Rc::new(move |tab_id: TabId, ctx: &mut EventContext| {
                if let Some(ref f) = transfer_out_cb {
                    f(tab_id, ctx);
                } else if let Some(ref model) = dyn_model_for_out {
                    let pos =
                        (0..model.len()).find(|&i| model.with_item(i, |h| h.id) == Some(tab_id));
                    if let Some(pos) = pos {
                        let _ = model.remove(pos);
                    }
                }
            }));
        }

        // Non-tab drops (foreign in-app drag / OS file drop). Translate
        // the bar's unified model index to a dynamic-region index for
        // the app callback. Independent of `accept_external_tabs`.
        if let Some(external_cb) = self.on_external_drop.clone() {
            bar = bar.on_external_drop_rc(Rc::new(
                move |payload: &DragPayload, to_model: usize, ctx: &mut EventContext| {
                    let dyn_index = to_model.saturating_sub(static_count);
                    (external_cb)(payload, dyn_index, ctx)
                },
            ));
        }

        bar
    }

    /// Build (or reuse) the content panes. Static and dynamic panes
    /// both memoize their pane `WidgetId` — once registered, the pane
    /// outlives sibling rebuilds (caused by dynamic-model mutations) so
    /// internal state survives. Static panes cache in
    /// [`StaticTabSlot::pane_id`]; dynamic panes cache in
    /// [`Self::dyn_pane_ids`] keyed by [`TabId`], pruned at the end to
    /// drop tabs no longer in the model.
    fn build_panes(
        &mut self,
        ctx: &mut BuildContext,
        all_handles: &[TabHandle],
        static_count: usize,
        dyn_count: usize,
        panel_ids: &Rc<RefCell<Vec<WidgetId>>>,
        header_ids: &Rc<RefCell<Vec<WidgetId>>>,
    ) -> Vec<WidgetId> {
        let mut pane_ids: Vec<WidgetId> = Vec::with_capacity(static_count + dyn_count);

        for slot in self.static_tabs.iter_mut() {
            let pane_id = match slot.pane_id {
                Some(id) => id,
                None => {
                    let content = slot.source.into_widget(&slot.handle);
                    let id = ctx.add(TabPane::new(
                        slot.handle.clone(),
                        content,
                        panel_ids.clone(),
                        header_ids.clone(),
                    ));
                    slot.pane_id = Some(id);
                    id
                }
            };
            pane_ids.push(pane_id);
        }

        let mut alive_dyn: HashSet<TabId> = HashSet::with_capacity(dyn_count);
        for handle in all_handles.iter().skip(static_count) {
            alive_dyn.insert(handle.id);
            let pane_id = match self.dyn_pane_ids.get(&handle.id) {
                Some(&id) => id,
                None => {
                    let factory = self.dynamic_registry.get(handle.kind).unwrap_or_else(|| {
                        panic!(
                            "tab kind '{}' has no registered content factory — \
                             add a `dynamic_tab::<S>(\"{}\", |handle, state| ...)` \
                             registration before connecting the model",
                            handle.kind, handle.kind,
                        )
                    });
                    let content = factory(handle, handle.payload.as_ref());
                    let id = ctx.add(TabPane::new(
                        handle.clone(),
                        content,
                        panel_ids.clone(),
                        header_ids.clone(),
                    ));
                    self.dyn_pane_ids.insert(handle.id, id);
                    id
                }
            };
            pane_ids.push(pane_id);
        }
        // Prune dynamic-pane memo entries for tabs the model no
        // longer carries — their widgets become unreachable from any
        // root and the arena will reap them.
        self.dyn_pane_ids.retain(|id, _| alive_dyn.contains(id));

        pane_ids
    }
}

impl Widget for TabWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();

        // Bind orientation at Rebuild level — toggling the signal
        // (e.g. via a toolbar button) rebuilds TabWidget with the
        // new outer layout (HStack ↔ VStack) and a fresh TabBar in
        // the new orientation. Memoized panes survive the rebuild.
        self.orientation
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        let orientation = self.orientation.get();

        // Subscribe to dynamic-model mutations so add / remove /
        // reorder triggers a TabWidget rebuild that picks up the
        // new tab list.
        if let Some(model) = &self.dynamic_model {
            let version = ctx.signal(0_u64);
            version.bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
            let observer = model.observe_changes({
                let v = version.clone();
                move |_change| v.set(v.get().wrapping_add(1))
            });
            ctx.own_handle(observer);
        }

        // Snapshot static + dynamic into a single ordered handle
        // list. Static tabs come first, in declaration order.
        let static_count = self.static_tabs.len();
        let dyn_count = self.dynamic_model.as_ref().map(|m| m.len()).unwrap_or(0);
        let total = static_count + dyn_count;

        let mut all_handles: Vec<TabHandle> = Vec::with_capacity(total);
        for slot in &self.static_tabs {
            all_handles.push(slot.handle.clone());
        }
        if let Some(model) = &self.dynamic_model {
            for i in 0..dyn_count {
                if let Some(h) = model.with_item(i, |h| h.clone()) {
                    all_handles.push(h);
                }
            }
        }

        // Index → id lookup table. Used by the close / reorder /
        // pin callback wrappers below to translate the bar's
        // index-shaped events into id-shaped app callbacks. The
        // id ↔ selection bridge itself lives inside [`TabBar`] now;
        // TabWidget hands the bar `selected_id` and `id_of` directly.
        let index_to_id: Rc<Vec<TabId>> = Rc::new(all_handles.iter().map(|h| h.id).collect());
        let id_to_index: Rc<HashMap<TabId, usize>> = Rc::new(
            index_to_id
                .iter()
                .copied()
                .enumerate()
                .map(|(i, id)| (id, i))
                .collect(),
        );

        // Drive `switcher_index` from `selected_id`. One-way only:
        // the inner `Switcher` reads the index to pick which pane is
        // visible, but never writes back — selection mutations all
        // flow through `selected_id` (the bar updates it on click,
        // app code may set it externally). Pre-sync handles the
        // initial state and stale-id cases without needing a
        // bidirectional effect.
        if total > 0 {
            let target_idx = self
                .selected_id
                .get()
                .and_then(|id| id_to_index.get(&id).copied())
                .unwrap_or_else(|| self.switcher_index.get().min(total - 1));
            if self.switcher_index.get() != target_idx {
                self.switcher_index.set(target_idx);
            }
        }
        let id_to_idx = id_to_index.clone();
        let switcher_idx = self.switcher_index.clone();
        ctx.effect(&self.selected_id, move |maybe_id| {
            if let Some(id) = maybe_id
                && let Some(&i) = id_to_idx.get(id)
                && switcher_idx.get() != i
            {
                switcher_idx.set(i);
            }
        });

        // Internal model fed to the inner TabBar — a snapshot of
        // the unified handle list. Rebuilds when dynamic_model
        // mutates (via the version signal above).
        let internal_model = ListModel::from_vec(all_handles.clone());

        // Translate `TabInfo` fields into the TabDelegate's
        // closure-shaped accessors.
        let delegate = self.build_delegate();

        // Shared panel-id buffer: the Switcher writes panel widget
        // ids into it as panes are added; the bar's headers read
        // it to publish the Tab → TabPanel `controls()`
        // accessibility relation.
        let panel_ids = Rc::new(RefCell::new(Vec::with_capacity(total)));

        // Shared header-id buffer: the bar populates this with each
        // tab header's WidgetId in tab order; each TabPane reads it
        // to publish the TabPanel → Tab `aria-labelledby` relation.
        let header_ids: Rc<RefCell<Vec<WidgetId>>> =
            Rc::new(RefCell::new(Vec::with_capacity(total)));

        // Build + configure the inner TabBar. Selection is plumbed
        // through as id-based — the bar maintains its own private
        // index-side signal and bridges the two internally.
        let mut bar = match orientation {
            TabBarOrientation::Horizontal => TabBar::horizontal(
                internal_model,
                delegate,
                self.selected_id.clone(),
                |_, h: &TabHandle| h.id,
            ),
            TabBarOrientation::Vertical => TabBar::vertical(
                internal_model,
                delegate,
                self.selected_id.clone(),
                |_, h: &TabHandle| h.id,
            ),
        }
        .with_panel_ids(panel_ids.clone())
        .with_header_ids(header_ids.clone());

        if let Some(ref sizing) = self.sizing {
            // Bind at Rebuild level so flipping the signal triggers
            // a TabWidget rebuild that picks up the new sizing
            // mode. Memoized panes survive — only the bar is
            // rebuilt with the new layout.
            sizing.bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
            bar = bar.tab_sizing(sizing.get());
        }
        if let Some(ref bg) = self.tab_surface_role {
            bar = bar.tab_surface_role(bg.clone());
        }
        if let Some(role) = self.selected_text_role {
            bar = bar.selected_text_role(role);
        }
        if let Some(role) = self.idle_text_role {
            bar = bar.idle_text_role(role);
        }
        if let Some(w) = self.min_tab_width {
            bar = bar.min_tab_width(w);
        }
        if let Some(w) = self.max_tab_width {
            bar = bar.max_tab_width(w);
        }
        if let Some(w) = self.pinned_tab_width {
            bar = bar.pinned_tab_width(w);
        }
        if let Some(s) = self.show_scroll_arrows {
            bar = bar.show_scroll_arrows(s);
        }
        if let Some(s) = self.show_overflow_dropdown {
            bar = bar.show_overflow_dropdown(s);
        }
        if self.reorderable {
            bar = bar.reorderable(true);
        }

        // Wrap the bar's index-shaped callbacks into the app's
        // id-shaped callbacks (close / reorder / pin / transfer / drop).
        bar = self.wire_bar_callbacks(bar, &index_to_id, static_count);

        if let Some(ref mut slot) = self.bar_leading_slot {
            let id = slot.resolve(ctx);
            bar = bar.bar_leading_slot_id(id);
        }
        if let Some(ref mut slot) = self.bar_trailing_slot {
            let id = slot.resolve(ctx);
            bar = bar.bar_trailing_slot_id(id);
        }
        let bar_id = ctx.add(bar);

        // Build (or reuse) the content panes — static + dynamic, both
        // memoized so internal state survives sibling rebuilds.
        let pane_ids = self.build_panes(
            ctx,
            &all_handles,
            static_count,
            dyn_count,
            &panel_ids,
            &header_ids,
        );

        let mut switcher =
            Switcher::new(self.switcher_index.clone()).capture_child_ids_into(panel_ids);
        for &pane_id in &pane_ids {
            switcher = switcher.child_id(pane_id);
        }
        let switcher_id = ctx.add(switcher);
        // Tab content area must claim BOTH axes: full panel width
        // (so per-tab content fills the bounds, not just its natural
        // width) AND full panel height (slack below the tab bar).
        // `respect_intrinsic` makes the cross-axis fall back to the
        // switcher's intrinsic when a parent queries us with an
        // unspecified proposal, instead of reporting 0.
        let content_id = ctx.add(Expand::new().respect_intrinsic().child_id(switcher_id));

        let root_id = match orientation {
            TabBarOrientation::Horizontal => {
                ctx.add(VStack::new().add_child(bar_id).add_child(content_id))
            }
            TabBarOrientation::Vertical => ctx.add(
                crate::primitives::HStack::new()
                    .add_child(bar_id)
                    .add_child(content_id),
            ),
        };
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
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
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    /// TabWidget memoizes the WidgetIds of its static-tab panes,
    /// dynamic-tab panes (keyed by [`TabId`]), and bar slots across
    /// rebuilds — internal mutable state (focus, scroll, animation,
    /// rich-text editor history, …) survives sibling mutations
    /// (dynamic-model push / remove / reorder, locale or theme
    /// changes that retitle live tabs). Without this opt-in, the
    /// framework's default `destroy_subtree` step on rebuild would
    /// reap those memoized panes and the user would see static-tab
    /// content vanish the first time they opened or closed a
    /// dynamic tab.
    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }
}

// ─── Once-per-process developer warning ─────────────────────────────

/// Print a developer-aid warning the first time a cross-boundary
/// reorder is rejected by the default handler. Suppressed on
/// subsequent calls so high-frequency drag events don't spam stderr.
fn warn_cross_boundary_reorder_once(from: usize, to: usize, static_count: usize) {
    use std::sync::Once;
    static WARNED: Once = Once::new();
    WARNED.call_once(|| {
        eprintln!(
            "[bastyde-widgets::tab_widget] default on_reorder rejected a \
             cross-boundary move (from={from}, to={to}, \
             static_count={static_count}). Install an explicit \
             `on_reorder(...)` handler if you want to interleave \
             static and dynamic tabs."
        );
    });
}

// ─── TabPane (internal content-pane wrapper) ────────────────────────

/// Wraps each tab's content widget so the `Switcher` can attach a
/// stable accessibility name (the tab's title) and the framework's
/// dormancy bookkeeping (`controls` relation, `is_visible` flag)
/// has a consistent target.
#[derive(Debug)]
struct TabPane {
    handle: TabHandle,
    child_id: Option<WidgetId>,
    pending_child: Option<Box<dyn Widget>>,
    /// Captured during `build()` so `accessibility()` can find this
    /// pane's position in `panel_ids` (and thereby look up the
    /// corresponding tab header in `header_ids`) — surviving
    /// reorders without needing the parent to update memoized
    /// state.
    self_id: Option<WidgetId>,
    /// Shared buffer the parent `TabWidget` populates (via the
    /// inner `Switcher::capture_child_ids_into`) with each pane's
    /// `WidgetId` in tab order. The pane reads it to discover its
    /// own current index.
    panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    /// Shared buffer the bar populates with each header's
    /// `WidgetId` in tab order. Read at `accessibility()` time to
    /// resolve the labelling tab.
    header_ids: Rc<RefCell<Vec<WidgetId>>>,
    /// When true, the pane attaches a `focusable(true)` handler to
    /// itself at build time AND advertises `Action::Focus` from
    /// `accessibility()`. Apps opt in via
    /// [`TabInfo::focusable_panel`] for panels containing no
    /// focusable descendants (an empty "About" tab, a chart-only
    /// metrics tab) so keyboard users can reach them.
    self_focusable: bool,
}

impl TabPane {
    fn new(
        handle: TabHandle,
        content: Box<dyn Widget>,
        panel_ids: Rc<RefCell<Vec<WidgetId>>>,
        header_ids: Rc<RefCell<Vec<WidgetId>>>,
    ) -> Self {
        let self_focusable = handle.info.focusable_panel;
        Self {
            handle,
            child_id: None,
            pending_child: Some(content),
            self_id: None,
            panel_ids,
            header_ids,
            self_focusable,
        }
    }
}

impl Widget for TabPane {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.self_id = Some(ctx.self_id());
        if let Some(child) = self.pending_child.take() {
            self.child_id = Some(ctx.add_boxed(child));
        }
        if self.self_focusable {
            // Apply self-handlers so the framework treats this pane
            // as a Tab-key stop, allowing Tab from the selected tab
            // header to land inside an otherwise-empty panel.
            ctx.apply_self_handlers(
                bastyde_core::widget_builder::HandlerSet::new().focusable(true),
            );
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child_id
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
        builder.set_role(bastyde_core::accesskit::Role::TabPanel);
        if let Some(ref title) = self.handle.info.title {
            let resolved: String = title.clone().into();
            builder.set_name(&resolved);
        }
        // ARIA aria-labelledby — point to the tab header that
        // controls this panel. Look up *current* index by finding
        // self_id in panel_ids (which the Switcher repopulates each
        // build, so this auto-corrects on reorder), then map that
        // to the header at the same position. Skip the relation —
        // no dangling — when self_id or the header for that index
        // isn't yet available (e.g. mid-rebuild after a model
        // mutation).
        if let Some(self_id) = self.self_id {
            let panel_ids = self.panel_ids.borrow();
            if let Some(pos) = panel_ids.iter().position(|&id| id == self_id) {
                if let Some(&header_id) = self.header_ids.borrow().get(pos) {
                    builder.push_labelled_by(bastyde_core::accessibility::widget_id_to_node_id(
                        header_id,
                    ));
                }
            }
        }
        // Opt-in panel focusability (TabInfo::focusable_panel).
        // AccessKit has no `tabindex` field; `Action::Focus` is the
        // canonical way to signal focusability to AT, matching how
        // TabHeader::accessibility advertises focusability.
        if self.self_focusable {
            builder.add_action(bastyde_core::accesskit::Action::Focus);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

// ─── AliasWidget: thin wrapper exposing a pre-registered widget id ──

/// One-shot wrapper that "absorbs" a pre-registered `WidgetId` on
/// first build, returning it as the wrapper's only child. Used by
/// [`TabWidget::static_tab_id`] to bridge the
/// `bati!` DSL's element-valued-slot pattern (which pre-registers
/// the inner widget and hands the parent its id) into the factory
/// shape `static_tab_factory` expects.
#[derive(Debug)]
struct AliasWidget {
    target: Option<WidgetId>,
    child_id: Option<WidgetId>,
}

impl Widget for AliasWidget {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(id) = self.target.take() {
            self.child_id = Some(id);
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child_id
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

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
