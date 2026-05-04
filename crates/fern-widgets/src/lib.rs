pub mod accordion;
pub mod avatar;
pub mod calendar;
pub mod common;
#[cfg(feature = "rich-text")]
pub mod date_edit;
#[cfg(feature = "rich-text")]
pub mod date_range_edit;
#[cfg(feature = "rich-text")]
pub mod date_time_edit;
#[cfg(feature = "rich-text")]
pub mod time_edit;
pub mod badge;
pub mod breadcrumb;
pub mod built_in_button;
pub mod button;
pub mod card;
pub mod animations;
pub mod checkbox;
pub mod combo_box;
pub mod dialog;
pub mod group_box;
pub mod group_header;
pub mod link;
pub(crate) mod drag_preview;
pub(crate) mod list_item_a11y;
pub(crate) mod list_source;
pub mod list_view;
pub mod menu_bar;
pub(crate) mod menu_context;
pub mod menu_item;
pub mod menu_list;
pub mod message_box;
pub(crate) mod overlay_trigger;
pub mod panel;
pub mod popover;
pub mod popover_button;
pub mod primitives;
pub mod privacy_settings;
pub mod progress_bar;
pub mod radio_button;
pub mod radio_group;
pub mod repeater;
#[cfg(feature = "rich-text")]
pub mod rich_text;
pub mod scroll_area;
#[cfg(feature = "rich-text")]
pub mod text_input;
#[cfg(feature = "rich-text")]
pub mod hex_color_input;
#[cfg(feature = "rich-text")]
pub mod color_picker;
#[cfg(feature = "rich-text")]
pub mod color_edit;
pub mod scroll_bar;
pub mod keystroke_format;
pub mod segmented_control;
pub mod shortcut_settings;
pub mod slider;
pub mod snackbar;
pub mod spinner;
#[cfg(feature = "rich-text")]
pub mod spin_box;
pub mod split_button;
pub mod split_view;
pub mod status_bar;
pub mod tab_widget;
pub mod table_view;
pub mod tool_box;
pub mod tree_table;
pub mod title_bar;
pub mod toggle;
pub mod toolbar;
pub mod tooltip;
pub mod tree_view;
pub mod wizard;

#[cfg(feature = "preview")]
mod preview_catalog;

pub use tooltip::TooltipWidget;

pub use accordion::Accordion;
pub use animations::{
    Blur, Collapse, Crossfade, Cycle, Fade, Pulse, Rotate, Scale, ScaleOrigin, Shake, Slide,
    SlideEdge, SmoothSize, SmoothSizeAxes,
};
pub use avatar::{Avatar, AvatarCorner, AvatarPresence, AvatarShape, AvatarSize};
pub use badge::Badge;
pub use breadcrumb::{Breadcrumb, BreadcrumbItem};
pub use built_in_button::{BuiltInButton, BuiltInButtonSize, BuiltInIcons};
pub use button::{Button, ButtonVariant, IconLocation};
pub use calendar::{Calendar, DateRange, WeekNumberDisplay};
pub use card::Card;
#[cfg(feature = "rich-text")]
pub use date_edit::DateEdit;
#[cfg(feature = "rich-text")]
pub use date_range_edit::DateRangeEdit;
#[cfg(feature = "rich-text")]
pub use date_time_edit::DateTimeEdit;
#[cfg(feature = "rich-text")]
pub use time_edit::{SecondsMode, TimeEdit, TimeFormat};
pub use checkbox::{CheckState, Checkbox};
pub use combo_box::ComboBox;
pub use dialog::{Dialog, DialogContent, ModalContainer};
pub use group_box::GroupBox;
pub use group_header::GroupHeader;
pub use link::Link;
pub use list_view::ListView;
pub use menu_bar::MenuBar;
pub use menu_item::MenuItem;
pub use menu_list::{MenuList, MenuSeparator};
pub use message_box::{
    ButtonRole, EventContextMessageBoxExt, MessageBox, MessageBoxButton, MessageBoxButtons,
    MessageBoxResult, MessageBoxSeverity, StandardButton,
};
pub use panel::Panel;
pub use popover::Popover;
pub use popover_button::PopoverButton;
pub use primitives::{
    AspectRatio, Center, Divider, Expand, FixedSize, FormLayout, Grid, HStack, IconWidget, ImageFit,
    ImageWidget, MasonryLayout, MaxSize, MinSize,
    Padding, RectWidget, Spacer, Switcher, TextWidget, TrackSize, VStack, Wrap, ZStack,
};
#[cfg(feature = "rich-text")]
pub use primitives::TextInputField;
pub use progress_bar::ProgressBar;
pub use radio_button::RadioButton;
pub use radio_group::RadioGroup;
pub use repeater::Repeater;
pub use scroll_area::{ScrollArea, ScrollBarPolicy, ScrollBarMode};
pub use scroll_bar::{ScrollBar, ScrollBarOrientation};
pub use table_view::{
    Alignment as TableAlignment, CellContext, CellSelectionModel, Column, ColumnContext,
    ColumnResizePolicy, ColumnWidth, EditTrigger, GridLines, PinnedSide, SortDirection,
    TabTraversal, TableSelectionMode, TableView, TruncationPolicy,
};
pub use segmented_control::SegmentedControl;
pub use shortcut_settings::ShortcutSettings;
pub use privacy_settings::PrivacySettings;
pub use slider::Slider;
pub use snackbar::Snackbar;
pub use spinner::Spinner;
#[cfg(feature = "rich-text")]
pub use spin_box::{ButtonLayout as SpinButtonLayout, SpinBox, SpinValue, StepType, WheelMode, WrapMode};
pub use split_button::SplitButton;
pub use split_view::SplitView;
pub use status_bar::StatusBar;
pub use tab_widget::{TabItem, TabWidget};
pub use tool_box::{ToolBox, ToolBoxItem};
#[cfg(feature = "rich-text")]
pub use text_input::{TextInput, ValidationState};
#[cfg(feature = "rich-text")]
pub use hex_color_input::HexColorInput;
#[cfg(feature = "rich-text")]
pub use color_picker::{
    ColorPicker, ColorPickerLayout, ColorSwatch, DEFAULT_SWATCHES,
};
#[cfg(feature = "rich-text")]
pub use color_edit::ColorEdit;
pub use title_bar::{DragRegion, ResizeStrip, TitleBar, WindowControls, WindowFrame};
pub use toggle::Toggle;
pub use toolbar::Toolbar;
pub use tree_table::TreeTable;
pub use tree_view::TreeView;
pub use wizard::{Wizard, WizardStep};

/// The framework bundle: fern-widgets' own translatable strings, grouped
/// by locale. Registered by applications via
/// `I18nConfig::framework_locales(fern_widgets::framework_locales())`
/// at startup (architecture §12.13).
///
/// fern-widgets currently ships `en-US` (source) and `fr-FR`. Keys
/// missing from a locale's bundle fall back to the en-US source via
/// fluent-bundle's per-key fallback. Applications that need a locale
/// fern-widgets doesn't ship can fill the gap with
/// `I18nConfig::override_widget_strings(...)` — see §12.13.4.
pub fn framework_locales() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("en-US", &[include_str!("../locales/en-US.ftl")]),
        ("fr-FR", &[include_str!("../locales/fr-FR.ftl")]),
    ]
}

#[cfg(test)]
mod layout_integration_tests;
