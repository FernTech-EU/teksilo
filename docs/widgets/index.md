<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Widget Catalog

Every widget shipped by `bastyde-widgets`, grouped by category. Each page links to its full rustdoc API reference.

## Layout primitives

- [AspectRatio](aspect_ratio.md) — AspectRatio — a single-child wrapper that constrains layout to a fixed
- [Center](center.md) — Center — a single-child wrapper that centers its child within the available
- [Divider](divider.md) — Divider — a themed separator line that visually partitions content
- [Expand](expand.md) — Expand — a layout modifier that claims slack space in a stack and
- [FixedSize](fixed_size.md) — FixedSize — a layout modifier that pins a child to its natural size,
- [FormLayout](form_layout.md) — FormLayout — a two-column settings or preferences form layout
- [Grid](grid.md) — Grid — a 2D layout container with explicit row and column tracks
- [HStack](hstack.md) — HStack — a horizontal layout container that distributes children left-to-right
- [IconWidget](icon_widget.md) — IconWidget — a vector or raster icon rendered at a configurable size
- [ImageMaskShape](image_mask.md) — Anti-aliased alpha masking for raster images — circle / rounded-square
- [ImageWidget](image_widget.md) — ImageWidget — displays a raster image (PNG, WebP) with a configurable
- [MasonryLayout](masonry.md) — MasonryLayout — a variable-height grid that packs children into the
- [MaxSize](max_size.md) — MaxSize — a layout modifier that caps a child to a maximum width and/or height
- [MinSize](min_size.md) — MinSize — a layout modifier that ensures a child reaches a minimum width and/or height
- [Padding](padding.md) — Padding — a single-child layout container that adds insets around its child
- [RectWidget](rect_widget.md) — RectWidget — a leaf widget that paints a filled and/or stroked rounded rectangle
- [Shrinkable](shrinkable.md) — Shrinkable — a layout modifier that allows its child to compress under an over-constraint
- [Spacer](spacer.md) — Spacer — an invisible, flexible gap that claims all available space on the
- [Switcher](switcher.md) — Switcher — a container that shows exactly one child page at a time
- [TextInputField](text_input_field.md) — `TextInputField` — editable single-line text surface primitive
- [TextWidget](text_widget.md) — TextWidget — a leaf widget that renders a localized text string
- [TwistArrow](twist_arrow.md) — TwistArrow — a small chevron that indicates and toggles a tree node's expansion
- [ValidationStrip](validation_strip.md) — ValidationStrip — a small inline message shown below a text field to
- [VStack](vstack.md) — VStack — a vertical layout container that distributes children top-to-bottom
- [Wrap](wrap.md) — Wrap — a horizontal flow layout that wraps children to the next line when
- [ZStack](zstack.md) — ZStack — a layout container that layers children on top of each other

## Widgets

- [Accordion](accordion.md) — Accordion — a collapsible section with a clickable header that shows or hides
- [ActivateOn](data_views.md) — Shared substrate for the data views' source-owned drag-and-drop + lazy
- [AttachedSide](shadow.md) — Layered drop-shadow helper for elevated surfaces
- [Avatar](avatar.md) — `Avatar` — circular (or rounded-square / square) user-identity widget
- [Badge](badge.md) — Badge — a pill-shaped label for tags, status indicators, and counts
- [Banner](banner.md) — Banner — persistent inline status strip (info / success / warning / error)
- [Breadcrumb](breadcrumb.md) — Breadcrumb — a navigational trail with automatic overflow into a `…` menu
- [Button](button.md) — Button — a labelled, activatable action trigger
- [Calendar](calendar.md) — `Calendar` — month-grid date picker, standalone widget
- [Card](card.md) — Card — a surface container with optional header, content, and footer slots
- [Checkbox](checkbox.md) — Checkbox — a two-state or tristate checkbox with an optional label
- [ColorEdit](color_edit.md) — `ColorEdit` — compact field-style color picker trigger that opens
- [ColorPicker](color_picker.md) — `ColorPicker` — embeddable composite color selector
- [ComboBox](combo_box.md) — ComboBox — dropdown selection widget
- [CommandLinkButton](command_link_button.md) — CommandLinkButton — large two-line button with icon, title, and
- [DateEdit](date_edit.md) — `DateEdit` — text input + calendar popover, bound to `Signal<Option<Date>>`
- [DateRangeEdit](date_range_edit.md) — `DateRangeEdit` — single unified control for picking a `DateRange`
- [DateTimeEdit](date_time_edit.md) — `DateTimeEdit` — single unified control for picking a `DateTime`
- [Dialog](dialog.md) — Modal dialogs — a trigger button that presents a centered modal panel
- [DockingLayout](docking.md) — `DockingLayout` — a VS Code-style dockable layout: a fixed centre slot
- [DropTarget](drop_target.md) — `DropTarget` — a transparent wrapping drop container
- [DropZone](drop_zone.md) — `DropZone` — a "drop files here" target for external (OS) drag-and-drop
- [FilePickerField](file_picker_field.md) — `FilePickerField` — a text-input preset for path entry with a Browse button
- [FocusScope](focus_scope.md) — `FocusScope` — a layout-transparent wrapper that declares a **traversal
- [GridView](grid_view.md) — Virtualized 2D tile grid bound to a `ListModel<T>` / `ListDataSource`
- [GroupBox](group_box.md) — GroupBox — titled cluster of controls in Int UI / Jewel style
- [GroupHeader](group_header.md) — GroupHeader — a horizontal section header: label followed by a trailing
- [HexColorInput](hex_color_input.md) — `HexColorInput` — single-line `#RRGGBB[AA]` color editor
- [IconButton](icon_button.md) — IconButton — a square, icon-only, flat-surface button
- [InputDialog](input_dialog.md) — InputDialog — a `QInputDialog`-style modal that prompts the user for
- [LanguageSwitcher](language_switcher.md) — LanguageSwitcher — a drop-in UI-language picker for settings screens
- [Link](link.md) — Link — a clickable text label rendered as underlined inline text
- [ListView](list_view.md) — ListView — a virtualized, scrollable list backed by a reactive data model
- [MenuBar](menu_bar.md) — MenuBar — a horizontal application menu bar with keyboard-driven dropdowns
- [MenuItem](menu_item.md) — MenuItem — a single command row in a menu or context menu
- [MenuList](menu_list.md) — MenuList — a themed vertical menu container with keyboard navigation
- [MessageBox](message_box.md) — MessageBox — QMessageBox-style alert dialog
- [NotificationEntry](notification.md) — Persistent notification archive — the storage and data-model layer
- [Panel](panel.md) — Panel — a themed single-child container that provides a background, border,
- [PasswordField](password_field.md) — `PasswordField` — secure single-line text entry with a reveal
- [Popover](popover.md) — `Popover` — a button that opens a floating panel anchored to itself
- [PopoverWidget](popover_widget.md) — `PopoverWidget<T>` — a generic trigger that opens a popover when
- [PrivacySettings](privacy_settings.md) — PrivacySettings — a user-facing panel for telemetry consent management
- [ProgressBar](progress_bar.md) — ProgressBar — a bar showing progress from 0.0 to 1.0
- [RadioButton](radio_button.md) — RadioButton — mutually exclusive selection control
- [RadioGroup](radio_group.md) — RadioGroup — invisible layout container that groups `RadioButton`s
- [Repeater](repeater.md) — Repeater — non-virtualized dynamic widget list driven by a `ListModel<T>`
- [RichTextEditor](rich_text.md) — Rich text editor and viewer widget
- [ScrollArea](scroll_area.md) — ScrollArea — a clipping viewport that scrolls its content on wheel, touch,
- [ScrollBar](scroll_bar.md) — ScrollBar — pointer and keyboard affordance for a `ScrollArea`
- [SearchField](search_field.md) — SearchField — a [`TextInput`] preset
- [SegmentedControl](segmented_control.md) — SegmentedControl — mutually exclusive segments in a horizontal row
- [ShortcutSettings](shortcut_settings.md) — ShortcutSettings — user-facing widget for browsing and rebinding
- [Slider](slider.md) — Slider — a draggable value selector bound to a `Signal<f32>`
- [Snackbar](snackbar.md) — Snackbar — a transient, button-triggered floating notification surface
- [SpinBox](spin_box.md) — `SpinBox` — numeric input with increment/decrement buttons
- [Spinner](spinner.md) — `Spinner` — a shader-driven circular-arc loading indicator
- [SplitButton](split_button.md) — SplitButton — a button split into two regions sharing a single frame
- [Splitter](splitter.md) — N-pane split container with draggable, collapsible dividers
- [StandardListItem](standard_item.md) — Canonical row layout for `ListView` / `TreeView` delegates
- [StatusBar](status_bar.md) — StatusBar — a horizontal chrome bar at the bottom of a window for status
- [Stepper](stepper.md) — [`Stepper`] — a modern, embeddable step-flow widget (Material/Ant/Flutter
- [TableView](table_view.md) — `TableView<T>` — generic, virtualized, accessible tabular widget
- [TabWidget](tab_widget.md) — Tabbed-container widgets
- [TextInput](text_input.md) — `TextInput` — styled single-line text field composite
- [TextScaleControl](text_scale_control.md) — [`TextScaleControl`] — the settings control that grows all text in the app
- [ThemeSwitcher](theme_switcher.md) — ThemeSwitcher — a drop-in app-theme picker for settings screens & toolbars
- [TimeEdit](time_edit.md) — `TimeEdit` — text input for time-of-day, bound to `Signal<Option<Time>>`
- [TitleBar](title_bar.md) — Custom window title bar widget
- [Toast](toast.md) — Toast notification — stackable, action-rich, severity-aware floating
- [Toggle](toggle.md) — Toggle — an animated on/off switch bound to a `Signal<bool>`
- [Toolbar](toolbar.md) — `Toolbar` — a command bar with automatic **overflow**
- [ToolBox](tool_box.md) — ToolBox — a vertical stack of collapsible sections, exactly one expanded
- [TooltipWidget](tooltip.md) — Tooltip system — hover-triggered overlays with configurable delay
- [TreeRowMeta](tree_source.md) — Type-erased data source adapter for `TreeView`
- [TreeTableView](tree_table_view.md) — `TreeTableView<T>` — hierarchical multi-column data table with expand/collapse
- [TreeView](tree_view.md) — TreeView — a virtualized, expandable/collapsible hierarchical list widget

## Animations

- [Blur](blur.md) — `Blur` — a wrapper widget that applies a Gaussian-equivalent blur
- [Collapse](collapse.md) — `Collapse` — a wrapper widget that animates its child between
- [Crossfade](crossfade.md) — `Crossfade` — when an external `Signal<K>` changes, the
- [Cycle](cycle.md) — `Cycle` — show one of N children at a time, advancing on a fixed
- [Fade](fade.md) — `Fade` — a wrapper widget that animates its child between hidden
- [Pulse](pulse.md) — `Pulse` — a wrapper widget that pulses its child's opacity between
- [Rotate](rotate.md) — `Rotate` — wraps a child and applies a 2D rotation to its entire
- [Scale](scale.md) — `Scale` — wraps a child and animates a uniform 2D scale on its
- [Shake](shake.md) — `Shake` — wraps a child and plays a damped horizontal oscillation
- [Slide](slide.md) — `Slide` — wraps a child and slides it in or out from a chosen
- [SmoothSize](smooth_size.md) — `SmoothSize` — auto-sizes the slot to fit the child's intrinsic
- [Unroll](unroll.md) — `Unroll` — the horizontal sibling of `Collapse`

## ColorPicker (submodule)

- [ColorSwatch](swatch.md) — `ColorSwatch` — single clickable color cell with `Role::ColorWell`

## Notification (submodule)

- [NotificationCenterButton](center_button.md) — `NotificationCenterButton` — bell icon with an unread-count badge that
- [NotificationLog](log.md) — `NotificationLog` — a scrollable, day-bucketed list of archived notifications

## Stepper (submodule)

- [Wizard](wizard.md) — [`Wizard`] — a thin modal launcher around [`Stepper`]

## TabWidget (submodule)

- [TabBar](bar.md) — `TabBar<T>` — header strip driven by a data source

## TitleBar (submodule)

- [DragRegion](drag_region.md) — `DragRegion` — flexible drag region inside a `TitleBar`
- [ResizeStrip](resize_strip.md) — A thin invisible widget that forwards a window resize gesture to the
- [WindowControls](controls.md) — The minimize / maximize / close button cluster on the trailing edge of
- [WindowFrame](window_frame.md) — A borderless-window frame: an invisible overlay of resize strips and

## Toast (submodule)

- [ToastHost](host.md) — `ToastHost` — invisible sibling widget that owns the toast queue
- [ToastSurface](surface.md) — `ToastSurface` — the rendered chrome of one toast
