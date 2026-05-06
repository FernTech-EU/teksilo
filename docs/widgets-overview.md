# FernUI Widget Catalog

A categorized index of every widget that ships in the workspace. One line
per widget; the source link is the authoritative reference. For full
public API surfaces (struct, builder methods, enums, module doc) of one
or more widgets, run `python3 tools/extract_widget_api.py <Widget…>`
or `--all` for everything.

Widgets gated behind the `rich-text` feature are marked **(rich-text)**;
the feature is on by default in the `fern-ui` umbrella crate.

For per-subsystem docs (data binding, accessibility overrides,
animation, drag-and-drop, multi-window, settings, i18n, theming,
shortcuts/intents/actions), see [SUMMARY.md](SUMMARY.md).

---

## Layout primitives — `crates/fern-widgets/src/primitives/`

The composable building blocks of every widget tree. See
[layout-primitives.md](layout-primitives.md) for the layout protocol,
slack distribution math, and worked examples.

- [HStack](../crates/fern-widgets/src/primitives/hstack.rs) — horizontal stack with cross-axis alignment, spacing, and slack distribution.
- [VStack](../crates/fern-widgets/src/primitives/vstack.rs) — vertical stack; same model.
- [ZStack](../crates/fern-widgets/src/primitives/zstack.rs) — overlay stack at a shared origin with two-axis alignment.
- [Grid](../crates/fern-widgets/src/primitives/grid.rs) — fixed/fr/auto track grid (`TrackSize`); explicit cell placement.
- [Wrap](../crates/fern-widgets/src/primitives/wrap.rs) — flow layout that wraps to new rows when out of width.
- [MasonryLayout](../crates/fern-widgets/src/primitives/masonry.rs) — variable-height grid packing into the shortest column (Pinterest-style).
- [FormLayout](../crates/fern-widgets/src/primitives/form_layout.rs) — labelled rows with column alignment for settings panels.
- [Center](../crates/fern-widgets/src/primitives/center.rs) — claims available space and centers a single child.
- [Expand](../crates/fern-widgets/src/primitives/expand.rs) — flex-basis-zero workhorse for ratio splits and full-bleed children.
- [Padding](../crates/fern-widgets/src/primitives/padding.rs) — uniform or per-edge inset around a single child.
- [Spacer](../crates/fern-widgets/src/primitives/spacer.rs) — flexible empty space that consumes slack via `flex = 1.0`.
- [Divider](../crates/fern-widgets/src/primitives/divider.rs) — 1 dp themed line, horizontal or vertical.
- [FixedSize](../crates/fern-widgets/src/primitives/fixed_size.rs) — pins width/height regardless of parent proposal.
- [MinSize](../crates/fern-widgets/src/primitives/min_size.rs) — clamps response to a floor (touch-target enforcement, etc.).
- [MaxSize](../crates/fern-widgets/src/primitives/max_size.rs) — clamps response to a ceiling.
- [AspectRatio](../crates/fern-widgets/src/primitives/aspect_ratio.rs) — constrains a child to a fixed width-to-height ratio.
- [Switcher](../crates/fern-widgets/src/primitives/switcher.rs) — shows one of N children, driven by `Signal<usize>`.

## Visual primitives

Direct draw surfaces with no internal composition.

- [RectWidget](../crates/fern-widgets/src/primitives/rect_widget.rs) — themed rectangle (background, border, corner radius); reactive bindings.
- [TextWidget](../crates/fern-widgets/src/primitives/text_widget.rs) — single-line text via the `TextBackend`; reactive content + color.
- [IconWidget](../crates/fern-widgets/src/primitives/icon_widget.rs) — vector icon rendered through the path atlas; `IconMode` for tinted vs. raw.
- [ImageWidget](../crates/fern-widgets/src/primitives/image_widget.rs) — bitmap with `ImageFit` (fill / contain / cover / none / scale-down).
- [ImageMask](../crates/fern-widgets/src/primitives/image_mask.rs) — CPU-side anti-aliased alpha mask (`ImageMaskShape`); used by Avatar and other shaped-image patterns.
- [ValidationStrip](../crates/fern-widgets/src/primitives/validation_strip.rs) **(rich-text)** — inline error/warning/success strip under a field.
- [TextInputField](../crates/fern-widgets/src/primitives/text_input_field.rs) **(rich-text)** — primitive single-line editable text used inside the higher-level field widgets.

---

## Containers and chrome

Themed framing, sectioning, and window-level structure.

- [Panel](../crates/fern-widgets/src/panel.rs) — themed background + border + corner radius + padding.
- [Card](../crates/fern-widgets/src/card.rs) — elevated panel with shadow and optional header/footer slots.
- [GroupBox](../crates/fern-widgets/src/group_box.rs) — labelled bordered group for related controls.
- [GroupHeader](../crates/fern-widgets/src/group_header.rs) — section header (label + trailing rule line) for settings forms.
- [Toolbar](../crates/fern-widgets/src/toolbar.rs) — dense horizontal action strip on a `surface_secondary` panel.
- [StatusBar](../crates/fern-widgets/src/status_bar.rs) — bottom-of-window status text strip with `Role::Status`.
- [Banner](../crates/fern-widgets/src/banner.rs) — persistent inline info / success / warning / error strip (`BannerSeverity`); `Role::Status` + `Live::Polite`.
- [Accordion](../crates/fern-widgets/src/accordion.rs) — vertically stacked collapsible sections, multiple-open allowed.
- [ToolBox](../crates/fern-widgets/src/tool_box.rs) — vertically stacked collapsible pages, exactly one expanded (Qt `QToolBox` analog).
- [ScrollArea](../crates/fern-widgets/src/scroll_area.rs) — viewport with overlay or permanent scrollbars (`ScrollBarMode`, `ScrollBarPolicy`).
- [ScrollBar](../crates/fern-widgets/src/scroll_bar.rs) — standalone scrollbar, drag/track-click/keyboard.
- [SplitView](../crates/fern-widgets/src/split_view.rs) — two-pane resizable splitter with drag handle.
- [TabWidget](../crates/fern-widgets/src/tab_widget.rs) — tab bar + content switcher; data-source-driven `TabBar<T>` underneath. See [tab-widget.md](tab-widget.md).
- [Wizard](../crates/fern-widgets/src/wizard.rs) — multi-step flow with header, footer, and step switching (`WizardStep`).
- [Breadcrumb](../crates/fern-widgets/src/breadcrumb.rs) — clickable path segments with chevron separators (`BreadcrumbItem`).
- [TitleBar](../crates/fern-widgets/src/title_bar.rs) — custom window title bar with drag region, resize strip, and window controls. See [title-bar.md](title-bar.md).

---

## Buttons

- [Button](../crates/fern-widgets/src/button.rs) — four styles (Filled / Outlined / Flat / Tonal) × five interaction states; `IconLocation` for leading/trailing icon. Reference exemplar — read the source.
- [IconButton](../crates/fern-widgets/src/icon_button.rs) — square icon-only button at five `IconButtonSize` steps (Compact / Default / Toolbar / Large / Hero). `.embedded()` mode for trailing-slot use inside fields. Includes `BuiltInIcons` factory.
- [CommandLinkButton](../crates/fern-widgets/src/command_link_button.rs) — large two-line CTA: leading icon + bold title + secondary description; flat surface.
- [PopoverButton](../crates/fern-widgets/src/popover_button.rs) — Button preset that opens a Popover when activated.
- [PopoverIconButton](../crates/fern-widgets/src/popover_icon_button.rs) — IconButton variant of the same.
- [SplitButton](../crates/fern-widgets/src/split_button.rs) — main action region + chevron region that opens a related-actions menu.

## Inputs and indicators

- [Checkbox](../crates/fern-widgets/src/checkbox.rs) — two-state and tristate (`CheckState`).
- [RadioButton](../crates/fern-widgets/src/radio_button.rs) — single radio, bound to a shared value via [RadioGroup](../crates/fern-widgets/src/radio_group.rs) for mutual exclusion.
- [Toggle](../crates/fern-widgets/src/toggle.rs) — switch-style on/off control.
- [Slider](../crates/fern-widgets/src/slider.rs) — horizontal or vertical, optional stepping.
- [SegmentedControl](../crates/fern-widgets/src/segmented_control.rs) — `Signal<usize>`-driven segmented chooser; `RadioGroup` AT role.
- [ComboBox](../crates/fern-widgets/src/combo_box.rs) — selection-only dropdown; virtualized via `ListView` past `max_visible_items`.
- [ProgressBar](../crates/fern-widgets/src/progress_bar.rs) — determinate or indeterminate; linear.
- [Spinner](../crates/fern-widgets/src/spinner.rs) — circular-arc loading indicator on the shader-driven `AnimatedQuadKind::SpinnerArc` pipeline; honours `prefers-reduced-motion`.
- [Link](../crates/fern-widgets/src/link.rs) — typographic hyperlink with hover and visited states.
- [Badge](../crates/fern-widgets/src/badge.rs) — passive count/label pill.
- [Avatar](../crates/fern-widgets/src/avatar.rs) — user identity (image / initials fallback / hash-derived tint); circular / rounded-square / square shapes; presence indicator with corner positioning.

---

## Text input family — **(rich-text)**

All gated behind the `rich-text` feature; on by default in `fern-ui`.

- [TextInput](../crates/fern-widgets/src/text_input.rs) — styled single-line input on top of `TextInputField`; `ValidationState`.
- [RichTextEditor](../crates/fern-widgets/src/rich_text.rs) — full editing surface with IME, formatting commands, undo/redo, intrinsic-mode sizing (`min_lines` / `max_lines`); also runs read-only as the rich-text viewer (`ScrollPolicy`).
- [SpinBox](../crates/fern-widgets/src/spin_box.rs) — numeric input with `WrapMode`, `StepType`, `ButtonLayout`, `WheelMode`, `WidthPolicy`.
- [SearchField](../crates/fern-widgets/src/search_field.rs) — TextInput preset with leading magnifier glyph and clear-X; `Role::SearchInput`.
- [FilePickerField](../crates/fern-widgets/src/file_picker_field.rs) — TextInput + Browse button wired to the native file dialog; `FilePickerKind::OpenFile / PickFolder / SaveFile`.
- [InputDialog](../crates/fern-widgets/src/input_dialog.rs) — single-field input modal: title + prompt + TextInput + Cancel/OK; `on_result` delivers `Some(value)` / `None`.

### Date and time

- [Calendar](../crates/fern-widgets/src/calendar.rs) — month grid with WAI-ARIA grid keyboard pattern; `CalendarMode::Single` / `Range` (`DateRange`); `WeekNumberDisplay` toggle. Locale-derived first day of week and format pattern.
- [DateEdit](../crates/fern-widgets/src/date_edit.rs) — date input with trailing calendar-icon trigger; `WidthPolicy`, `ValidationBehavior`.
- [TimeEdit](../crates/fern-widgets/src/time_edit.rs) — time input; `TimeFormat`, `SecondsMode`.
- [DateTimeEdit](../crates/fern-widgets/src/date_time_edit.rs) — combined date + time input.
- [DateRangeEdit](../crates/fern-widgets/src/date_range_edit.rs) — two-date range input.

### Color

- [HexColorInput](../crates/fern-widgets/src/hex_color_input.rs) — hex code text input with live swatch.
- [ColorEdit](../crates/fern-widgets/src/color_edit.rs) — compact color editor with swatch trigger.
- [ColorPicker](../crates/fern-widgets/src/color_picker.rs) — full HSV picker; `ColorPickerLayout` controls panel arrangement.

---

## Menus

- [MenuBar](../crates/fern-widgets/src/menu_bar.rs) — top-of-window menu strip; widget-based on Windows/Linux. (macOS native `NSMenu` integration tracked under §30 of the architecture doc.)
- [MenuList](../crates/fern-widgets/src/menu_list.rs) — overlay menu panel; accepts arbitrary `impl Widget` children. `MenuSeparator` for inline rules.
- [MenuItem](../crates/fern-widgets/src/menu_item.rs) — keyboard-highlightable menu row with `for_shortcut(id)` for live-rebinding labels.

## Overlays and dialogs

See [tooltips.md](tooltips.md) for the tooltip system.

- [TooltipWidget](../crates/fern-widgets/src/tooltip.rs) — plain or rich tooltips; sticky-on-dwell promotion to non-modal `Role::Dialog`; `TooltipRegistry` for app-wide reuse.
- [Popover](../crates/fern-widgets/src/popover.rs) — anchored overlay accepting arbitrary `impl Widget` content; configurable placement, dismissal, optional caret.
- [Dialog](../crates/fern-widgets/src/dialog.rs) — modal dialog frame; `DialogContent` / `ModalContainer` for content + presentation.
- [MessageBox](../crates/fern-widgets/src/message_box.rs) — predefined info/warning/error/question modals (`MessageBoxSeverity`); semantic-role buttons (`ButtonRole`, `StandardButton`, `MessageBoxButton`, `MessageBoxButtons`) with platform-aware ordering; result via `MessageBoxResult`.
- [Snackbar](../crates/fern-widgets/src/snackbar.rs) — queued auto-dismissing toast with animated slide-in.
- [Shadow](../crates/fern-widgets/src/shadow.rs) — drop-shadow primitive used by elevated surfaces (`AttachedSide` for one-sided shadows).

---

## Data-driven widgets

Backed by the `fern-data` reactive collections. See [data-models.md](data-models.md) for the underlying `ListModel<T>` / `TreeModel<T>` / `SelectionModel` / sort-filter projections.

- [Repeater](../crates/fern-widgets/src/repeater.rs) — non-virtualized siblings driven by `ListModel<T>` change notifications; for small bounded collections.
- [ListView](../crates/fern-widgets/src/list_view.rs) — virtualized vertical list for large/unbounded collections.
- [TreeView](../crates/fern-widgets/src/tree_view.rs) — hierarchical list with twist-arrow expand/collapse.
- [TableView](../crates/fern-widgets/src/table_view.rs) — multi-column, virtualized; sort/filter via `SortFilterListModel`, drag-resize and drag-reorder columns, pinned Leading/Trailing, cell + row selection, edit hooks, row drag-drop reorder, full `Role::Table` AT tree. See [table-view.md](table-view.md).
- [TreeTable](../crates/fern-widgets/src/tree_table.rs) — hierarchical multi-column variant of TableView; `Role::TreeGrid`.

## Charts — `crates/fern-charts/src/`

Sits at the same tier as `fern-widgets` (no dep on widgets). See [charts.md](charts.md).

- [BarChart](../crates/fern-charts/src/bar_chart.rs) — vertical or horizontal bars; single or grouped series; optional value labels, axis labels, grid lines.
- [LineChart](../crates/fern-charts/src/line_chart.rs) — points connected by polylines; single or multiple series; optional area fill; hover tooltips on data points.
- [PieChart](../crates/fern-charts/src/pie_chart.rs) — pie + donut variants; donut variant has a center slot.

Shared infrastructure ([series.rs](../crates/fern-charts/src/series.rs), [axis.rs](../crates/fern-charts/src/axis.rs), [legend.rs](../crates/fern-charts/src/legend.rs), [palette.rs](../crates/fern-charts/src/palette.rs), [layout.rs](../crates/fern-charts/src/layout.rs)) is reused across all three.

---

## Animation wrappers — `crates/fern-widgets/src/animations/`

Wrappers that animate a child subtree without the caller managing scheduler state. See [animation.md](animation.md) for `Signal<f32>::animate_to` and the underlying scheduler.

- [Fade](../crates/fern-widgets/src/animations/fade.rs) — opacity tween 0↔1; layout-transparent.
- [Pulse](../crates/fern-widgets/src/animations/pulse.rs) — sine-driven looping opacity oscillation (recording-indicator pattern).
- [Cycle](../crates/fern-widgets/src/animations/cycle.rs) — cycles through children on a fixed period.
- [Crossfade](../crates/fern-widgets/src/animations/crossfade.rs) — keyed builder; old fades to new on key change.
- [Collapse](../crates/fern-widgets/src/animations/collapse.rs) — height-collapse tween used by Accordion and disclosure patterns.
- [SmoothSize](../crates/fern-widgets/src/animations/smooth_size.rs) — auto-sizes to the child's intrinsic size and animates every change (`SmoothSizeAxes`).
- [Slide](../crates/fern-widgets/src/animations/slide.rs) — slides a child in/out from a chosen edge (`SlideEdge`); layout-stable.
- [Shake](../crates/fern-widgets/src/animations/shake.rs) — damped horizontal oscillation triggered by a `Signal<u32>` bump (invalid-input feedback).
- [Scale](../crates/fern-widgets/src/animations/scale.rs) — uniform 2D scale 0↔1 (`ScaleOrigin`); visual-only by default, optional layout-driving mode.
- [Rotate](../crates/fern-widgets/src/animations/rotate.rs) — rotates a child subtree by a `Prop<f32>` angle in radians.
- [Blur](../crates/fern-widgets/src/animations/blur.rs) — Gaussian-equivalent blur on the child subtree via dual-Kawase chain; sub-perceptual radii are zero-cost.

---

## Settings widgets

Pre-built UI for common app-level concerns.

- [ShortcutSettings](../crates/fern-widgets/src/shortcut_settings.rs) — full keyboard-shortcut rebind UI (Rebind / Reset / conflict auto-unbind / key capture). See [shortcut-intent-action.md](shortcut-intent-action.md).
- [PrivacySettings](../crates/fern-widgets/src/privacy_settings.rs) **(`telemetry` feature)** — consent toggles for telemetry adapters; ties into the [telemetry.md](telemetry.md) consent gate.

---

## Cross-references

- Layout protocol and slack distribution: [layout-primitives.md](layout-primitives.md)
- Events, gestures, focus, attached handlers: [events-and-gestures.md](events-and-gestures.md)
- Accessibility overrides on every widget: [accessibility-overrides.md](accessibility-overrides.md)
- Animation scheduler and `MotionTokens`: [animation.md](animation.md)
- Theming and role-based color resolution: [reactive-theme.md](reactive-theme.md)
- Drag and drop integration: [drag-and-drop.md](drag-and-drop.md)
- Settings persistence (`SettingsStore`, `SettingsFile<T>`, `MruList<T>`): [settings.md](settings.md)
- i18n (`tr!`, `tr_signal!`, locale-aware formatting): [i18n.md](i18n.md)
- Shortcuts / intents / actions: [shortcut-intent-action.md](shortcut-intent-action.md)
- Multi-window orchestration: [multi-window.md](multi-window.md)
- Inspector for runtime introspection: [inspector.md](inspector.md)
- Framework internals (Canvas, rendering pipeline, threading, testability): [fern-ui-architecture.md](fern-ui-architecture.md)
- Full per-widget API extraction: `python3 tools/extract_widget_api.py <Widget…>` or `--all`
