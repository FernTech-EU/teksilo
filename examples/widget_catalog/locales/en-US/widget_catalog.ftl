# Bastyde Widget Catalog — English (source) translations.
#
# Keys use kebab-case at the Fluent layer; Rust call sites use the
# snake_case form, and the proc macro converts. Tab order is stable —
# index N corresponds to the Nth `static_tab(...)` call in main.rs.

# ── App chrome ──────────────────────────────────────────────────────────
app-title = Bastyde — Widget Catalog
app-subtitle = drag · double-click maximize · right-click for menu
app-unsupported-chrome = (custom chrome unsupported on this platform — falling back to native decorations)

# ── View mode toggle (TabWidget trailing slot) ──────────────────────────
mode-label = bati! DSL
mode-tooltip = Switch every tab between the classic builder and the bati! macro version of the same tree.

# ── Locale switcher ─────────────────────────────────────────────────────
locale-en = English
locale-fr = Français
locale-ar = العربية

# ── Theme switcher ──────────────────────────────────────────────────────
theme-label = Theme
theme-tooltip = Toggle between light and dark theme.

# ── Tab titles ──────────────────────────────────────────────────────────
tab-palette-title = Palette
tab-layout-title = Layout
tab-visuals-title = Visuals
tab-containers-title = Containers
tab-chrome-title = Chrome
tab-buttons-title = Buttons
tab-styling-title = Styling
tab-inputs-title = Inputs
tab-indicators-title = Indicators
tab-text-title = Text
tab-datetime-title = Date & Time
tab-color-title = Color
tab-menus-title = Menus
tab-overlays-title = Overlays
tab-data-title = Data
tab-animations-title = Animations
tab-settings-title = Settings

# ── Deep-dive references ────────────────────────────────────────────────
tab-palette-refs = All surface, text, and editor roles, with a rich-text + emoji pangram so theme switching reads visually. See: docs/reactive-theme.md.
tab-layout-refs = Stacks, grids, and slack distribution. See: cargo run -p text_and_layout, cargo run -p split_view.
tab-visuals-refs = Rendering primitives, icons, images, validation strips. See: cargo run -p simple_button.
tab-containers-refs = Panels, cards, accordions, scroll areas, split views. See: cargo run -p split_view, cargo run -p tool_box.
tab-chrome-refs = App chrome: toolbar, status bar, breadcrumbs, wizards, banners. See: cargo run -p title_bar_demo.
tab-buttons-refs = Every button variant. See: cargo run -p simple_button, cargo run -p menus_and_dropdowns.
tab-styling-refs = The four-tier styling ladder: variant grids (Tier 1) and per-call .style(impl FooStyle) overrides (Tier 3). See: docs/styling-system.md, cargo run -p theme_styles.
tab-inputs-refs = Boolean and selection inputs. Deep keyboard nav lives in the dedicated examples.
tab-indicators-refs = Read-only status: progress, spinners, badges, avatars, links.
tab-text-refs = Single-line and rich text editing. See: cargo run -p rich_text_editor, cargo run -p spin_box.
tab-datetime-refs = Calendar, date, time, and date-range pickers. See: cargo run -p datetime_pickers.
tab-color-refs = Hex input, compact color edit, full HSV picker. See: cargo run -p color_picker.
tab-menus-refs = Menu bar, menu list, context menus. See: cargo run -p menus_and_dropdowns.
tab-overlays-refs = Tooltips, popovers, dialogs, snackbars. See: cargo run -p tooltips_showcase, cargo run -p dialogs_and_popovers, cargo run -p file_dialogs.
tab-data-refs = ListView, TreeView, TableView, TreeTable. See: cargo run -p data_grid, cargo run -p tree_table, cargo run -p data_collections.
tab-animations-refs = Fade, pulse, slide, blur, and friends. See: cargo run -p animations, cargo run -p animations_kit.
tab-settings-refs = Shortcut rebinding and privacy settings widgets. See: cargo run -p shortcuts_demo.

# ── Tab body placeholder (used by stubs) ────────────────────────────────
stub-heading = Coming soon
stub-body = This tab will be filled in during Phase 3/4 of the catalog rewrite.

# ── Common reusable demo labels ────────────────────────────────────────
demo-save = Save
demo-cancel = Cancel
demo-open = Open
demo-new = New
demo-edit = Edit
demo-quit = Quit
demo-undo = Undo
demo-redo = Redo
demo-cut = Cut
demo-copy = Copy
demo-paste = Paste
demo-find = Find…
demo-confirm = Confirm
demo-learn-more = Learn more
demo-next = Next
demo-back = Back
demo-finish = Finish
demo-loading = Loading…

# ── Indicators tab ──────────────────────────────────────────────────────
ind-progress-determinate-label = 60 %
ind-link-docs = Open the Bastyde docs
ind-link-handler = With a click handler

# ── Inputs tab ──────────────────────────────────────────────────────────
inp-checkbox-two-state = Two-state checkbox
inp-checkbox-tristate = Tristate checkbox
inp-checkbox-disabled = Disabled (cannot toggle)
inp-radio-a = Option A
inp-radio-b = Option B
inp-radio-c = Option C
inp-toggle-feature = Enable feature
inp-toggle-with-label = With label
inp-toggle-disabled = Disabled toggle
inp-slider-volume = Volume
inp-slider-stepped = Quality (steps of 25)
inp-slider-vertical = Vertical slider
inp-segment-first = First
inp-segment-second = Second
inp-segment-third = Third
inp-combo-apple = Apple
inp-combo-banana = Banana
inp-combo-cherry = Cherry
inp-combo-placeholder = Pick a fruit

# ── Buttons tab ─────────────────────────────────────────────────────────
btn-default = Default
btn-regular = Regular
btn-flat = Flat
btn-confirm-label = Confirm
btn-cmdlink-signin-title = Sign in to your Bastyde account
btn-cmdlink-signin-desc = Use your existing credentials to access projects.
btn-cmdlink-signup-title = Create a new account
btn-cmdlink-signup-desc = Free for personal and open-source use.
btn-popover-trigger = Open popover
btn-popover-title = Popover content
btn-popover-body = Click outside to dismiss.
btn-popover-icon-body = Quick-add menu

# ── Containers tab ──────────────────────────────────────────────────────
cnt-panel-body = Panel surface with role-driven background + border
cnt-card-header = Card header
cnt-card-body = A Card has elevation (shadow), an optional header, content, and footer.
cnt-card-footer = footer · auto-tinted
cnt-groupbox-title = Notifications
cnt-cb-sounds = Play sounds
cnt-cb-banner = Show banner
cnt-groupheader-title = Section title
cnt-groupheader-body = …content under the header
cnt-accordion-1-title = Show details
cnt-accordion-1-body = Body of the first accordion section.
cnt-accordion-2-title = Advanced
cnt-accordion-2-body = Body of the second accordion section.
cnt-toolbox-general = General
cnt-toolbox-general-body = General preferences
cnt-toolbox-editor = Editor
cnt-toolbox-editor-body = Editor settings
cnt-toolbox-privacy = Privacy
cnt-toolbox-privacy-body = Privacy + telemetry settings
cnt-split-leading = Leading pane
cnt-split-trailing = Trailing pane

# ── Chrome tab ──────────────────────────────────────────────────────────
chr-status = Ready · 1247 lines · UTF-8 · Rust
chr-banner-info-title = Info
chr-banner-info-body = Did you know Bastyde is RTL-aware?
chr-banner-success-title = Success
chr-banner-success-body = Settings saved.
chr-banner-warning-title = Warning
chr-banner-warning-body = Disk is 90 % full.
chr-banner-error-title = Error
chr-banner-error-body = Network connection lost.
chr-breadcrumb-home = Home
chr-breadcrumb-docs = Documents
chr-breadcrumb-bastyde = Bastyde
chr-breadcrumb-current = widget-catalog
chr-wizard-title = Onboarding
chr-wizard-step1 = Welcome
chr-wizard-step1-body = Step 1 — welcome to Bastyde
chr-wizard-step2 = Configure
chr-wizard-step2-body = Step 2 — configure your editor
chr-wizard-step3 = Finish
chr-wizard-step3-body = Step 3 — you're ready to go
chr-wizard-trigger = Open wizard

# ── Visuals tab ─────────────────────────────────────────────────────────
vis-text-body = Body text
vis-text-bold = BodyBold
vis-text-small = Small Secondary
vis-text-tiny = Tiny Disabled
vis-image-alt-1 = Star icon (raster)
vis-image-alt-2 = Star icon, circle-masked
vis-image-alt-3 = Star icon, rounded-square-masked
vis-panel-body = Panel: background + border + radius + padding

# ── Layout tab (per-widget short descriptions) ─────────────────────────
lay-overlay = overlay
lay-padding-body = inset 16 px on all sides
lay-fixed-size = 140 × 40
lay-min-size = min 160 × 32
lay-max-size = clamped to ≤ 240 × 32, even with very long text inside
lay-aspect-label = 16:9
lay-centered = centered
lay-form-label-a = Label A
lay-form-value-a = value A
lay-form-label-b = Label B
lay-form-value-b = value B
lay-switcher-next = next page

# ── Text tab ────────────────────────────────────────────────────────────
txt-username-label = Username
txt-username-placeholder = e.g. ferris
txt-readonly-label = Read-only field
txt-search-placeholder = Type a fruit — Apple, Banana, …
txt-file-label = Choose a file
txt-file-placeholder = No file selected
txt-input-dialog-trigger = Open InputDialog
txt-input-dialog-title = Rename file
txt-input-dialog-prompt = Enter the new file name:
txt-input-dialog-placeholder = untitled.txt

# ── Color tab ───────────────────────────────────────────────────────────
clr-brand-label = Brand color
clr-accent-label = Theme accent

# ── Palette tab ─────────────────────────────────────────────────────────
pal-surfaces = Surfaces
pal-text = Text
pal-editor = Editor

# ── Menus tab ───────────────────────────────────────────────────────────
mnu-file = File
mnu-menu-edit = Edit
mnu-standalone-a = Standalone item A
mnu-with-shortcut = With shortcut
mnu-disabled = Disabled item

# ── Overlays tab ────────────────────────────────────────────────────────
ovr-tooltip-hover = Hover me
ovr-tooltip-hover-body = Plain tooltip text
ovr-tooltip-longer = With longer text
ovr-tooltip-longer-body = Tooltips can wrap onto multiple lines if needed.
ovr-popover-anchor = Anchor
ovr-popover-title = Popover content
ovr-popover-body = Click outside to dismiss.
ovr-dialog-trigger = Open Dialog
ovr-dialog-title = Dialog example
ovr-dialog-body = This is a Dialog (presented via MessageBox::information).
ovr-mb-info = Information
ovr-mb-info-body = Informational dialog.
ovr-mb-warning = Warning
ovr-mb-warning-body = Disk is almost full.
ovr-mb-error = Error
ovr-mb-error-body = Something went wrong.
ovr-mb-confirm = Are you sure?
ovr-mb-confirm-body = This action cannot be undone.
ovr-snackbar-trigger = Show snackbar
ovr-snackbar-body = File saved successfully
ovr-shadow-body = Card-like surface with the default Panel shadow

# ── Data tab ────────────────────────────────────────────────────────────
dat-fruit-apple = Apple
dat-fruit-banana = Banana
dat-fruit-cherry = Cherry
dat-fruit-date = Date
dat-list-row = Row
dat-list-item-1 = First item
dat-list-item-2 = Second item
dat-list-item-3 = Third item
dat-tree-root = Root
dat-tree-child-a = Child A
dat-tree-child-b = Child B
dat-tree-grandchild = Grandchild
dat-tree-note = TreeView requires a TreeModel<T>. See `cargo run -p tree-table` for the full demo.
dat-table-note = TableView requires column definitions and a ListModel. See `cargo run -p data-grid` for a 1k×6 grid showcase.
dat-treetable-note = TreeTable combines TableView columns with TreeView hierarchy. See `cargo run -p tree-table` for the mock-filesystem demo.

# ── Animations tab ──────────────────────────────────────────────────────
anim-visible = Visible
anim-expanded = Expanded
anim-tip-1 = Tip 1 — drag the divider
anim-tip-2 = Tip 2 — try Ctrl+P
anim-tip-3 = Tip 3 — F12 opens the inspector
anim-crossfade-next = Next variant
anim-collapse-body = Collapsing content
anim-smooth-body = Animates to its child's intrinsic size on every change.
anim-shake = Shake
anim-rotate = Rotate +45°
anim-blur-toggle = Toggle blur
anim-blur-body = Sensitive content — toggle blur to reveal

# ── Settings tab ────────────────────────────────────────────────────────
set-privacy-note = Gated behind the `telemetry` cargo feature. Run `cargo run -p telemetry_plausible` or similar to see the consent UI.

# ── Auto-generated by /tmp/find_literals.py ───────────────
animations-tip-1-drag-the-divider = Tip 1 — drag the divider
animations-tip-2-try-ctrl-p = Tip 2 — try Ctrl+P
animations-tip-3-f12-opens-the-inspector = Tip 3 — F12 opens the inspector
animations-next-variant = Next variant
animations-collapsing-content = Collapsing content
animations-animates-to-its-child-s-intrin = Animates to its child's intrinsic size on every change.
animations-rotate-45 = Rotate +45°
animations-toggle-blur = Toggle blur
animations-sensitive-content-toggle-blur = Sensitive content — toggle blur to reveal
buttons-save-as = Save As…
data-first-item = First item
data-second-item = Second item
data-third-item = Third item
data-child-a = Child A
data-child-b = Child B
data-treeview-requires-a-treemodel = TreeView requires a TreeModel<T>. See `cargo run -p tree-table` for the full demo.
layout-cross-platform = Cross-platform
layout-clamped-to-240-32-even-with-ve = clamped to ≤ 240 × 32, even with very long text inside
layout-cross-platform-2 = Cross-platform
overlays-hover-me = Hover me
overlays-plain-tooltip-text = Plain tooltip text
overlays-with-longer-text = With longer text
overlays-tooltips-can-wrap-onto-multipl = Tooltips can wrap onto multiple lines if needed.
overlays-popover-content = Popover content
overlays-click-outside-to-dismiss = Click outside to dismiss.
overlays-open-dialog = Open Dialog
overlays-dialog-example = Dialog example
overlays-this-is-a-dialog-presented-via = This is a Dialog (presented via MessageBox::information).
overlays-informational-dialog = Informational dialog.
overlays-disk-is-almost-full = Disk is almost full.
overlays-something-went-wrong = Something went wrong.
overlays-are-you-sure = Are you sure?
overlays-this-action-cannot-be-undone = This action cannot be undone.
overlays-file-saved-successfully = File saved successfully
overlays-file-saved-successfully-2 = File saved successfully
overlays-show-snackbar = Show snackbar
overlays-card-like-surface-with-the-def = Card-like surface with the default Panel shadow
