//! Bastyde's theming system: [`Theme`] aggregator, [`ThemeAppearance`]
//! flag, typed [`ThemeExtensions`] registry, Tier 2 paint-recipe
//! primitives, and Tier 3 per-widget style trait protocols.
//!
//! See `docs/styling-system.md` for the full four-tier ladder.

#[path = "styles/avatar_style.rs"]
mod avatar_style;
#[path = "styles/badge_style.rs"]
mod badge_style;
#[path = "styles/banner_style.rs"]
mod banner_style;
#[path = "styles/button_style.rs"]
mod button_style;
#[path = "styles/calendar_style.rs"]
mod calendar_style;
#[path = "styles/card_style.rs"]
mod card_style;
#[path = "styles/checkbox_style.rs"]
mod checkbox_style;
#[path = "styles/color_picker_style.rs"]
mod color_picker_style;
#[path = "styles/combo_box_style.rs"]
mod combo_box_style;
#[path = "styles/component_style_slots.rs"]
mod component_style_slots;
#[path = "styles/date_edit_style.rs"]
mod date_edit_style;
#[path = "styles/dialog_style.rs"]
mod dialog_style;
#[path = "styles/drop_target_style.rs"]
mod drop_target_style;
#[path = "styles/drop_zone_style.rs"]
mod drop_zone_style;
#[path = "styles/grid_view_style.rs"]
mod grid_view_style;
#[path = "styles/icon_button_style.rs"]
mod icon_button_style;
#[path = "styles/link_style.rs"]
mod link_style;
#[path = "styles/list_container_style.rs"]
mod list_container_style;
#[path = "styles/menu_item_style.rs"]
mod menu_item_style;
#[path = "styles/panel_style.rs"]
mod panel_style;
#[path = "styles/popover_style.rs"]
mod popover_style;
#[path = "styles/progress_bar_style.rs"]
mod progress_bar_style;
#[path = "styles/radio_style.rs"]
mod radio_style;
#[path = "styles/recipe.rs"]
mod recipe;
#[path = "styles/rich_text_editor_style.rs"]
mod rich_text_editor_style;
#[path = "styles/scroll_bar_style.rs"]
mod scroll_bar_style;
#[path = "styles/search_field_style.rs"]
mod search_field_style;
#[path = "styles/segmented_control_style.rs"]
mod segmented_control_style;
#[path = "styles/slider_style.rs"]
mod slider_style;
#[path = "styles/snackbar_style.rs"]
mod snackbar_style;
#[path = "styles/spin_box_style.rs"]
mod spin_box_style;
#[path = "styles/split_button_style.rs"]
mod split_button_style;
#[path = "styles/standard_item_style.rs"]
mod standard_item_style;
#[path = "styles/tab_style.rs"]
mod tab_style;
#[path = "styles/table_style.rs"]
mod table_style;
#[path = "styles/text_input_style.rs"]
mod text_input_style;
#[path = "styles/theme.rs"]
mod theme;
#[path = "styles/theme_appearance.rs"]
mod theme_appearance;
#[path = "styles/theme_extension.rs"]
mod theme_extension;
#[path = "styles/toast_style.rs"]
mod toast_style;
#[path = "styles/toggle_style.rs"]
mod toggle_style;
#[path = "styles/tooltip_style.rs"]
mod tooltip_style;
#[path = "styles/web_view_style.rs"]
mod web_view_style;

pub use avatar_style::{
    AvatarCorner, AvatarPresence, AvatarShape, AvatarSize, AvatarStyle, AvatarStyleConfig,
    SharedAvatarStyle,
};
pub use badge_style::{BadgeStyle, BadgeStyleConfig, SharedBadgeStyle};
pub use banner_style::{BannerSeverity, BannerStyle, BannerStyleConfig, SharedBannerStyle};
pub use button_style::{
    ButtonRecipe, ButtonStyle, ButtonStyleConfig, ButtonVariant, SharedButtonStyle,
};
pub use calendar_style::{
    CalendarDayConfig, CalendarDayFill, CalendarHeaderConfig, CalendarStyle,
    CalendarZoomCellConfig, SharedCalendarStyle,
};
pub use card_style::{CardStyle, CardStyleConfig, CardVariant, SharedCardStyle};
pub use checkbox_style::{
    CheckboxState, CheckboxStyle, CheckboxStyleConfig, CheckboxVariant, SharedCheckboxStyle,
};
pub use color_picker_style::{
    ColorPickerLayout, ColorPickerStyle, ColorPickerStyleConfig, SharedColorPickerStyle,
};
pub use combo_box_style::{
    ComboBoxStyle, ComboBoxStyleConfig, ComboBoxVariant, SharedComboBoxStyle,
};
pub use component_style_slots::ComponentStyleSlots;
pub use date_edit_style::{DateEditStyle, DateEditStyleConfig, SharedDateEditStyle};
pub use dialog_style::{DialogStyle, DialogStyleConfig, SharedDialogStyle};
pub use drop_target_style::{
    DropTargetDragState, DropTargetStyle, DropTargetStyleConfig, DropTargetVariant,
    SharedDropTargetStyle,
};
pub use drop_zone_style::{
    DropZoneStyle, DropZoneStyleConfig, DropZoneVisualState, SharedDropZoneStyle,
};
pub use grid_view_style::{
    GridFocusRingRecipe, GridInsertionRecipe, GridMarqueeRecipe, GridViewStyle, SharedGridViewStyle,
};
pub use icon_button_style::{
    IconButtonSize, IconButtonStyle, IconButtonStyleConfig, SharedIconButtonStyle,
};
pub use link_style::{LinkStyle, LinkStyleConfig, SharedLinkStyle};
pub use list_container_style::{
    ListContainerStyle, ListInsertionConfig, ListInsertionRecipe, SharedListContainerStyle,
};
pub use menu_item_style::{MenuItemStyle, MenuItemStyleConfig, SharedMenuItemStyle};
pub use panel_style::{PanelStyle, PanelStyleConfig, PanelVariant, SharedPanelStyle};
pub use popover_style::{PopoverStyle, PopoverStyleConfig, PopoverVariant, SharedPopoverStyle};
pub use progress_bar_style::{
    ProgressBarStyle, ProgressBarStyleConfig, ProgressKind, SharedProgressBarStyle,
};
pub use radio_style::{RadioStyle, RadioStyleConfig, RadioVariant, SharedRadioStyle};
pub use recipe::{
    BorderPosition, BorderRecipe, BorderStyle, FillRecipe, GradientStop, PerStateRecipe,
    RecipeColor, ShadowRecipe, ShapeRecipe, WidgetState,
};
pub use rich_text_editor_style::{
    RichTextEditorStyle, RichTextEditorStyleConfig, SharedRichTextEditorStyle,
};
pub use scroll_bar_style::{
    ScrollBarOrientation, ScrollBarStyle, ScrollBarStyleConfig, ScrollBarVariant,
    SharedScrollBarStyle,
};
pub use search_field_style::{SearchFieldStyle, SearchFieldStyleConfig, SharedSearchFieldStyle};
pub use segmented_control_style::{
    SegmentedControlStyle, SegmentedControlStyleConfig, SharedSegmentedControlStyle,
};
pub use slider_style::{
    SharedSliderStyle, SliderOrientation, SliderStyle, SliderStyleConfig, SliderVariant,
};
pub use snackbar_style::{SharedSnackbarStyle, SnackbarStyle, SnackbarStyleConfig};
pub use spin_box_style::{ButtonLayout, SharedSpinBoxStyle, SpinBoxStyle, SpinBoxStyleConfig};
pub use split_button_style::{SharedSplitButtonStyle, SplitButtonStyle, SplitButtonStyleConfig};
pub use standard_item_style::{
    SharedStandardItemStyle, StandardItemStyle, StandardItemStyleConfig,
};
pub use tab_style::{
    SharedTabStyle, TabBarChromeConfig, TabBarOrientation, TabStyle, TabStyleConfig,
};
pub use table_style::{
    SharedTableStyle, SortDirection, TableGridRecipe, TableHeaderCellConfig, TableRowConfig,
    TableStyle,
};
pub use text_input_style::{
    SharedTextInputStyle, TextInputStyle, TextInputStyleConfig, TextInputValidationLevel,
    TextInputVariant,
};
pub use theme::Theme;
pub use theme_appearance::ThemeAppearance;
pub use theme_extension::ThemeExtensions;
pub use toast_style::{SharedToastStyle, ToastPriority, ToastStyle, ToastStyleConfig};
pub use toggle_style::{SharedToggleStyle, ToggleStyle, ToggleStyleConfig, ToggleVariant};
pub use tooltip_style::{SharedTooltipStyle, TooltipStyle, TooltipStyleConfig};
pub use web_view_style::{
    SharedWebViewStyle, WebViewStyle, WebViewStyleConfig, WebViewVisualState,
};
