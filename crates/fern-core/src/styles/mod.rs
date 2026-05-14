//! FernUI's theming system: [`Theme`] aggregator, [`ThemeAppearance`]
//! flag, typed [`ThemeExtensions`] registry, Tier 2 paint-recipe
//! primitives, and Tier 3 per-widget style trait protocols.
//!
//! See `docs/styling-system.md` for the full four-tier ladder.

pub mod banner_style;
pub mod button_style;
pub mod card_style;
pub mod checkbox_style;
pub mod combo_box_style;
pub mod component_style_slots;
pub mod dialog_style;
pub mod icon_button_style;
pub mod menu_item_style;
pub mod panel_style;
pub mod popover_style;
pub mod radio_style;
pub mod recipe;
pub mod scroll_bar_style;
pub mod snackbar_style;
pub mod slider_style;
pub mod standard_item_style;
pub mod tab_style;
pub mod text_input_style;
pub mod theme;
pub mod theme_appearance;
pub mod theme_extension;
pub mod toggle_style;
pub mod tooltip_style;

pub use banner_style::{
    BannerSeverity, BannerStyle, BannerStyleConfig, SharedBannerStyle,
};
pub use button_style::{
    ButtonRecipe, ButtonStyle, ButtonStyleConfig, ButtonVariant, SharedButtonStyle,
};
pub use card_style::{CardStyle, CardStyleConfig, CardVariant, SharedCardStyle};
pub use checkbox_style::{
    CheckboxState, CheckboxStyle, CheckboxStyleConfig, CheckboxVariant, SharedCheckboxStyle,
};
pub use combo_box_style::{
    ComboBoxStyle, ComboBoxStyleConfig, ComboBoxVariant, SharedComboBoxStyle,
};
pub use component_style_slots::ComponentStyleSlots;
pub use dialog_style::{DialogStyle, DialogStyleConfig, SharedDialogStyle};
pub use icon_button_style::{
    IconButtonSize, IconButtonStyle, IconButtonStyleConfig, SharedIconButtonStyle,
};
pub use menu_item_style::{MenuItemStyle, MenuItemStyleConfig, SharedMenuItemStyle};
pub use panel_style::{PanelStyle, PanelStyleConfig, PanelVariant, SharedPanelStyle};
pub use popover_style::{PopoverStyle, PopoverStyleConfig, PopoverVariant, SharedPopoverStyle};
pub use radio_style::{RadioStyle, RadioStyleConfig, RadioVariant, SharedRadioStyle};
pub use recipe::{
    BorderPosition, BorderRecipe, BorderStyle, FillRecipe, GradientStop, PerStateRecipe,
    RecipeColor, ShadowRecipe, ShapeRecipe, WidgetState,
};
pub use scroll_bar_style::{
    ScrollBarOrientation, ScrollBarStyle, ScrollBarStyleConfig, ScrollBarVariant,
    SharedScrollBarStyle,
};
pub use slider_style::{
    SharedSliderStyle, SliderOrientation, SliderStyle, SliderStyleConfig, SliderVariant,
};
pub use snackbar_style::{SharedSnackbarStyle, SnackbarStyle, SnackbarStyleConfig};
pub use standard_item_style::{
    SharedStandardItemStyle, StandardItemStyle, StandardItemStyleConfig,
};
pub use tab_style::{
    SharedTabStyle, TabBarChromeConfig, TabBarOrientation, TabStyle, TabStyleConfig,
};
pub use text_input_style::{
    SharedTextInputStyle, TextInputStyle, TextInputStyleConfig, TextInputValidationLevel,
    TextInputVariant,
};
pub use theme::Theme;
pub use theme_appearance::ThemeAppearance;
pub use theme_extension::ThemeExtensions;
pub use toggle_style::{SharedToggleStyle, ToggleStyle, ToggleStyleConfig, ToggleVariant};
pub use tooltip_style::{SharedTooltipStyle, TooltipStyle, TooltipStyleConfig};
