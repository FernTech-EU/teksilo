#![allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::collapsible_if,
    clippy::manual_find,
    clippy::redundant_guards,
    clippy::needless_range_loop,
    clippy::explicit_counter_loop,
    clippy::if_same_then_else,
    dead_code
)]

pub mod accordion;
pub mod animations;
pub mod avatar;
pub mod badge;
pub mod banner;
pub mod breadcrumb;
pub mod button;
pub mod calendar;
pub mod card;
pub mod checkbox;
#[cfg(feature = "rich-text")]
pub mod color_edit;
#[cfg(feature = "rich-text")]
pub mod color_picker;
pub mod combo_box;
pub mod command_link_button;
pub mod common;
#[cfg(feature = "rich-text")]
pub mod date_edit;
#[cfg(feature = "rich-text")]
pub mod date_range_edit;
#[cfg(feature = "rich-text")]
pub mod date_time_edit;
pub mod dialog;
pub(crate) mod drag_preview;
#[cfg(feature = "rich-text")]
pub mod file_picker_field;
pub mod group_box;
pub mod group_header;
#[cfg(feature = "rich-text")]
pub mod hex_color_input;
pub mod icon_button;
#[cfg(feature = "rich-text")]
pub mod input_dialog;
pub mod keystroke_format;
pub mod link;
pub(crate) mod list_item_a11y;
pub(crate) mod list_source;
pub mod list_view;
pub mod menu_bar;
pub(crate) mod menu_context;
pub mod menu_item;
pub mod menu_list;
pub mod message_box;
pub mod notification;
pub(crate) mod overlay_trigger;
pub mod panel;
pub mod popover;
pub mod popover_button;
pub(crate) mod popover_caret;
pub mod popover_icon_button;
pub mod primitives;
#[cfg(feature = "telemetry")]
pub mod privacy_settings;
pub mod progress_bar;
pub mod radio_button;
pub mod radio_group;
pub mod repeater;
#[cfg(feature = "rich-text")]
pub mod rich_text;
pub mod scroll_area;
pub mod scroll_bar;
#[cfg(feature = "rich-text")]
pub mod search_field;
pub mod segmented_control;
pub mod shadow;
pub mod shortcut_settings;
pub mod slider;
pub mod snackbar;
#[cfg(feature = "rich-text")]
pub mod spin_box;
pub mod spinner;
pub mod split_button;
pub mod split_view;
pub mod standard_item;
pub mod status_bar;
pub mod styles;
pub mod tab_widget;
pub mod table_view;
#[cfg(feature = "rich-text")]
pub mod text_input;
#[cfg(feature = "rich-text")]
pub mod time_edit;
pub mod title_bar;
pub mod toast;
pub mod toggle;
pub mod tool_box;
pub mod toolbar;
pub mod tooltip;
pub mod tree_table;
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
pub use banner::{Banner, BannerSeverity};
pub use breadcrumb::{Breadcrumb, BreadcrumbItem};
pub use button::{Button, ButtonVariant, IconLocation};
pub use calendar::{Calendar, DateRange, WeekNumberDisplay};
pub use card::Card;
pub use checkbox::Checkbox;
#[cfg(feature = "rich-text")]
pub use color_edit::ColorEdit;
#[cfg(feature = "rich-text")]
pub use color_picker::{ColorPicker, ColorPickerLayout, ColorSwatch, DEFAULT_SWATCHES};
pub use combo_box::ComboBox;
pub use command_link_button::CommandLinkButton;
#[cfg(feature = "rich-text")]
pub use date_edit::DateEdit;
#[cfg(feature = "rich-text")]
pub use date_range_edit::DateRangeEdit;
#[cfg(feature = "rich-text")]
pub use date_time_edit::DateTimeEdit;
pub use dialog::{Dialog, DialogContent, ModalContainer, ModalScrim};
pub use fern_data::CheckState;
#[cfg(feature = "rich-text")]
pub use file_picker_field::{FilePickerField, FilePickerKind};
pub use group_box::GroupBox;
pub use group_header::GroupHeader;
#[cfg(feature = "rich-text")]
pub use hex_color_input::HexColorInput;
pub use icon_button::{BuiltInIcons, IconButton, IconButtonSize};
#[cfg(feature = "rich-text")]
pub use input_dialog::InputDialog;
pub use link::Link;
pub use list_view::ListView;
pub use menu_bar::MenuBar;
pub use menu_item::MenuItem;
pub use menu_list::{MenuList, MenuSeparator};
pub use message_box::{
    ButtonRole, EventContextMessageBoxExt, MessageBox, MessageBoxButton, MessageBoxButtons,
    MessageBoxResult, MessageBoxSeverity, StandardButton,
};
pub use notification::{
    ARCHIVE_FILE_NAME, ArchivedAction, ArchivedActionStyle, DEFAULT_ARCHIVE_LIMIT,
    NotificationArchive, NotificationArchiveModel, NotificationEntry, NotificationUpdate,
};
pub use panel::Panel;
pub use popover::Popover;
pub use popover_button::PopoverButton;
pub use popover_icon_button::PopoverIconButton;
#[cfg(feature = "rich-text")]
pub use primitives::TextInputField;
pub use primitives::{
    AspectRatio, Center, Divider, Expand, FixedSize, FormLayout, Grid, HStack, IconWidget,
    ImageFit, ImageWidget, MasonryLayout, MaxSize, MinSize, Padding, RectWidget, Spacer, Switcher,
    TextWidget, TrackSize, VStack, Wrap, ZStack,
};
#[cfg(feature = "telemetry")]
pub use privacy_settings::PrivacySettings;
pub use progress_bar::ProgressBar;
pub use radio_button::RadioButton;
pub use radio_group::RadioGroup;
pub use repeater::Repeater;
pub use scroll_area::{ScrollArea, ScrollBarMode, ScrollBarPolicy};
pub use scroll_bar::{ScrollBar, ScrollBarOrientation};
#[cfg(feature = "rich-text")]
pub use search_field::SearchField;
pub use segmented_control::SegmentedControl;
pub use shortcut_settings::ShortcutSettings;
pub use slider::Slider;
pub use snackbar::Snackbar;
#[cfg(feature = "rich-text")]
pub use spin_box::{
    ButtonLayout as SpinButtonLayout, SpinBox, SpinValue, StepType, WheelMode, WrapMode,
};
pub use spinner::Spinner;
pub use split_button::SplitButton;
pub use split_view::SplitView;
pub use standard_item::{StandardListItem, StandardTreeItem};
pub use status_bar::StatusBar;
pub use tab_widget::{
    ContextMenuFactory, IconFactory as TabIconFactory, STATIC_KIND, StaticContentFactory, TabBar,
    TabBarOrientation, TabDelegate, TabHandle, TabId, TabInfo, TabSizing, TabWidget,
};
pub use table_view::{
    Alignment as TableAlignment, CellContext, CellSelectionModel, Column, ColumnContext,
    ColumnResizePolicy, ColumnWidth, EditTrigger, GridLines, PinnedSide, SortDirection,
    TabTraversal, TableSelectionMode, TableView, TruncationPolicy,
};
#[cfg(feature = "rich-text")]
pub use text_input::{TextInput, ValidationState};
#[cfg(feature = "rich-text")]
pub use time_edit::{SecondsMode, TimeEdit, TimeFormat};
pub use title_bar::{DragRegion, ResizeStrip, TitleBar, WindowControls, WindowFrame};
pub use toast::{
    EventContextToastExt, Toast, ToastAction, ToastActionStyle, ToastDismissCause, ToastHandle,
    ToastHost, ToastInstallOptions, ToastPriority, ToastRegistry, ToastSeverity, ToastSurface,
};
pub use toggle::Toggle;
pub use tool_box::{ToolBox, ToolBoxItem};
pub use toolbar::Toolbar;
pub use tree_table::TreeTable;
pub use tree_view::{TreeRowContext, TreeView};
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
