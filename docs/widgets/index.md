<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Widget Catalog

Every public widget in `teksilo-widgets`, grouped by category. Each page links to its full rustdoc API reference.

## Layout primitives: `crates/teksilo-widgets/src/primitives/`

- [AspectRatio](aspect_ratio.md) — AspectRatio — a single-child wrapper that constrains layout to a fixed width-to-height ratio
- [Center](center.md) — Center — a single-child wrapper that centers its child within the available space
- [ColumnFlow](column_flow.md) — `ColumnFlow` — flows children into as many columns as the width affords, re-partitioning every child when a column is gained or lost
- [DeadZone](dead_zone.md) — `DeadZone` — a gesture **dead zone** wrapper
- [Divider](divider.md) — Divider — a themed separator line that visually partitions content
- [Expand](expand.md) — Expand — a layout modifier that claims slack space in a stack and stretches its child to fill the allocated bounds
- [FixedSize](fixed_size.md) — FixedSize — a layout modifier that pins a child to its natural size, optionally overriding one or both dimensions with a reactive value
- [FocusScope](focus_scope.md) — `FocusScope` — a layout-transparent wrapper that declares a **traversal boundary** for Tab / Shift+Tab focus cycling
- [FormLayout](form_layout.md) — FormLayout — a two-column settings or preferences form layout
- [Grid](grid.md) — Grid — a 2D layout container with explicit row and column tracks
- [HStack](hstack.md) — HStack — a horizontal layout container that distributes children left-to-right
- [MasonryLayout](masonry.md) — MasonryLayout — a variable-height grid that packs children into the shortest column (Pinterest-style)
- [MaxSize](max_size.md) — MaxSize — a layout modifier that caps a child to a maximum width and/or height
- [MinSize](min_size.md) — MinSize — a layout modifier that ensures a child reaches a minimum width and/or height
- [Padding](padding.md) — Padding — a single-child layout container that adds insets around its child
- [Shrinkable](shrinkable.md) — Shrinkable — a layout modifier that allows its child to compress under an over-constraint
- [Spacer](spacer.md) — Spacer — an invisible, flexible gap that claims all available space on the container's main axis
- [Switcher](switcher.md) — Switcher — a container that shows exactly one child page at a time
- [TouchTarget](touch_target.md) — `TouchTarget` — the last resort of the three hit-targeting mechanisms: the one that actually moves things
- [VStack](vstack.md) — VStack — a vertical layout container that distributes children top-to-bottom
- [Wrap](wrap.md) — Wrap — a horizontal flow layout that wraps children to the next line when they exceed the available width
- [ZStack](zstack.md) — ZStack — a layout container that layers children on top of each other

## Visual primitives

- [IconWidget](icon_widget.md) — IconWidget — a vector or raster icon rendered at a configurable size
- [ImageMaskShape](image_mask.md) — Anti-aliased alpha masking for raster images — circle / rounded-square / square coverage applied in-place to RGBA8 pixel buffers
- [ImageWidget](image_widget.md) — ImageWidget — displays a raster image (PNG, WebP) with a configurable sizing policy, content-fit mode, and intra-box alignment
- [LiveImage](live_image.md) — LiveImage — shows a picture another thread rewrites many times a second
- [RectWidget](rect_widget.md) — RectWidget — a leaf widget that paints a filled and/or stroked rounded rectangle
- [TextInputField](text_input_field.md) — `TextInputField` — editable single-line text surface primitive
- [TextWidget](text_widget.md) — TextWidget — a leaf widget that renders a localized text string
- [TwistArrow](twist_arrow.md) — TwistArrow — a small chevron that indicates and toggles a tree node's expansion
- [ValidationStrip](validation_strip.md) — ValidationStrip — a small inline message shown below a text field to surface a validation outcome

## Containers and chrome

- [Accordion](accordion.md) — Accordion — a collapsible section with a clickable header that shows or hides its content when activated
- [Banner](banner.md) — Banner — persistent inline status strip (info / success / warning / error)
- [Breadcrumb](breadcrumb.md) — Breadcrumb — a navigational trail with automatic overflow into a `…` menu
- [Card](card.md) — Card — a surface container with optional header, content, and footer slots
- [DockingLayout](docking.md) — `DockingLayout` — a VS Code-style dockable layout: a fixed centre slot (the app's main content) surrounded by four collapsible, splittable, draggable side…
- [DropTarget](drop_target.md) — `DropTarget` — a transparent wrapping drop container
- [DropZone](drop_zone.md) — `DropZone` — a "drop files here" target for external (OS) drag-and-drop
- [GroupBox](group_box.md) — GroupBox — titled cluster of controls in Int UI / Jewel style
- [GroupHeader](group_header.md) — GroupHeader — a horizontal section header: label followed by a trailing rule line that fills the remaining width
- [Panel](panel.md) — Panel — a themed single-child container that provides a background, border, corner radius, and padding
- [ScrollArea](scroll_area.md) — ScrollArea — a clipping viewport that scrolls its content on wheel, on a finger's pan, and on assistive-technology actions
- [ScrollBar](scroll_bar.md) — ScrollBar — pointer and keyboard affordance for a `ScrollArea`
- [Splitter](splitter.md) — N-pane split container with draggable, collapsible dividers
- [StatusBar](status_bar.md) — StatusBar — a horizontal chrome bar at the bottom of a window for status information
- [Stepper](stepper.md) — `Stepper` — a modern, embeddable step-flow widget (Material/Ant/Flutter "stepper"), and `Wizard`, a thin modal launcher built on it
- [TabWidget](tab_widget.md) — Tabbed-container widgets
- [TitleBar](title_bar.md) — Custom window title bar widget
- [Toolbar](toolbar.md) — `Toolbar` — a command bar with automatic **overflow**
- [ToolBox](tool_box.md) — ToolBox — a vertical stack of collapsible sections, exactly one expanded at a time
- [Wizard](wizard.md) — `Wizard` — a thin modal launcher around `Stepper`

## Buttons

- [Button](button.md) — Button — a labelled, activatable action trigger
- [CommandLinkButton](command_link_button.md) — CommandLinkButton — large two-line button with icon, title, and subtitle
- [IconButton](icon_button.md) — IconButton — a square, icon-only, flat-surface button
- [OverlayTrigger](overlay_trigger.md) — `OverlayTrigger` — the shared "this widget opens that overlay" wrapper
- [SplitButton](split_button.md) — SplitButton — a button split into two regions sharing a single frame

## Inputs and indicators

- [Avatar](avatar.md) — `Avatar` — circular (or rounded-square / square) user-identity widget
- [Badge](badge.md) — Badge — a pill-shaped label for tags, status indicators, and counts
- [Checkbox](checkbox.md) — Checkbox — a two-state or tristate checkbox with an optional label
- [ComboBox](combo_box.md) — ComboBox — dropdown selection widget
- [FontPicker](font_picker.md) — FontPicker — a drop-in font-family selector
- [Link](link.md) — Link — a clickable text label rendered as underlined inline text
- [ProgressBar](progress_bar.md) — ProgressBar — a bar showing progress from 0.0 to 1.0
- [RadioButton](radio_button.md) — RadioButton — mutually exclusive selection control
- [RadioGroup](radio_group.md) — RadioGroup — invisible layout container that groups `RadioButton`s and wires their accessibility metadata
- [RadioTile](radio_tile.md) — RadioTile — a "selectable card" radio option
- [RadioTileGroup](radio_tile_group.md) — RadioTileGroup — an N-ary group of `RadioTile`s with single selection
- [SegmentedControl](segmented_control.md) — SegmentedControl — mutually exclusive segments in a horizontal row
- [Slider](slider.md) — Slider — a draggable value selector bound to a `Signal<f32>`
- [Spinner](spinner.md) — `Spinner` — a shader-driven circular-arc loading indicator
- [Toggle](toggle.md) — Toggle — an animated on/off switch bound to a `Signal<bool>`

## Text input family

- [CodeEditor](widget.md) — The public editing surfaces: `CodeEditor` and `PlainTextEditor`
- [FilePickerField](file_picker_field.md) — `FilePickerField` — a text-input preset for path entry with a Browse button
- [InputDialog](input_dialog.md) — InputDialog — a `QInputDialog`-style modal that prompts the user for a single string
- [LogView](log_view.md) — `LogView` — a read-only, append-only, tail-following streaming view
- [PasswordField](password_field.md) — `PasswordField` — secure single-line text entry with a reveal toggle, masking, Caps Lock warning, and clipboard protection
- [RichTextEditor](rich_text.md) — Rich text editor and viewer widget
- [SearchField](search_field.md) — SearchField — a `TextInput` preset configured for search workflows: leading magnifier glyph, default-on clear-X, and an optional anchored suggestions…
- [SpinBox](spin_box.md) — `SpinBox` — numeric input with increment/decrement buttons
- [TextInput](text_input.md) — `TextInput` — styled single-line text field composite

## Date and time

- [Calendar](calendar.md) — `Calendar` — month-grid date picker, standalone widget
- [DateEdit](date_edit.md) — `DateEdit` — text input + calendar popover, bound to `Signal<Option<Date>>`
- [DateRangeEdit](date_range_edit.md) — `DateRangeEdit` — single unified control for picking a `DateRange`
- [DateTimeEdit](date_time_edit.md) — `DateTimeEdit` — single unified control for picking a `DateTime`
- [TimeEdit](time_edit.md) — `TimeEdit` — text input for time-of-day, bound to `Signal<Option<Time>>`

## Color

- [ColorEdit](color_edit.md) — `ColorEdit` — compact field-style color picker trigger that opens a popover containing a `ColorPicker`
- [ColorPicker](color_picker.md) — `ColorPicker` — embeddable composite color selector
- [ColorSwatch](swatch.md) — `ColorSwatch` — single clickable color cell with `Role::ColorWell`
- [HexColorInput](hex_color_input.md) — `HexColorInput` — single-line `#RRGGBB[AA]` color editor

## Menus

- [MenuBar](menu_bar.md) — MenuBar — a horizontal application menu bar with keyboard-driven dropdowns
- [MenuItem](menu_item.md) — MenuItem — a single command row in a menu or context menu
- [MenuList](menu_list.md) — MenuList — a themed vertical menu container with keyboard navigation

## Overlays and dialogs

- [AttachedSide](shadow.md) — Layered drop-shadow helper for elevated surfaces
- [CommandPalette](command_palette.md) — CommandPalette — type-to-run access to every command an app has registered
- [Dialog](dialog.md) — Modal dialogs — a trigger button that presents a centered modal panel
- [MessageBox](message_box.md) — MessageBox — QMessageBox-style alert dialog
- [NotificationCenterButton](center_button.md) — `NotificationCenterButton` — bell icon with an unread-count badge that opens a `NotificationLog` popover when clicked
- [NotificationLog](log.md) — `NotificationLog` — a scrollable, day-bucketed list of archived notifications
- [PopoverWidget](popover_widget.md) — `PopoverWidget<T>` — a generic trigger that opens a popover when activated, plus the `PopoverButton` / `PopoverIconButton` aliases
- [Snackbar](snackbar.md) — Snackbar — a transient, button-triggered floating notification surface
- [Toast](toast.md) — Toast notification — stackable, action-rich, severity-aware floating notification (the "upgrade path" from `Snackbar`)
- [ToastHost](host.md) — `ToastHost` — invisible sibling widget that owns the toast queue
- [TooltipWidget](tooltip.md) — Tooltip system — hover-triggered overlays with configurable delay

## Data-driven widgets

- [GridView](grid_view.md) — Virtualized 2D tile grid bound to a `ListModel<T>` / `ListDataSource`
- [ListView](list_view.md) — ListView — a virtualized, scrollable list backed by a reactive data model
- [Repeater](repeater.md) — Repeater — non-virtualized dynamic widget list driven by a `ListModel<T>`
- [StandardListItem](standard_item.md) — Canonical row layout for `ListView` / `TreeView` delegates
- [TableView](table_view.md) — `TableView<T>` — generic, virtualized, accessible tabular widget
- [TreeTableView](tree_table_view.md) — `TreeTableView<T>` — hierarchical multi-column data table with expand/collapse
- [TreeView](tree_view.md) — TreeView — a virtualized, expandable/collapsible hierarchical list widget

## Animation wrappers: `crates/teksilo-widgets/src/animations/`

- [Blur](blur.md) — `Blur` — a wrapper widget that applies a Gaussian-equivalent blur to its child subtree, driven by a `Prop<f32>` radius (in logical pixels)
- [Collapse](collapse.md) — `Collapse` — a wrapper widget that animates its child between hidden and natural size when an external `Signal<bool>` toggles
- [Crossfade](crossfade.md) — `Crossfade` — when an external `Signal<K>` changes, the previous content fades out while the new content fades in over the same window
- [Cycle](cycle.md) — `Cycle` — show one of N children at a time, advancing on a fixed period
- [Fade](fade.md) — `Fade` — a wrapper widget that animates its child between hidden (opacity 0) and visible (opacity 1) when an external `Signal<bool>` toggles
- [Pulse](pulse.md) — `Pulse` — a wrapper widget that pulses its child's opacity between a `min` and `max` value on a fixed period, sine-shaped
- [Rotate](rotate.md) — `Rotate` — wraps a child and applies a 2D rotation to its entire subtree, driven by an external `Prop<f32>` of radians
- [Scale](scale.md) — `Scale` — wraps a child and animates a uniform 2D scale on its entire subtree when an external `Prop<bool>` toggles
- [Shake](shake.md) — `Shake` — wraps a child and plays a damped horizontal oscillation whenever an external trigger `Signal<u32>` is bumped
- [Slide](slide.md) — `Slide` — wraps a child and slides it in or out from a chosen edge when an external `Signal<bool>` toggles
- [SmoothSize](smooth_size.md) — `SmoothSize` — auto-sizes the slot to fit the child's intrinsic size, but tweens the change instead of jumping
- [Unroll](unroll.md) — `Unroll` — the horizontal sibling of `Collapse`

## Settings widgets

- [LanguageSwitcher](language_switcher.md) — LanguageSwitcher — a drop-in UI-language picker for settings screens
- [PrivacySettings](privacy_settings.md) — PrivacySettings — a user-facing panel for telemetry consent management
- [ShortcutSettings](shortcut_settings.md) — ShortcutSettings — user-facing widget for browsing and rebinding application shortcuts
- [TextScaleControl](text_scale_control.md) — `TextScaleControl` — the settings control that grows all text in the app
- [ThemeSwitcher](theme_switcher.md) — ThemeSwitcher — a drop-in app-theme picker for settings screens & toolbars

## Other

- [ActivateOn](data_views.md) — Shared substrate for the data views' source-owned drag-and-drop + lazy loading
- [CodeEditorHandle](code_editor.md) — Multi-line plain-text and code editing surfaces
- [NotificationEntry](notification.md) — Persistent notification archive — the storage and data-model layer backing `NotificationLog`, `NotificationCenterButton`, and `NotificationLogDialog`
- [PopoverSurface](popover_surface.md) — `PopoverSurface` — the themed panel a popover's content sits in
- [TreeRowMeta](tree_source.md) — Type-erased data source adapter for `TreeView`

## TabWidget (submodule)

- [TabBar](bar.md) — `TabBar<T>` — header strip driven by a data source

## TitleBar (submodule)

- [DragRegion](drag_region.md) — `DragRegion` — flexible drag region inside a `TitleBar`
- [ResizeStrip](resize_strip.md) — A thin invisible widget that forwards a window resize gesture to the platform host when the user presses the primary button inside it
- [WindowControls](controls.md) — The minimize / maximize / close button cluster on the trailing edge of a `TitleBar`
- [WindowFrame](window_frame.md) — A borderless-window frame: an invisible overlay of resize strips and corner cells along the four edges of a single content widget

## Toast (submodule)

- [ToastSurface](surface.md) — `ToastSurface` — the rendered chrome of one toast
