<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Banner

Banner — persistent inline status strip (info / success / warning / error).

A non-transient, full-width callout for app-level conditions: deprecation
notices, "you have unsaved changes", trial-expiry warnings, license
issues, restored-from-cache notices, etc. Distinct from
`Snackbar` (transient, corner-anchored) and
`MessageBox` (modal).

```ignore
Banner::warning(tr!(unsaved_changes()))
    .description(tr!(close_loses_changes()))
    .action(Button::new(tr!(save_now()))
        .on_activate_fn(|ctx| ctx.send_intent(AppIntent::SaveNow)))
    .on_dismiss(|ctx| ctx.send_intent(AppIntent::DismissBanner))
```

## Builder methods at a glance

`style`, `info`, `success`, `warning`, `error`, `description`, `action`, `on_dismiss`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/banner/index.html)

## `pub struct Banner`

A persistent inline status strip.

```rust
pub struct Banner { /* fields */ }
```

### Methods

#### `pub fn style(mut self, style: impl bastyde_core::styles::BannerStyle) -> Self`

Per-call style override for the banner strip chrome. Replaces
the theme-wide default `BannerStyle` for just this instance.

#### `pub fn info(title: impl Into<LocalizedString>) -> Self`

Construct an info-severity banner.

#### `pub fn success(title: impl Into<LocalizedString>) -> Self`

Construct a success-severity banner.

#### `pub fn warning(title: impl Into<LocalizedString>) -> Self`

Construct a warning-severity banner.

#### `pub fn error(title: impl Into<LocalizedString>) -> Self`

Construct an error-severity banner.

#### `pub fn description(mut self, text: impl Into<LocalizedString>) -> Self`

Optional secondary line of text rendered below the title.

#### `pub fn action(mut self, widget: impl Widget + 'static) -> Self`

Trailing widget — typically a `Button` or
an `HStack` of buttons. Placed before the optional dismiss button.

#### `pub fn on_dismiss(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Attach a trailing dismiss (X) button. The closure runs when the
user clicks it; the host is expected to remove the banner from the
tree (typically by toggling a `Signal<bool>` driving a `Switcher`).
