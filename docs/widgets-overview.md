# Bastyde Widget Catalog

A categorized index of every widget that ships in the workspace. One line
per widget; the source link is the authoritative reference. For full
public API surfaces (struct, builder methods, enums, module doc) of one
or more widgets, run `python3 tools/extract_widget_api.py <Widget…>`
or `--all` for everything.

Widgets gated behind the `rich-text` feature are marked **(rich-text)**;
the feature is on by default in the `bastyde` umbrella crate.

For per-subsystem docs (data binding, accessibility overrides,
animation, drag-and-drop, multi-window, settings, i18n, theming,
shortcuts/intents/actions), see [SUMMARY.md](SUMMARY.md).

## Styling status

All 16 themable widgets are on the four-tier styling system
([docs/styling-system.md](styling-system.md)): each ships a `*Style`
trait in `bastyde-core::styles::*` plus a default `Recipe*Style` impl in
`bastyde-widgets/src/styles/*`. The widget builds its parts, hands a
`*StyleConfig` to the active style, and uses the returned `WidgetId`
as its root child — no themable widget self-paints. Style resolution
is per-call `.style(impl FooStyle)` → theme-wide
`theme.style_slots.<slot>` → recipe default.

| Widget | Variant enum | Style trait | Slot |
| --- | --- | --- | --- |
| `Toggle` | `ToggleVariant` (Switch/Pill/Square/Inset) | `ToggleStyle` | `style_slots.toggle` |
| `Button` | `ButtonVariant` (Filled/Tinted/Outlined/Plain/Ghost/Link/Destructive) | `ButtonStyle` | `style_slots.button` |
| `Checkbox` | `CheckboxVariant` (Square/Rounded/Circle) | `CheckboxStyle` | `style_slots.checkbox` |
| `RadioButton` | `RadioVariant` (Circle/Square/Rounded) | `RadioStyle` | `style_slots.radio` |
| `IconButton` | `IconButtonSize` (Compact/Default/Toolbar/Large/Hero) | `IconButtonStyle` | `style_slots.icon_button` |
| `Panel` | `PanelVariant` (Plain/Sunken/Raised/Highlighted) | `PanelStyle` | `style_slots.panel` |
| `Card` | `CardVariant` (Plain/Elevated/Outlined/Filled) | `CardStyle` | `style_slots.card` |
| `TooltipWidget` | — | `TooltipStyle` | `style_slots.tooltip` |
| `MenuItem` | — | `MenuItemStyle` | `style_slots.menu_item` |
| `StandardListItem` / `StandardTreeItem` | — | `StandardItemStyle` | `style_slots.standard_item` |
| `Popover` | `PopoverVariant` (Default/Menu/Tooltip) — surface | `PopoverStyle` | `style_slots.popover` |
| `ScrollBar` | `ScrollBarVariant` (Permanent/Overlay/Thin) + `ScrollBarOrientation` | `ScrollBarStyle` | `style_slots.scroll_bar` |
| `TabBar` | — (carries `TabBarOrientation`) | `TabStyle` | `style_slots.tab` |
| `ComboBox` | `ComboBoxVariant` (Outlined/Filled/Underline/Plain) | `ComboBoxStyle` | `style_slots.combo_box` |
| `Slider` | `SliderVariant` (Continuous/Discrete/Range) + `SliderOrientation` | `SliderStyle` | `style_slots.slider` |
| `TextInput` | `TextInputVariant` (Outlined/Filled/Underline/Bare) | `TextInputStyle` | `style_slots.text_input` |

The 17 legacy per-widget dimension structs in `bastyde-tokens::components`
were deleted (Step 7); their IntUI constants now live in the matching
`bastyde-widgets/src/styles/recipe_*_style.rs` modules. `theme.components`
still carries dimension data for the *non-themable* widgets (toolbar,
status bar, dialog, accordion, badge, progress bar, table, …). Image-
backed styles, the `ImageTheme` TOML loader, and the sibling preset
crates are still pending.

End-to-end demo of the slot bag + per-call override: see
[`examples/theme_styles/`](../examples/theme_styles/).

---

## Layout primitives — `crates/bastyde-widgets/src/primitives/`

The composable building blocks of every widget tree. See
[layout-primitives.md](layout-primitives.md) for the layout protocol,
slack distribution math, and worked examples.

- [HStack](../crates/bastyde-widgets/src/primitives/hstack.rs) — horizontal stack with cross-axis alignment, spacing, and slack distribution.
- [VStack](../crates/bastyde-widgets/src/primitives/vstack.rs) — vertical stack; same model.
- [ZStack](../crates/bastyde-widgets/src/primitives/zstack.rs) — overlay stack at a shared origin with two-axis alignment.
- [Grid](../crates/bastyde-widgets/src/primitives/grid.rs) — fixed/fr/auto track grid (`TrackSize`); explicit cell placement.
- [Wrap](../crates/bastyde-widgets/src/primitives/wrap.rs) — flow layout that wraps to new rows when out of width.
- [MasonryLayout](../crates/bastyde-widgets/src/primitives/masonry.rs) — variable-height grid packing into the shortest column (Pinterest-style).
- [FormLayout](../crates/bastyde-widgets/src/primitives/form_layout.rs) — labelled rows with column alignment for settings panels.
- [Center](../crates/bastyde-widgets/src/primitives/center.rs) — claims available space and centers a single child.
- [Expand](../crates/bastyde-widgets/src/primitives/expand.rs) — flex-basis-zero workhorse for ratio splits and full-bleed children.
- [Padding](../crates/bastyde-widgets/src/primitives/padding.rs) — uniform or per-edge inset around a single child.
- [Spacer](../crates/bastyde-widgets/src/primitives/spacer.rs) — flexible empty space that consumes slack via `flex = 1.0`.
- [Divider](../crates/bastyde-widgets/src/primitives/divider.rs) — 1 dp themed line, horizontal or vertical.
- [FixedSize](../crates/bastyde-widgets/src/primitives/fixed_size.rs) — pins width/height regardless of parent proposal.
- [MinSize](../crates/bastyde-widgets/src/primitives/min_size.rs) — clamps response to a floor (touch-target enforcement, etc.).
- [MaxSize](../crates/bastyde-widgets/src/primitives/max_size.rs) — clamps response to a ceiling.
- [AspectRatio](../crates/bastyde-widgets/src/primitives/aspect_ratio.rs) — constrains a child to a fixed width-to-height ratio.
- [Switcher](../crates/bastyde-widgets/src/primitives/switcher.rs) — shows one of N children, driven by `Signal<usize>`.

## Visual primitives

Direct draw surfaces with no internal composition.

- [RectWidget](../crates/bastyde-widgets/src/primitives/rect_widget.rs) — themed rectangle (background, border, corner radius); reactive bindings.
- [TextWidget](../crates/bastyde-widgets/src/primitives/text_widget.rs) — single-line text via the `TextBackend`; reactive content + color.
- [IconWidget](../crates/bastyde-widgets/src/primitives/icon_widget.rs) — vector icon rendered through the path atlas; `IconMode` for tinted vs. raw.
- [ImageWidget](../crates/bastyde-widgets/src/primitives/image_widget.rs) — bitmap with `ImageFit` (fill / contain / cover / none / scale-down).
- [ImageMask](../crates/bastyde-widgets/src/primitives/image_mask.rs) — CPU-side anti-aliased alpha mask (`ImageMaskShape`); used by Avatar and other shaped-image patterns.
- [ValidationStrip](../crates/bastyde-widgets/src/primitives/validation_strip.rs) **(rich-text)** — inline error/warning/success strip under a field.
- [TextInputField](../crates/bastyde-widgets/src/primitives/text_input_field.rs) **(rich-text)** — primitive single-line editable text used inside the higher-level field widgets.

---

## Containers and chrome

Themed framing, sectioning, and window-level structure.

- [Panel](../crates/bastyde-widgets/src/panel.rs) — themed background + border + corner radius + padding.
- [Card](../crates/bastyde-widgets/src/card.rs) — elevated panel with shadow and optional header/footer slots.
- [GroupBox](../crates/bastyde-widgets/src/group_box.rs) — labelled bordered group for related controls.
- [GroupHeader](../crates/bastyde-widgets/src/group_header.rs) — section header (label + trailing rule line) for settings forms.
- [Toolbar](../crates/bastyde-widgets/src/toolbar.rs) — dense horizontal action strip on a `surface_secondary` panel.
- [StatusBar](../crates/bastyde-widgets/src/status_bar.rs) — bottom-of-window status text strip with `Role::Status`.
- [Banner](../crates/bastyde-widgets/src/banner.rs) — persistent inline info / success / warning / error strip (`BannerSeverity`); `Role::Status` + `Live::Polite`.
- [Accordion](../crates/bastyde-widgets/src/accordion.rs) — vertically stacked collapsible sections, multiple-open allowed.
- [ToolBox](../crates/bastyde-widgets/src/tool_box.rs) — vertically stacked collapsible pages, exactly one expanded (Qt `QToolBox` analog).
- [ScrollArea](../crates/bastyde-widgets/src/scroll_area.rs) — viewport with overlay or permanent scrollbars (`ScrollBarMode`, `ScrollBarPolicy`).
- [ScrollBar](../crates/bastyde-widgets/src/scroll_bar.rs) — standalone scrollbar, drag/track-click/keyboard.
- [SplitView](../crates/bastyde-widgets/src/split_view.rs) — two-pane resizable splitter with drag handle.
- [TabWidget](../crates/bastyde-widgets/src/tab_widget.rs) — tab bar + content switcher; data-source-driven `TabBar<T>` underneath. See [tab-widget.md](tab-widget.md).
- [Wizard](../crates/bastyde-widgets/src/wizard.rs) — multi-step flow with header, footer, and step switching (`WizardStep`).
- [Breadcrumb](../crates/bastyde-widgets/src/breadcrumb.rs) — clickable path segments with chevron separators (`BreadcrumbItem`).
- [TitleBar](../crates/bastyde-widgets/src/title_bar.rs) — custom window title bar with drag region, resize strip, and window controls. See [title-bar.md](title-bar.md).

---

## Buttons

- [Button](../crates/bastyde-widgets/src/button.rs) — seven `ButtonVariant`s (Filled / Tinted / Outlined / Plain / Ghost / Link / Destructive) × five interaction states; `IconLocation` for leading/trailing icon; chrome via the `ButtonStyle` trait (see Styling status above). Reference exemplar — read the source.
- [IconButton](../crates/bastyde-widgets/src/icon_button.rs) — square icon-only button at five `IconButtonSize` steps (Compact / Default / Toolbar / Large / Hero). `.embedded()` mode for trailing-slot use inside fields. Includes `BuiltInIcons` factory.
- [CommandLinkButton](../crates/bastyde-widgets/src/command_link_button.rs) — large two-line CTA: leading icon + bold title + secondary description; flat surface.
- [PopoverButton](../crates/bastyde-widgets/src/popover_button.rs) — Button preset that opens a Popover when activated.
- [PopoverIconButton](../crates/bastyde-widgets/src/popover_icon_button.rs) — IconButton variant of the same.
- [SplitButton](../crates/bastyde-widgets/src/split_button.rs) — main action region + chevron region that opens a related-actions menu.

## Inputs and indicators

- [Checkbox](../crates/bastyde-widgets/src/checkbox.rs) — two-state and tristate (`CheckState`).
- [RadioButton](../crates/bastyde-widgets/src/radio_button.rs) — single radio, bound to a shared value via [RadioGroup](../crates/bastyde-widgets/src/radio_group.rs) for mutual exclusion.
- [Toggle](../crates/bastyde-widgets/src/toggle.rs) — on/off control; four `ToggleVariant`s (Switch / Pill / Square / Inset) via the `ToggleStyle` trait.
- [Slider](../crates/bastyde-widgets/src/slider.rs) — horizontal or vertical, optional stepping.
- [SegmentedControl](../crates/bastyde-widgets/src/segmented_control.rs) — `Signal<usize>`-driven segmented chooser; `RadioGroup` AT role.
- [ComboBox](../crates/bastyde-widgets/src/combo_box.rs) — selection-only dropdown; virtualized via `ListView` past `max_visible_items`.
- [ProgressBar](../crates/bastyde-widgets/src/progress_bar.rs) — determinate or indeterminate; linear.
- [Spinner](../crates/bastyde-widgets/src/spinner.rs) — circular-arc loading indicator on the shader-driven `AnimatedQuadKind::SpinnerArc` pipeline; honours `prefers-reduced-motion`.
- [Link](../crates/bastyde-widgets/src/link.rs) — typographic hyperlink with hover and visited states.
- [Badge](../crates/bastyde-widgets/src/badge.rs) — passive count/label pill.
- [Avatar](../crates/bastyde-widgets/src/avatar.rs) — user identity (image / initials fallback / hash-derived tint); circular / rounded-square / square shapes; presence indicator with corner positioning.

---

## Text input family — **(rich-text)**

All gated behind the `rich-text` feature; on by default in `bastyde`.

- [TextInput](../crates/bastyde-widgets/src/text_input.rs) — styled single-line input on top of `TextInputField`; `ValidationState`.
- [RichTextEditor](../crates/bastyde-widgets/src/rich_text.rs) — full editing surface with IME, formatting commands, undo/redo, intrinsic-mode sizing (`min_lines` / `max_lines`); also runs read-only as the rich-text viewer (`ScrollPolicy`).
- [SpinBox](../crates/bastyde-widgets/src/spin_box.rs) — numeric input with `WrapMode`, `StepType`, `ButtonLayout`, `WheelMode`, `WidthPolicy`.
- [SearchField](../crates/bastyde-widgets/src/search_field.rs) — TextInput preset with leading magnifier glyph and clear-X; `Role::SearchInput`.
- [FilePickerField](../crates/bastyde-widgets/src/file_picker_field.rs) — TextInput + Browse button wired to the native file dialog; `FilePickerKind::OpenFile / PickFolder / SaveFile`.
- [InputDialog](../crates/bastyde-widgets/src/input_dialog.rs) — single-field input modal: title + prompt + TextInput + Cancel/OK; `on_result` delivers `Some(value)` / `None`.

### Date and time

- [Calendar](../crates/bastyde-widgets/src/calendar.rs) — month grid with WAI-ARIA grid keyboard pattern; `CalendarMode::Single` / `Range` (`DateRange`); `WeekNumberDisplay` toggle. Locale-derived first day of week and format pattern.
- [DateEdit](../crates/bastyde-widgets/src/date_edit.rs) — date input with trailing calendar-icon trigger; `WidthPolicy`, `ValidationBehavior`.
- [TimeEdit](../crates/bastyde-widgets/src/time_edit.rs) — time input; `TimeFormat`, `SecondsMode`.
- [DateTimeEdit](../crates/bastyde-widgets/src/date_time_edit.rs) — combined date + time input.
- [DateRangeEdit](../crates/bastyde-widgets/src/date_range_edit.rs) — two-date range input.

### Color

- [HexColorInput](../crates/bastyde-widgets/src/hex_color_input.rs) — hex code text input with live swatch.
- [ColorEdit](../crates/bastyde-widgets/src/color_edit.rs) — compact color editor with swatch trigger.
- [ColorPicker](../crates/bastyde-widgets/src/color_picker.rs) — full HSV picker; `ColorPickerLayout` controls panel arrangement.

---

## Menus

- [MenuBar](../crates/bastyde-widgets/src/menu_bar.rs) — top-of-window menu strip; widget-based on Windows/Linux. (macOS native `NSMenu` integration tracked under §30 of the architecture doc.)
- [MenuList](../crates/bastyde-widgets/src/menu_list.rs) — overlay menu panel; accepts arbitrary `impl Widget` children. `MenuSeparator` for inline rules.
- [MenuItem](../crates/bastyde-widgets/src/menu_item.rs) — keyboard-highlightable menu row with `for_shortcut(id)` for live-rebinding labels.

## Overlays and dialogs

See [tooltips.md](tooltips.md) for the tooltip system.

- [TooltipWidget](../crates/bastyde-widgets/src/tooltip.rs) — plain, rich, or composite tooltips (three tiers, per-anchor mutual exclusion); sticky-on-dwell promotion to non-modal `Role::Dialog`; `TooltipRegistry` for app-wide reuse. Rich tier carries inline markup + shortcut chip + "more" disclosure; composite tier ([`CompositeTooltipWidget`](../crates/bastyde-widgets/src/tooltip/composite.rs)) hosts an arbitrary widget tree (CK3-style: tabbed sections, charts, progress bars).
- [Popover](../crates/bastyde-widgets/src/popover.rs) — anchored overlay accepting arbitrary `impl Widget` content; configurable placement, dismissal, optional caret.
- [Dialog](../crates/bastyde-widgets/src/dialog.rs) — modal dialog frame; `DialogContent` / `ModalContainer` for content + presentation.
- [MessageBox](../crates/bastyde-widgets/src/message_box.rs) — predefined info/warning/error/question modals (`MessageBoxSeverity`); semantic-role buttons (`ButtonRole`, `StandardButton`, `MessageBoxButton`, `MessageBoxButtons`) with platform-aware ordering; result via `MessageBoxResult`.
- [Snackbar](../crates/bastyde-widgets/src/snackbar.rs) — queued auto-dismissing toast with animated slide-in.
- [Toast](../crates/bastyde-widgets/src/toast.rs) — stackable, action-rich, severity-aware floating notification (`info` / `success` / `warning` / `error` / `loading`); link + button actions; `Toast::id` update-in-place; persistent archive backing; corner-anchored hover-pause stack. The "upgrade path" from `Snackbar`. Full reference: [toast.md](toast.md).
- [ToastHost](../crates/bastyde-widgets/src/toast/host.rs) — per-window invisible widget owning the toast queue + per-frame timer + hover-pause; mounted by `install_toast`.
- [NotificationLog](../crates/bastyde-widgets/src/notification/log.rs) — archive UI: mark-all-read / clear toolbar + day-bucket section headers (Today / Yesterday / This week / Earlier) + replayable action buttons.
- [NotificationCenterButton](../crates/bastyde-widgets/src/notification/center_button.rs) — bell icon + live unread-count badge + popover containing a `NotificationLog`. Marks-all-read on popover open.
- [NotificationLogDialog](../crates/bastyde-widgets/src/notification/log_dialog.rs) — one-liner `::show(archive, ctx)` modal preset.
- [Shadow](../crates/bastyde-widgets/src/shadow.rs) — drop-shadow primitive used by elevated surfaces (`AttachedSide` for one-sided shadows).

---

## Data-driven widgets

Backed by the `bastyde-data` reactive collections. See [data-models.md](data-models.md) for the underlying `ListModel<T>` / `TreeModel<T>` / `SelectionModel` / sort-filter projections.

- [Repeater](../crates/bastyde-widgets/src/repeater.rs) — non-virtualized siblings driven by `ListModel<T>` change notifications; for small bounded collections.
- [ListView](../crates/bastyde-widgets/src/list_view.rs) — virtualized vertical list for large/unbounded collections.
- [TreeView](../crates/bastyde-widgets/src/tree_view.rs) — hierarchical list with twist-arrow expand/collapse. The 4-arg [`new_with_context`](../crates/bastyde-widgets/src/tree_view.rs) variant passes a `TreeRowContext` carrying a one-line `toggle_callback()` for chevron wiring.
- [StandardListItem](../crates/bastyde-widgets/src/standard_item.rs) — canonical row layout for `ListView` delegates: `[checkbox?] [leading_slot?] [center_slot?] [label] [Spacer] [trailing_slot?]`, plus an optional subtitle line with its own `[subtitle_leading_slot?] [subtitle] [Spacer] [subtitle_trailing_slot?]`. Selection / hover / pressed background routes through `SurfaceRole::Selected` / `AccentSubtle` / `Pressed` (theme-driven, rounded `item_corner_radius: 8.0`, mirrors `MenuItem` / `ComboBox`). Optional two-state (`Signal<bool>`) or tri-state (`Signal<CheckState>`) checkbox at the start of the row, independent of row selection. See the worked example in [examples/data_collections/src/main.rs](../examples/data_collections/src/main.rs).
- [StandardTreeItem](../crates/bastyde-widgets/src/standard_item.rs) — `StandardListItem` plus depth-driven indent and a chevron column (always reserved, even for leaves, so labels at the same depth align). `.from_entry(&FlatEntry)` sets depth + has_children + is_expanded in one call; `.on_toggle(...)` / `.on_toggle_rc(...)` wires the chevron tap to a `TreeSliceHandle::toggle_expand` callback (cleanest with `TreeView::new_with_context`).
- [TableView](../crates/bastyde-widgets/src/table_view.rs) — multi-column, virtualized; sort/filter via `SortFilterListModel`, drag-resize and drag-reorder columns, pinned Leading/Trailing, cell + row selection, edit hooks, row drag-drop reorder, full `Role::Table` AT tree. See [table-view.md](table-view.md).
- [TreeTable](../crates/bastyde-widgets/src/tree_table.rs) — hierarchical multi-column variant of TableView; `Role::TreeGrid`.

Worked TreeView delegate using both new pieces:

```rust
let tree_checks: TreeCheckedModel<Item> = state.app_state();
TreeView::new_with_context(model, move |item, entry, selected, ctx| {
    let mut row = StandardTreeItem::new_literal(&item.title)
        .from_entry(entry)
        .selected(selected)
        .on_toggle_rc(ctx.toggle_callback());
    if entry.has_children {
        row = row.tristate_checkbox(tree_checks.signal_for(entry.node_id));
    }
    Box::new(row)
})
```

## Charts — `crates/bastyde-charts/src/`

Sits at the same tier as `bastyde-widgets` (no dep on widgets). See [charts.md](charts.md).

- [BarChart](../crates/bastyde-charts/src/bar_chart.rs) — vertical or horizontal bars; single or grouped series; optional value labels, axis labels, grid lines.
- [LineChart](../crates/bastyde-charts/src/line_chart.rs) — points connected by polylines; single or multiple series; optional area fill; hover tooltips on data points.
- [PieChart](../crates/bastyde-charts/src/pie_chart.rs) — pie + donut variants; donut variant has a center slot.

Shared infrastructure ([series.rs](../crates/bastyde-charts/src/series.rs), [axis.rs](../crates/bastyde-charts/src/axis.rs), [legend.rs](../crates/bastyde-charts/src/legend.rs), [palette.rs](../crates/bastyde-charts/src/palette.rs), [layout.rs](../crates/bastyde-charts/src/layout.rs)) is reused across all three.

---

## Animation wrappers — `crates/bastyde-widgets/src/animations/`

Wrappers that animate a child subtree without the caller managing scheduler state. See [animation.md](animation.md) for `Signal<f32>::animate_to` and the underlying scheduler.

- [Fade](../crates/bastyde-widgets/src/animations/fade.rs) — opacity tween 0↔1; layout-transparent.
- [Pulse](../crates/bastyde-widgets/src/animations/pulse.rs) — sine-driven looping opacity oscillation (recording-indicator pattern).
- [Cycle](../crates/bastyde-widgets/src/animations/cycle.rs) — cycles through children on a fixed period.
- [Crossfade](../crates/bastyde-widgets/src/animations/crossfade.rs) — keyed builder; old fades to new on key change.
- [Collapse](../crates/bastyde-widgets/src/animations/collapse.rs) — height-collapse tween used by Accordion and disclosure patterns.
- [SmoothSize](../crates/bastyde-widgets/src/animations/smooth_size.rs) — auto-sizes to the child's intrinsic size and animates every change (`SmoothSizeAxes`).
- [Slide](../crates/bastyde-widgets/src/animations/slide.rs) — slides a child in/out from a chosen edge (`SlideEdge`); layout-stable.
- [Shake](../crates/bastyde-widgets/src/animations/shake.rs) — damped horizontal oscillation triggered by a `Signal<u32>` bump (invalid-input feedback).
- [Scale](../crates/bastyde-widgets/src/animations/scale.rs) — uniform 2D scale 0↔1 (`ScaleOrigin`); visual-only by default, optional layout-driving mode.
- [Rotate](../crates/bastyde-widgets/src/animations/rotate.rs) — rotates a child subtree by a `Prop<f32>` angle in radians.
- [Blur](../crates/bastyde-widgets/src/animations/blur.rs) — Gaussian-equivalent blur on the child subtree via dual-Kawase chain; sub-perceptual radii are zero-cost.

---

## Settings widgets

Pre-built UI for common app-level concerns.

- [ShortcutSettings](../crates/bastyde-widgets/src/shortcut_settings.rs) — full keyboard-shortcut rebind UI (Rebind / Reset / conflict auto-unbind / key capture). See [shortcut-intent-action.md](shortcut-intent-action.md).
- [PrivacySettings](../crates/bastyde-widgets/src/privacy_settings.rs) **(`telemetry` feature)** — consent toggles for telemetry adapters; ties into the [telemetry.md](telemetry.md) consent gate.

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
- Framework internals (Canvas, rendering pipeline, threading, testability): [architecture.md](architecture.md)
- Full per-widget API extraction: `python3 tools/extract_widget_api.py <Widget…>` or `--all`
