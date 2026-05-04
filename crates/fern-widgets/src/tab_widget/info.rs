//! Per-tab presentation metadata.
//!
//! [`TabInfo`] is the bundle of "what does this tab look like?"
//! values: title, icon, tooltip, capability flags. Decoupled from
//! [`crate::tab_widget::TabHandle`] so the same struct is reusable
//! by both static and dynamic tab construction paths.
//!
//! Title and tooltip are [`fern_i18n::LocalizedString`] — they
//! accept `tr!(...)` (locale-reactive) and convert from raw
//! strings via `LocalizedString::literal`. Icon is a factory
//! closure (no `IconWidget: Clone` requirement) so the same
//! `TabInfo` can be cloned cheaply and the icon is rebuilt each
//! TabHeader build, picking up theme / state changes naturally.

use std::rc::Rc;

use fern_i18n::LocalizedString;

use crate::IconWidget;

/// Reusable factory for an [`IconWidget`]. Boxed in `Rc` so
/// [`TabInfo`] is `Clone` without forcing `IconWidget: Clone`.
pub type IconFactory = Rc<dyn Fn() -> IconWidget>;

/// Per-tab presentation metadata. Build with [`TabInfo::new`] and
/// fluent setters.
///
/// ```ignore
/// TabInfo::new()
///     .title(tr!("welcome"))
///     .icon(|| IconWidget::checkmark(16.0))
///     .closable(true);
/// ```
#[derive(Default, Clone)]
pub struct TabInfo {
    pub(crate) title: Option<LocalizedString>,
    pub(crate) icon: Option<IconFactory>,
    pub(crate) tooltip: Option<LocalizedString>,
    pub(crate) closable: bool,
    pub(crate) pinned: bool,
    pub(crate) enabled: bool,
}

impl std::fmt::Debug for TabInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabInfo")
            .field("title", &self.title)
            .field("has_icon", &self.icon.is_some())
            .field("tooltip", &self.tooltip)
            .field("closable", &self.closable)
            .field("pinned", &self.pinned)
            .field("enabled", &self.enabled)
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
            closable: false,
            pinned: false,
            enabled: true,
        }
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
    /// called each time the [`TabHeader`](crate::tab_widget::TabHeader)
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
    pub fn enabled(mut self, b: bool) -> Self {
        self.enabled = b;
        self
    }
}
