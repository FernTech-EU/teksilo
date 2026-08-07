// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-tab presentation metadata.
//!
//! [`TabInfo`] is the bundle of "what does this tab look like?"
//! values: title, icon, tooltip, capability flags. Decoupled from
//! [`crate::tab_widget::TabHandle`] so the same struct is reusable
//! by both static and dynamic tab construction paths.
//!
//! Title and tooltip are [`LocalizedString`] — they
//! accept `tr!(...)` (locale-reactive) and convert from raw
//! strings via `LocalizedString::literal`. Icon is a factory
//! closure (no `IconWidget: Clone` requirement) so the same
//! `TabInfo` can be cloned cheaply and the icon is rebuilt each
//! TabHeader build, picking up theme / state changes naturally.

use std::rc::Rc;

use teksilo_canvas::Point;
use teksilo_core::widget::{EventContext, Widget};
use teksilo_i18n::LocalizedString;

use super::delegate::ContextMenuFactory;
use crate::IconWidget;
use crate::tooltip::RichTooltipSource;

/// Reusable factory for an [`IconWidget`]. Boxed in `Rc` so
/// [`TabInfo`] is `Clone` without forcing `IconWidget: Clone`.
pub type IconFactory = Rc<dyn Fn() -> IconWidget>;

/// Reusable factory for a composite-tooltip body widget. Boxed in
/// `Rc` so [`TabInfo`] is `Clone` without forcing the body's type to
/// be `Clone`. The factory is called each time the tab's header
/// builds — typically once per tab lifetime, plus rebuilds triggered
/// by data-source mutations.
pub type CompositeTooltipFactory = Rc<dyn Fn() -> Box<dyn Widget>>;

/// Per-tab presentation metadata. Build with [`TabInfo::new`] and
/// fluent setters.
///
/// ```rust
/// # use teksilo_widgets::tab_widget::TabInfo;
/// # use teksilo_widgets::primitives::IconWidget;
/// # use teksilo_i18n::lit;
/// let _info = TabInfo::new()
///     .title(lit!("Welcome"))
///     .icon(|| IconWidget::checkmark(16.0))
///     .closable(true);
/// ```
#[derive(Clone)]
pub struct TabInfo {
    pub(crate) title: Option<LocalizedString>,
    pub(crate) icon: Option<IconFactory>,
    pub(crate) tooltip: Option<LocalizedString>,
    /// Optional rich tooltip — registry key or inline content.
    /// Mutually exclusive with `tooltip` and `composite_tooltip`.
    pub(crate) rich_tooltip: Option<RichTooltipSource>,
    /// Optional composite tooltip body factory. Mutually exclusive
    /// with the other two tooltip slots.
    pub(crate) composite_tooltip: Option<CompositeTooltipFactory>,
    pub(crate) closable: bool,
    pub(crate) pinned: bool,
    /// Initial-enabled hint. Forwarded into the arena at build time
    /// via `ctx.enabled_when(header_id, false)` when `false`; the
    /// arena is then the single source of truth and ANDs with
    /// ancestors. A disabled `TabBar` ancestor disables every tab
    /// regardless of this flag.
    pub(crate) initial_enabled: teksilo_core::signal::Prop<bool>,
    /// Mark the tab's content pane as focusable so keyboard users can
    /// reach it. ARIA: a `tabpanel` with no focusable content must
    /// itself be focusable (`tabindex="0"`). Opt-in because the
    /// framework can't reliably auto-detect at build time (children
    /// are built lazily). Default: `false`.
    pub(crate) focusable_panel: bool,
    /// Optional per-tab context menu (right-click the tab header). Same
    /// shape as the delegate's `context_menu`; for dynamic tabs this is
    /// the per-handle way to attach one.
    pub(crate) context_menu: Option<ContextMenuFactory>,
}

impl Default for TabInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TabInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabInfo")
            .field("title", &self.title)
            .field("has_icon", &self.icon.is_some())
            .field("tooltip", &self.tooltip)
            .field("has_rich_tooltip", &self.rich_tooltip.is_some())
            .field("has_composite_tooltip", &self.composite_tooltip.is_some())
            .field("closable", &self.closable)
            .field("pinned", &self.pinned)
            .field("initial_enabled", &self.initial_enabled.get())
            .field("focusable_panel", &self.focusable_panel)
            .field("has_context_menu", &self.context_menu.is_some())
            .finish()
    }
}

impl TabInfo {
    /// Empty defaults: no title, no icon, no tooltip, not closable,
    /// not pinned, enabled.
    pub fn new() -> Self {
        Self {
            title: None,
            icon: None,
            tooltip: None,
            rich_tooltip: None,
            composite_tooltip: None,
            closable: false,
            pinned: false,
            initial_enabled: teksilo_core::signal::Prop::Static(true),
            focusable_panel: false,
            context_menu: None,
        }
    }

    /// Attach a per-tab context menu (right-click the tab header). The
    /// factory receives the click position (tab-local) and a full
    /// [`EventContext`], and returns `Some(menu)` to mount or `None` to
    /// decline (falling through to an ancestor). Cloned per header build.
    pub fn context_menu(
        mut self,
        f: impl Fn(Point, &mut EventContext) -> Option<Box<dyn Widget>> + 'static,
    ) -> Self {
        self.context_menu = Some(Rc::new(f));
        self
    }

    /// Set the tab's title. Accepts `tr!(...)`, a literal string,
    /// or any value implementing `Into<LocalizedString>`.
    /// `None` means icon-only (the pinned-tab presentation).
    pub fn title(mut self, t: impl Into<LocalizedString>) -> Self {
        self.title = Some(t.into());
        self
    }

    /// Untitled — useful for icon-only tabs even when not pinned.
    pub fn no_title(mut self) -> Self {
        self.title = None;
        self
    }

    /// Set the leading icon via a factory closure. The closure is
    /// called each time the `TabHeader`
    /// is built — typically once per tab lifetime, plus any rebuild
    /// triggered by data-source mutations.
    pub fn icon(mut self, factory: impl Fn() -> IconWidget + 'static) -> Self {
        self.icon = Some(Rc::new(factory));
        self
    }

    /// Tooltip text shown on hover. If unset and the tab is
    /// [pinned](Self::pinned), the framework promotes [title](Self::title)
    /// to the tooltip — pinned tabs render icon-only and otherwise
    /// have no way for the user to identify them.
    pub fn tooltip(mut self, t: impl Into<LocalizedString>) -> Self {
        self.tooltip = Some(t.into());
        self.rich_tooltip = None;
        self.composite_tooltip = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip
    /// registry. See [`Button::rich_tooltip`](crate::button::Button::rich_tooltip).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip = Some(RichTooltipSource::Key(key.into()));
        self.tooltip = None;
        self.composite_tooltip = None;
        self
    }

    /// Attach a rich tooltip driven by inline `TooltipContent`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip = Some(RichTooltipSource::Content(content));
        self.tooltip = None;
        self.composite_tooltip = None;
        self
    }

    /// Attach a composite tooltip — third tier, hosting an arbitrary
    /// widget tree. The `factory` closure is called each time the
    /// tab's header rebuilds, so the body picks up theme / locale
    /// changes naturally without retaining state across rebuilds.
    pub fn composite_tooltip<W>(mut self, factory: impl Fn() -> W + 'static) -> Self
    where
        W: Widget + 'static,
    {
        self.composite_tooltip = Some(Rc::new(move || -> Box<dyn Widget> { Box::new(factory()) }));
        self.tooltip = None;
        self.rich_tooltip = None;
        self
    }

    /// Whether the tab shows a close button + responds to
    /// middle-click. Default: `false`.
    pub fn closable(mut self, b: bool) -> Self {
        self.closable = b;
        self
    }

    /// Whether the tab renders in the leading pinned strip
    /// (icon-only, fixed-width, no close button — Firefox / Chrome
    /// convention). Default: `false`.
    pub fn pinned(mut self, b: bool) -> Self {
        self.pinned = b;
        self
    }

    /// Whether the tab can be activated. Disabled tabs render but
    /// are skipped by keyboard navigation, can't be clicked, and
    /// don't get the close button. Default: `true`.
    ///
    /// Forwarded to the arena via `ctx.enabled_when(header_id, false)`
    /// at build time when `false`. Ancestor-driven disable (e.g. a
    /// disabled `TabBar`) ANDs with this flag automatically.
    pub fn enabled(mut self, enabled: impl Into<teksilo_core::signal::Prop<bool>>) -> Self {
        self.initial_enabled = enabled.into();
        self
    }

    /// Make the tab's content pane itself focusable, so keyboard users
    /// can press `Tab` from the selected tab header and land inside
    /// the panel.
    ///
    /// Opt in for panels you know contain no focusable descendants —
    /// a static text-only "About" tab, a chart-only metrics tab.
    /// Panels that already host a `Button`, `TextInput`, `ListView`,
    /// or any other interactive widget don't need this: focus will
    /// flow naturally into the descendant.
    ///
    /// ARIA: this implements the `tabindex="0"` requirement that an
    /// empty `tabpanel` must be focusable so its content can be read
    /// by screen readers in browse mode. AccessKit has no `tabindex`
    /// field; the framework advertises `Action::Focus` on the panel
    /// node to signal focusability to AT. Default: `false`.
    pub fn focusable_panel(mut self, b: bool) -> Self {
        self.focusable_panel = b;
        self
    }
}
