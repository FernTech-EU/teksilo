#![allow(
// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
pub mod color_edit;
pub mod color_picker;
pub mod combo_box;
pub mod command_link_button;
pub mod common;
pub mod date_edit;
pub mod date_range_edit;
pub mod date_time_edit;
pub mod dialog;
pub mod docking;
pub(crate) mod data_views;
pub(crate) mod drag_preview;
pub mod drop_target;
pub mod drop_zone;
pub mod file_picker_field;
pub mod grid_view;
pub mod group_box;
pub mod group_header;
pub mod hex_color_input;
pub mod icon_button;
pub mod input_dialog;
pub mod keystroke_format;
pub mod link;
pub(crate) mod list_item_a11y;
pub(crate) mod list_source;
pub mod list_view;
pub mod menu;
pub mod menu_bar;
pub(crate) mod menu_context;
pub mod menu_item;
pub mod menu_list;
pub mod message_box;
pub mod notification;
pub(crate) mod overlay_trigger;
pub mod panel;
pub mod password_field;
pub mod popover;
pub(crate) mod popover_caret;
pub mod popover_widget;
pub mod primitives;
#[cfg(feature = "telemetry")]
pub mod privacy_settings;
pub mod progress_bar;
pub mod radio_button;
pub mod radio_group;
pub mod repeater;
pub mod rich_text;
pub mod scroll_area;
pub mod scroll_bar;
pub mod search_field;
pub mod segmented_control;
pub mod shadow;
pub mod shortcut_settings;
pub mod slider;
pub mod snackbar;
pub mod spin_box;
pub mod spinner;
pub mod split_button;
pub mod splitter;
pub mod standard_item;
pub mod status_bar;
pub mod stepper;
pub mod styles;
pub mod tab_widget;
pub mod table_view;
pub mod text_input;
pub mod time_edit;
pub mod title_bar;
pub mod toast;
pub mod toggle;
pub mod tool_box;
pub mod toolbar;
pub mod tooltip;
pub(crate) mod tree_source;
pub mod tree_table_view;
pub mod tree_view;

#[cfg(feature = "preview")]
mod preview_catalog;

pub use tooltip::TooltipWidget;

pub use accordion::{Accordion, AccordionOrientation};
pub use animations::{
    Blur, Collapse, Crossfade, Cycle, Fade, Pulse, Rotate, Scale, ScaleOrigin, Shake, Slide,
    SlideEdge, SmoothSize, SmoothSizeAxes,
};
pub use avatar::{Avatar, AvatarCorner, AvatarPresence, AvatarShape, AvatarSize};
pub use badge::Badge;
pub use banner::{Banner, BannerSeverity};
pub use bastyde_core::styles::{
    DropTargetDragState, DropTargetStyle, DropTargetStyleConfig, DropTargetVariant,
    SharedDropTargetStyle,
};
pub use bastyde_data::CheckState;
pub use breadcrumb::{Breadcrumb, BreadcrumbItem};
pub use button::{Button, ButtonVariant, IconLocation};
pub use calendar::{Calendar, DateRange, WeekNumberDisplay};
pub use card::Card;
pub use checkbox::Checkbox;
pub use color_edit::ColorEdit;
pub use color_picker::{ColorPicker, ColorPickerLayout, ColorSwatch, DEFAULT_SWATCHES};
pub use combo_box::ComboBox;
pub use command_link_button::CommandLinkButton;
pub use common::scroll::OverscrollBehavior;
pub use date_edit::DateEdit;
pub use date_range_edit::DateRangeEdit;
pub use date_time_edit::DateTimeEdit;
pub use dialog::{Dialog, DialogContent, ModalContainer, ModalScrim};
pub use docking::{
    CornerOwners, DockCorner, DockLayoutState, DockOpenLocation, DockOpenMode, DockPolicy, DockRail,
    DockRailItemSize, DockRailSlot, DockSide, DockTabDisplay, DockTabId, DockWidget, DockWidgetId,
    DockingLayout, DockingModel, TabPresentation,
};
pub use drop_target::DropTarget;
pub use drop_zone::DropZone;
pub use file_picker_field::{FilePickerField, FilePickerKind};
pub use grid_view::{
    GridSectionProvider, GridSizing, GridTabTraversal, GridView, GroupingSections, ScrollAnchor,
    TileContext, grouping_sections,
};
pub use group_box::GroupBox;
pub use group_header::GroupHeader;
pub use hex_color_input::HexColorInput;
pub use icon_button::{BuiltInIcons, IconButton, IconButtonSize};
pub use input_dialog::InputDialog;
pub use link::Link;
pub use list_view::ListView;
pub use menu::{
    MenuEntry, MenuItemState, MenuItems, MenuModel, MenuNode, NativeMenuMode, StandardMenu,
    StandardMenuRole,
};
pub use menu_bar::{CollapsePolicy, MenuBar};
pub use menu_item::MenuItem;
pub use menu_list::{MenuList, MenuSeparator};
pub use message_box::{
    ButtonRole, EventContextMessageBoxExt, MessageBox, MessageBoxButton, MessageBoxButtons,
    MessageBoxResult, MessageBoxSeverity, StandardButton,
};
pub use notification::{
    ARCHIVE_FILE_NAME, ArchivedAction, ArchivedActionStyle, DEFAULT_ARCHIVE_LIMIT,
    NotificationArchive, NotificationArchiveModel, NotificationCenterButton, NotificationEntry,
    NotificationLog, NotificationLogDialog, NotificationUpdate,
};
pub use panel::Panel;
pub use password_field::{AtRevealPolicy, EchoMode, PasswordField, RevealMode};
pub use popover::Popover;
pub use popover_widget::{PopoverButton, PopoverIconButton, PopoverTrigger, PopoverWidget};
pub use primitives::TextInputField;
pub use primitives::text_input_field::{ValidationFeedback, ValidationOutcome};
pub use primitives::{
    AspectRatio, Center, Divider, Expand, FixedSize, FormLayout, Grid, HStack, IconWidget,
    ImageFit, ImageWidget, MasonryLayout, MaxSize, MinSize, Padding, RectWidget, Shrinkable,
    Spacer, Switcher, TextWidget, TrackSize, VStack, Wrap, ZStack,
};
#[cfg(feature = "telemetry")]
pub use privacy_settings::PrivacySettings;
pub use progress_bar::ProgressBar;
pub use radio_button::RadioButton;
pub use radio_group::RadioGroup;
pub use repeater::Repeater;
pub use scroll_area::{ScrollArea, ScrollBarMode, ScrollBarPolicy};
pub use scroll_bar::{ScrollBar, ScrollBarOrientation};
pub use search_field::SearchField;
pub use segmented_control::{Segment, SegmentedControl};
pub use shortcut_settings::ShortcutSettings;
pub use slider::Slider;
pub use snackbar::Snackbar;
pub use spin_box::{
    ButtonLayout as SpinButtonLayout, SpinBox, SpinValue, StepType, WheelMode, WrapMode,
};
pub use spinner::Spinner;
pub use split_button::SplitButton;
pub use splitter::{
    PaneDescriptor, PaneSnapshot, PaneState, Splitter, SplitterModel, SplitterState,
};
// `Splitter`'s public API takes an `Orientation`; re-export it so callers
// don't need a separate `bastyde_tokens` import.
pub use bastyde_tokens::Orientation;
pub use standard_item::{StandardListItem, StandardTreeItem};
pub use status_bar::StatusBar;
pub use stepper::{
    ChromePosition, Step, StepStatus, Stepper, StepperController, StepperOrientation, Wizard,
};
pub use tab_widget::{
    ContextMenuFactory, IconFactory as TabIconFactory, STATIC_KIND, StaticContentFactory, TabBar,
    TabBarOrientation, TabBarVisibility, TabDelegate, TabDisplayMode, TabHandle, TabId, TabInfo,
    TabSizing, TabWidget,
};
pub use table_view::{
    Alignment as TableAlignment, CellContext, CellSelectionModel, Column, ColumnContext,
    ColumnResizePolicy, ColumnWidth, EditTrigger, GridLines, PinnedSide, SortDirection,
    TabTraversal, TableSelectionMode, TableView, TruncationPolicy,
};
pub use text_input::{TextInput, ValidationState};
pub use time_edit::{SecondsMode, TimeEdit, TimeFormat};
pub use title_bar::{DragRegion, ResizeStrip, TitleBar, WindowControls, WindowFrame};
pub use toast::{
    EventContextToastExt, Toast, ToastAction, ToastActionStyle, ToastDismissCause, ToastHandle,
    ToastHost, ToastInstallOptions, ToastPriority, ToastRegistry, ToastSeverity, ToastSurface,
};
pub use toggle::Toggle;
pub use tool_box::{ToolBox, ToolBoxItem, ToolBoxOrientation};
pub use toolbar::{
    Toolbar, ToolbarAction, ToolbarDisplayMode, ToolbarItem, ToolbarOrientation, ToolbarOverflow,
};
pub use tree_table_view::TreeTableView;
pub use tree_source::{TreeRow, TreeRowMeta};
pub use tree_view::{TreeRowContext, TreeView};

/// The framework bundle: bastyde-widgets' own translatable strings, grouped
/// by locale. Registered by applications via
/// `I18nConfig::framework_locales(bastyde_widgets::framework_locales())`
/// at startup.
///
/// bastyde-widgets currently ships `en-US` (source) and `fr-FR`. Keys
/// missing from a locale's bundle fall back to the en-US source via
/// fluent-bundle's per-key fallback. Applications that need a locale
/// bastyde-widgets doesn't ship can fill the gap with
/// `I18nConfig::override_widget_strings(...)`.
pub fn framework_locales() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("en-US", &[include_str!("../locales/en-US.ftl")]),
        ("fr-FR", &[include_str!("../locales/fr-FR.ftl")]),
    ]
}

#[cfg(test)]
mod layout_integration_tests;
