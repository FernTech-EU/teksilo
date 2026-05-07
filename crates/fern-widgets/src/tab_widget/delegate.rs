//! Per-tab metadata extraction for `TabBar<T>` and `TabWidget<T>`.
//!
//! `TabDelegate<T>` is a struct of closures the tab bar invokes against
//! each item to obtain its label, icon, slots, tooltip, and capability
//! flags (closable / pinned / enabled). Mirrors `ListView`'s
//! `Fn(usize, &T, bool) -> Box<dyn Widget>` delegate pattern, but split
//! into per-aspect callbacks so callers don't have to compose every
//! affordance into one giant builder.
//!
//! Closures are called at build time. Mutating an item via
//! `ListModel::set(i, …)` fires `DataChange::ItemUpdated` which
//! triggers a rebuild of the bar — closures re-run, labels and icons
//! re-resolve. Locale changes propagate through the same path because
//! `LocalizedString` already carries reactive resolution semantics.

use std::rc::Rc;

use fern_canvas::Point;
use fern_core::widget::{EventContext, Widget};
use fern_i18n::LocalizedString;

use crate::IconWidget;
use crate::tooltip::{RichTooltipSource, TooltipContent};

/// A reusable widget factory the framework calls every time a context
/// menu opens. Returns a fresh widget instance each call (the
/// framework can't reuse a single widget across multiple openings).
///
/// Same shape as the framework's
/// [`fern_core::widget_builder::ContextMenuFactory`] — receives the
/// click position (in tab-local coords) and a full
/// [`EventContext`], and returns `Some(menu)` to mount or `None` to
/// decline. The `Rc` wrapping is a tab-widget convenience: the
/// delegate clones the factory per-tab without reallocating.
pub type ContextMenuFactory = Rc<dyn Fn(Point, &mut EventContext) -> Option<Box<dyn Widget>>>;

/// Bar orientation. Selects between a horizontal row of tabs (default
/// for browser-style document tabs) and a vertical column of pills
/// (sidebar / IDE perspective convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabBarOrientation {
    /// Tabs flow left-to-right in a horizontal row. Scroll axis is
    /// horizontal; a vertical wheel maps to horizontal scroll
    /// (Firefox / Chrome convention) when
    /// `vertical_wheel_scrolls_horizontally` is on.
    #[default]
    Horizontal,
    /// Tabs flow top-to-bottom in a vertical column. Scroll axis is
    /// vertical; vertical wheel scrolls vertically. Pinned tabs
    /// render in a non-scrolling strip at the top of the column.
    Vertical,
}

/// Whether the layout-axis extent (width in horizontal bars, height in
/// vertical bars) is shared across all unpinned tabs or chosen
/// per-tab from content.
///
/// See the module docs of [`crate::tab_widget`] for how this is
/// applied per orientation. In wrap (multi-line horizontal) mode
/// `Independent` is forced regardless of this setting — equal-width
/// tabs in a wrapping row look like a tile grid and lose the
/// bookmark-bar / pill-strip aesthetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabSizing {
    /// All non-pinned tabs share the same extent on the layout axis.
    /// The available region is divided equally across the unpinned
    /// count, then clamped to `[min_tab_extent, max_tab_extent]`.
    /// Below the min, content overflows into scroll. Above the max,
    /// slack is left as empty space at the trailing edge.
    Shared,
    /// Each tab sizes to its content (icon + label + slots), clamped
    /// to `[min_tab_extent, max_tab_extent]`. Truncation via ellipsis
    /// when content hits `max`.
    Independent,
}

/// Type alias for label-resolving callbacks.
type LabelFn<T> = Box<dyn Fn(usize, &T) -> LocalizedString>;
/// Type alias for icon-resolving callbacks.
type IconFn<T> = Box<dyn Fn(usize, &T) -> Option<IconWidget>>;
/// Type alias for slot-widget-resolving callbacks.
type SlotFn<T> = Box<dyn Fn(usize, &T) -> Option<Box<dyn Widget>>>;
/// Type alias for context-menu-factory-resolving callbacks. The
/// returned factory is callable many times (once per right-click).
type ContextMenuFn<T> = Box<dyn Fn(usize, &T) -> Option<ContextMenuFactory>>;
/// Type alias for tooltip callbacks.
type TooltipFn<T> = Box<dyn Fn(usize, &T) -> Option<LocalizedString>>;
/// Type alias for rich-tooltip-key callbacks (returns a registry key per tab).
type RichTooltipKeyFn<T> = Box<dyn Fn(usize, &T) -> Option<String>>;
/// Type alias for inline rich-tooltip-content callbacks.
type RichTooltipContentFn<T> = Box<dyn Fn(usize, &T) -> Option<TooltipContent>>;
/// Type alias for composite-tooltip callbacks. Returns a boxed widget
/// because closure-returning-`impl Trait` isn't object-safe.
type CompositeTooltipFn<T> = Box<dyn Fn(usize, &T) -> Option<Box<dyn Widget>>>;
/// Type alias for boolean capability callbacks.
type FlagFn<T> = Box<dyn Fn(usize, &T) -> bool>;

/// Resolves per-tab UI from a model item.
///
/// Required: a `label` callback. Everything else is optional and
/// defaults to "no leading icon, no slots, no tooltip, not closable,
/// not pinned, enabled".
pub struct TabDelegate<T: 'static> {
    pub(crate) label: LabelFn<T>,
    pub(crate) icon: Option<IconFn<T>>,
    pub(crate) leading: Option<SlotFn<T>>,
    pub(crate) trailing: Option<SlotFn<T>>,
    pub(crate) context_menu: Option<ContextMenuFn<T>>,
    pub(crate) closable: Option<FlagFn<T>>,
    pub(crate) pinned: Option<FlagFn<T>>,
    pub(crate) enabled: Option<FlagFn<T>>,
    pub(crate) tooltip: Option<TooltipFn<T>>,
    pub(crate) rich_tooltip_key: Option<RichTooltipKeyFn<T>>,
    pub(crate) rich_tooltip_content: Option<RichTooltipContentFn<T>>,
    pub(crate) composite_tooltip: Option<CompositeTooltipFn<T>>,
}

impl<T: 'static> std::fmt::Debug for TabDelegate<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabDelegate")
            .field("has_icon", &self.icon.is_some())
            .field("has_leading", &self.leading.is_some())
            .field("has_trailing", &self.trailing.is_some())
            .field("has_context_menu", &self.context_menu.is_some())
            .field("has_closable", &self.closable.is_some())
            .field("has_pinned", &self.pinned.is_some())
            .field("has_enabled", &self.enabled.is_some())
            .field("has_tooltip", &self.tooltip.is_some())
            .field("has_rich_tooltip_key", &self.rich_tooltip_key.is_some())
            .field(
                "has_rich_tooltip_content",
                &self.rich_tooltip_content.is_some(),
            )
            .field(
                "has_composite_tooltip",
                &self.composite_tooltip.is_some(),
            )
            .finish()
    }
}

impl<T: 'static> TabDelegate<T> {
    /// Construct from the label callback. Every other field defaults
    /// to its identity behavior.
    pub fn new(label: impl Fn(usize, &T) -> LocalizedString + 'static) -> Self {
        Self {
            label: Box::new(label),
            icon: None,
            leading: None,
            trailing: None,
            context_menu: None,
            closable: None,
            pinned: None,
            enabled: None,
            tooltip: None,
            rich_tooltip_key: None,
            rich_tooltip_content: None,
            composite_tooltip: None,
        }
    }

    /// Per-tab leading icon (rendered before the label).
    pub fn icon(mut self, f: impl Fn(usize, &T) -> Option<IconWidget> + 'static) -> Self {
        self.icon = Some(Box::new(f));
        self
    }

    /// Per-tab leading slot (between the icon and label, or before
    /// the label when no icon is present).
    pub fn leading(mut self, f: impl Fn(usize, &T) -> Option<Box<dyn Widget>> + 'static) -> Self {
        self.leading = Some(Box::new(f));
        self
    }

    /// Per-tab trailing slot (between the label and the close button,
    /// or at the trailing edge when no close button is present).
    pub fn trailing(mut self, f: impl Fn(usize, &T) -> Option<Box<dyn Widget>> + 'static) -> Self {
        self.trailing = Some(Box::new(f));
        self
    }

    /// Per-tab context menu factory. Activated by right-click /
    /// long-press / `accesskit::Action::ShowContextMenu`.
    ///
    /// The closure runs once per build and returns an optional
    /// [`ContextMenuFactory`]. The factory itself is called every
    /// time the menu opens, returning a fresh menu widget each call —
    /// the framework cannot reuse a single widget instance across
    /// multiple openings.
    pub fn context_menu(
        mut self,
        f: impl Fn(usize, &T) -> Option<ContextMenuFactory> + 'static,
    ) -> Self {
        self.context_menu = Some(Box::new(f));
        self
    }

    /// Per-tab closable flag. When `true`, the tab gets a trailing
    /// close button and middle-click / `Ctrl+W` close affordances.
    /// Pinned tabs suppress the close button regardless of this flag
    /// (pinned tabs only close via the context menu — Firefox
    /// convention).
    pub fn closable(mut self, f: impl Fn(usize, &T) -> bool + 'static) -> Self {
        self.closable = Some(Box::new(f));
        self
    }

    /// Per-tab pinned flag. Pinned tabs render in a leading
    /// non-scrolling region with a fixed icon-only width.
    pub fn pinned(mut self, f: impl Fn(usize, &T) -> bool + 'static) -> Self {
        self.pinned = Some(Box::new(f));
        self
    }

    /// Per-tab enabled flag. Disabled tabs are visible but not
    /// activatable, skipped by keyboard navigation, and excluded from
    /// the close / pin / context-menu affordances.
    pub fn enabled(mut self, f: impl Fn(usize, &T) -> bool + 'static) -> Self {
        self.enabled = Some(Box::new(f));
        self
    }

    /// Per-tab tooltip text. Shown on hover via the existing
    /// `WidgetBuilder::tooltip` mechanism.
    pub fn tooltip(mut self, f: impl Fn(usize, &T) -> Option<LocalizedString> + 'static) -> Self {
        self.tooltip = Some(Box::new(f));
        self.rich_tooltip_key = None;
        self.rich_tooltip_content = None;
        self.composite_tooltip = None;
        self
    }

    /// Per-tab rich-tooltip registry key. Returning `Some(key)` makes
    /// the tab show a rich tooltip resolved against
    /// [`TooltipRegistry`](crate::tooltip::TooltipRegistry).
    pub fn rich_tooltip_key(
        mut self,
        f: impl Fn(usize, &T) -> Option<String> + 'static,
    ) -> Self {
        self.rich_tooltip_key = Some(Box::new(f));
        self.tooltip = None;
        self.rich_tooltip_content = None;
        self.composite_tooltip = None;
        self
    }

    /// Per-tab inline rich-tooltip content. Skips the registry — useful
    /// for tooltips whose body depends on `T`'s state.
    pub fn rich_tooltip_content_with(
        mut self,
        f: impl Fn(usize, &T) -> Option<TooltipContent> + 'static,
    ) -> Self {
        self.rich_tooltip_content = Some(Box::new(f));
        self.tooltip = None;
        self.rich_tooltip_key = None;
        self.composite_tooltip = None;
        self
    }

    /// Per-tab composite-tooltip body factory. Returning
    /// `Some(boxed_widget)` makes the tab show a composite tooltip
    /// containing that subtree. The closure runs at tab-header build
    /// time (and on every rebuild after data changes), so the body
    /// can carry per-tab dynamic state.
    pub fn composite_tooltip_with(
        mut self,
        f: impl Fn(usize, &T) -> Option<Box<dyn Widget>> + 'static,
    ) -> Self {
        self.composite_tooltip = Some(Box::new(f));
        self.tooltip = None;
        self.rich_tooltip_key = None;
        self.rich_tooltip_content = None;
        self
    }

    pub(crate) fn resolve_label(&self, index: usize, item: &T) -> LocalizedString {
        (self.label)(index, item)
    }

    pub(crate) fn resolve_icon(&self, index: usize, item: &T) -> Option<IconWidget> {
        self.icon.as_ref().and_then(|f| f(index, item))
    }

    pub(crate) fn resolve_leading(&self, index: usize, item: &T) -> Option<Box<dyn Widget>> {
        self.leading.as_ref().and_then(|f| f(index, item))
    }

    pub(crate) fn resolve_trailing(&self, index: usize, item: &T) -> Option<Box<dyn Widget>> {
        self.trailing.as_ref().and_then(|f| f(index, item))
    }

    pub(crate) fn resolve_context_menu(
        &self,
        index: usize,
        item: &T,
    ) -> Option<ContextMenuFactory> {
        self.context_menu.as_ref().and_then(|f| f(index, item))
    }

    pub(crate) fn resolve_closable(&self, index: usize, item: &T) -> bool {
        self.closable
            .as_ref()
            .map(|f| f(index, item))
            .unwrap_or(false)
    }

    pub(crate) fn resolve_pinned(&self, index: usize, item: &T) -> bool {
        self.pinned
            .as_ref()
            .map(|f| f(index, item))
            .unwrap_or(false)
    }

    pub(crate) fn resolve_enabled(&self, index: usize, item: &T) -> bool {
        self.enabled
            .as_ref()
            .map(|f| f(index, item))
            .unwrap_or(true)
    }

    pub(crate) fn resolve_tooltip(&self, index: usize, item: &T) -> Option<LocalizedString> {
        self.tooltip.as_ref().and_then(|f| f(index, item))
    }

    pub(crate) fn resolve_rich_tooltip(
        &self,
        index: usize,
        item: &T,
    ) -> Option<RichTooltipSource> {
        if let Some(ref f) = self.rich_tooltip_key
            && let Some(k) = f(index, item)
        {
            return Some(RichTooltipSource::Key(k));
        }
        if let Some(ref f) = self.rich_tooltip_content
            && let Some(c) = f(index, item)
        {
            return Some(RichTooltipSource::Content(c));
        }
        None
    }

    pub(crate) fn resolve_composite_tooltip(
        &self,
        index: usize,
        item: &T,
    ) -> Option<Box<dyn Widget>> {
        self.composite_tooltip.as_ref().and_then(|f| f(index, item))
    }
}
