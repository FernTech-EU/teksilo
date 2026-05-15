//! Typed `Rc<dyn FooStyle>` slot bag — the theme-wide override channel
//! for the four-tier styling system.
//!
//! Each themable widget reads its slot like:
//!
//! ```ignore
//! let style = self.style_override
//!     .clone()
//!     .or_else(|| ctx.theme().style_slots.button.clone())
//!     .unwrap_or_else(|| Rc::new(RecipeButtonStyle::default()));
//! ```
//!
//! Per-call `.style(...)` overrides win first; theme-wide
//! `style_slots.button = Some(...)` wins second; the widget's local
//! `Recipe*Style` default is the fallback.
//!
//! `Option` per slot rather than a populated default because the
//! `Recipe*Style` types live in `fern-widgets` and can't be imported by
//! `fern-core` (cycle). Apps that want theme-wide custom styles
//! install them explicitly:
//!
//! ```ignore
//! let mut theme = intui::light();
//! theme.style_slots.button = Some(Rc::new(MyGlassButton));
//! ```

use crate::styles::{
    SharedAvatarStyle, SharedBadgeStyle, SharedBannerStyle, SharedButtonStyle, SharedCardStyle,
    SharedCheckboxStyle, SharedComboBoxStyle, SharedDialogStyle, SharedIconButtonStyle,
    SharedLinkStyle, SharedMenuItemStyle, SharedPanelStyle, SharedPopoverStyle,
    SharedProgressBarStyle, SharedRadioStyle, SharedScrollBarStyle,
    SharedSegmentedControlStyle, SharedSliderStyle, SharedSnackbarStyle,
    SharedStandardItemStyle, SharedTabStyle, SharedTextInputStyle, SharedToggleStyle,
    SharedTooltipStyle,
};

/// Typed slot bag living on [`crate::styles::Theme`]. One slot per
/// themable widget. `None` means "use the widget's local default
/// `Recipe*Style`"; `Some(rc)` installs the override theme-wide.
#[derive(Default, Clone)]
pub struct ComponentStyleSlots {
    pub button: Option<SharedButtonStyle>,
    pub icon_button: Option<SharedIconButtonStyle>,
    pub toggle: Option<SharedToggleStyle>,
    pub checkbox: Option<SharedCheckboxStyle>,
    pub radio: Option<SharedRadioStyle>,
    pub slider: Option<SharedSliderStyle>,
    pub text_input: Option<SharedTextInputStyle>,
    pub combo_box: Option<SharedComboBoxStyle>,
    pub menu_item: Option<SharedMenuItemStyle>,
    pub panel: Option<SharedPanelStyle>,
    pub card: Option<SharedCardStyle>,
    pub popover: Option<SharedPopoverStyle>,
    pub tooltip: Option<SharedTooltipStyle>,
    pub scroll_bar: Option<SharedScrollBarStyle>,
    pub standard_item: Option<SharedStandardItemStyle>,
    pub tab: Option<SharedTabStyle>,
    pub dialog: Option<SharedDialogStyle>,
    pub snackbar: Option<SharedSnackbarStyle>,
    pub banner: Option<SharedBannerStyle>,
    pub badge: Option<SharedBadgeStyle>,
    pub progress_bar: Option<SharedProgressBarStyle>,
    pub link: Option<SharedLinkStyle>,
    pub segmented_control: Option<SharedSegmentedControlStyle>,
    pub avatar: Option<SharedAvatarStyle>,
}

impl std::fmt::Debug for ComponentStyleSlots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-rolled because `Rc<dyn FooStyle>` doesn't impl Debug.
        // Show which slots are populated (Some/None) — the actual
        // chrome behaviour isn't introspectable.
        f.debug_struct("ComponentStyleSlots")
            .field("button", &self.button.is_some())
            .field("icon_button", &self.icon_button.is_some())
            .field("toggle", &self.toggle.is_some())
            .field("checkbox", &self.checkbox.is_some())
            .field("radio", &self.radio.is_some())
            .field("slider", &self.slider.is_some())
            .field("text_input", &self.text_input.is_some())
            .field("combo_box", &self.combo_box.is_some())
            .field("menu_item", &self.menu_item.is_some())
            .field("panel", &self.panel.is_some())
            .field("card", &self.card.is_some())
            .field("popover", &self.popover.is_some())
            .field("tooltip", &self.tooltip.is_some())
            .field("scroll_bar", &self.scroll_bar.is_some())
            .field("standard_item", &self.standard_item.is_some())
            .field("tab", &self.tab.is_some())
            .field("dialog", &self.dialog.is_some())
            .field("snackbar", &self.snackbar.is_some())
            .field("banner", &self.banner.is_some())
            .field("badge", &self.badge.is_some())
            .field("progress_bar", &self.progress_bar.is_some())
            .field("link", &self.link.is_some())
            .field("segmented_control", &self.segmented_control.is_some())
            .field("avatar", &self.avatar.is_some())
            .finish()
    }
}

impl PartialEq for ComponentStyleSlots {
    fn eq(&self, other: &Self) -> bool {
        // Rc trait-object pointer-equality is the only meaningful "are
        // these the same style" check. Used for theme-equality (mostly
        // tests + cache keys).
        fn rc_eq<T: ?Sized>(a: &Option<std::rc::Rc<T>>, b: &Option<std::rc::Rc<T>>) -> bool {
            match (a, b) {
                (None, None) => true,
                (Some(x), Some(y)) => std::rc::Rc::ptr_eq(x, y),
                _ => false,
            }
        }
        rc_eq(&self.button, &other.button)
            && rc_eq(&self.icon_button, &other.icon_button)
            && rc_eq(&self.toggle, &other.toggle)
            && rc_eq(&self.checkbox, &other.checkbox)
            && rc_eq(&self.radio, &other.radio)
            && rc_eq(&self.slider, &other.slider)
            && rc_eq(&self.text_input, &other.text_input)
            && rc_eq(&self.combo_box, &other.combo_box)
            && rc_eq(&self.menu_item, &other.menu_item)
            && rc_eq(&self.panel, &other.panel)
            && rc_eq(&self.card, &other.card)
            && rc_eq(&self.popover, &other.popover)
            && rc_eq(&self.tooltip, &other.tooltip)
            && rc_eq(&self.scroll_bar, &other.scroll_bar)
            && rc_eq(&self.standard_item, &other.standard_item)
            && rc_eq(&self.tab, &other.tab)
            && rc_eq(&self.dialog, &other.dialog)
            && rc_eq(&self.snackbar, &other.snackbar)
            && rc_eq(&self.banner, &other.banner)
            && rc_eq(&self.badge, &other.badge)
            && rc_eq(&self.progress_bar, &other.progress_bar)
            && rc_eq(&self.link, &other.link)
            && rc_eq(&self.segmented_control, &other.segmented_control)
            && rc_eq(&self.avatar, &other.avatar)
    }
}
