<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Avatar

![Avatar preview](img/avatar.png)

`Avatar` — circular (or rounded-square / square) user-identity widget.

Displays either a person's image (clipped to the configured shape via
a CPU-side anti-aliased alpha mask applied at construction time) or
their initials over a hash-derived background colour. Optional
presence indicator (Online / Offline / Away / Busy) and outer ring.
Can be made activable to serve as a user-menu trigger.

```rust
# use teksilo_widgets::{Avatar, AvatarPresence, AvatarSize};
# use teksilo_canvas::raster::RasterIcon;
# use teksilo_i18n::lit;
# use teksilo_core::Intent;
# let face = RasterIcon::from_raw(vec![0u8; 4 * 4 * 4], 4, 4);
// Image with a presence dot.
let _w = Avatar::with_image(&face)
    .alt(lit!("Jane Doe"))
    .presence(AvatarPresence::Online)
    .size(AvatarSize::Medium);

// Hash-tinted initials, auto-derived from a name.
let _w = Avatar::with_name(lit!("Jane Doe")).size(AvatarSize::Large);

// Click target — opens a user menu via an intent.
let _w = Avatar::with_image(&face)
    .label(lit!("Open user menu"))
    .alt(lit!("Jane Doe"))
    .on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.open-user-menu")));
```

The widget reuses `ImageWidget` for the image path and draws bg /
border / presence directly via `Canvas`. Hash-derived background
tints come from `theme.colors.chart_palette` (Okabe-Ito), so they
track the active theme automatically.

## Builder methods at a glance

`with_initials`, `with_name`, `with_image`, `from_raw_image`, `style`, `size`, `shape`, `fallback_initials`, `image_visible`, `background`, `foreground`, `seed`, `border`, `border_color`, `presence`, `presence_corner`, `label`, `alt`, `a11y_hidden`, `on_activate_fn`, `has_popup`, `expanded_when`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`, `name_signal`, `image_signal`, `alt_signal`, `label_signal`, `presence_signal`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/avatar/index.html)

## `pub struct Avatar`

Circular (or rounded-square / square) user-identity widget showing
either a photo or hash-tinted initials, with an optional presence dot.

Static and reactive content fields coexist: each knob (`name`, `image`,
`alt`, `label`, `presence`) has a static constructor or setter *and* a
`bind_*` counterpart that takes a `Signal`. When a signal is bound it
wins; the static value acts as a fallback. Signal-bound rebuilds fire at
`BindingLevel::Rebuild` so inner children are recreated with fresh values —
the canonical pattern for a "logged-out → logged-in" transition.

```rust
pub struct Avatar { /* fields */ }
```

### Methods

#### `pub fn with_initials(initials: impl Into<LocalizedString>) -> Self`

Create an avatar from an explicit initials string. Uppercases and
truncates to at most 2 chars; empty input yields `"?"`.

#### `pub fn with_name(name: impl Into<LocalizedString>) -> Self`

Create an avatar from a display name; initials are derived
automatically (`"Jane Doe" → "JD"`, `"jane.doe@x.com" → "JD"`,
`"Cher" → "C"`, `"" → "?"`), and the full name is used as the
hash seed for the background tint so users with identical initials
still get distinct colours.

#### `pub fn with_image(icon: &RasterIcon) -> Self`

Create an avatar from a decoded `RasterIcon`. The pixels are
centre-cropped to a square and CPU-masked to the configured shape
at the first `build()`. Call `.alt(...)` to provide a
screen-reader name for the image.

#### `pub fn from_raw_image(pixels: Vec<u8>, width: u32, height: u32) -> Self`

Create an avatar from raw RGBA pixels (`width × height × 4` bytes).
Same pixel-layout convention as `ImageWidget::from_raw`.

#### `pub fn style(mut self, style: impl teksilo_core::styles::AvatarStyle) -> Self`

Per-call style override for the avatar chrome.

#### `pub fn size(mut self, size: AvatarSize) -> Self`

Set the avatar's discrete size. Default: `AvatarSize::Medium` (32 dp).

#### `pub fn shape(mut self, shape: AvatarShape) -> Self`

Set the avatar's clip shape. Default: `AvatarShape::Circle`.

#### `pub fn fallback_initials(mut self, initials: impl Into<LocalizedString>) -> Self`

Override the initials shown when the image is hidden via
`image_visible(false)` or fails to register. Defaults to the
derived initials if `with_image` was paired with `with_name`,
otherwise `"?"`.

#### `pub fn image_visible(mut self, visible: impl Into<Prop<bool>>) -> Self`

Reactive image visibility. When unbound it's `true`. When bound
to a `Signal<bool>` and the value is `false`, the initials
fallback paints in place of the image — same logical bounds, no
layout shift.

#### `pub fn background(mut self, color: impl Into<ColorProp>) -> Self`

Override the auto hash-derived background. Accepts a `Color`,
a role, or a `Signal<Color>`.

#### `pub fn foreground(mut self, color: impl Into<ColorProp>) -> Self`

Override the auto-contrast text colour for the initials. Auto
(unset) picks white over dark backgrounds and near-black over
light ones, computed at paint time from the resolved bg's
luminance.

#### `pub fn seed(mut self, seed: impl Into<String>) -> Self`

Override the seed string used to pick a hash-derived background
from the theme's chart palette. Defaults to the resolved name
(when constructed via `with_name`) or the initials.

#### `pub fn border(mut self, width: f32) -> Self`

Outer ring thickness. A non-zero value enables the ring (drawn
in `BorderRole::Default` unless `Self::border_color` overrides
it). `0.0` disables the ring.

#### `pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self`

Override the outer ring colour. Accepts a `Color`, a theme role,
or a `Signal<Color>`. Has no effect unless `Self::border` is also
set to a positive width.

#### `pub fn presence(mut self, presence: AvatarPresence) -> Self`

Show a presence indicator dot. Pass `AvatarPresence::Online`,
`Offline`, `Away`, or `Busy`.

#### `pub fn presence_corner(mut self, corner: AvatarCorner) -> Self`

Choose which corner the presence dot occupies. Default:
`AvatarCorner::BottomTrailing`.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the accessible name. When unset:
* image-mode → `alt` if set, else the initials, else "Avatar"
* initials-mode → the initials.

#### `pub fn alt(mut self, alt: impl Into<LocalizedString>) -> Self`

Image alt text — distinct from `label` so a clickable avatar
can have a button label like "Open user menu" while still
describing the image as "Jane Doe".

#### `pub fn a11y_hidden(mut self) -> Self`

Hide from the a11y tree entirely. Use only when an adjacent
label conveys the avatar's meaning.

#### `pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Make the avatar activable. Promotes the a11y role to
`Role::Button` and adds `Action::Click` / `Action::Focus`. Tap,
Enter, and Space all fire the closure. Cursor changes to
`Pointer` on hover.

#### `pub fn has_popup(mut self, kind: teksilo_core::accesskit::HasPopup) -> Self`

Declare that this avatar is a disclosure trigger for a popup
(typically `HasPopup::Menu` for a user-menu trigger). Surfaces
via `set_has_popup` in the a11y node so screen readers
announce the avatar as "menu button" / "has popup". Only takes
effect when paired with `.on_activate_fn(...)` — without an
activation handler the avatar isn't a trigger.

#### `pub fn expanded_when(mut self, signal: impl Into<Prop<bool>>) -> Self`

Bind a signal reporting whether this avatar's popup is
currently visible. The wrapping Popover / overlay manager owns
the signal and flips it on show / dismiss; Avatar reads it in
`accessibility()` to publish `set_expanded`. Only meaningful
alongside `.has_popup(...)`.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip shown after the hover delay.
Mutually exclusive with `Self::rich_tooltip`,
`Self::rich_tooltip_content`, and `Self::composite_tooltip` —
this call clears the other three slots.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip identified by a registry key. The tooltip
content is resolved from the application's `TooltipRegistry` at
hover time. Mutually exclusive with the other tooltip setters —
this call clears the other three slots.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip from inline `crate::tooltip::TooltipContent`
without a registry key. Mutually exclusive with the other tooltip
setters — this call clears the other three slots.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip whose body is an arbitrary widget tree.
Shown after the longer `tooltip_delay_heavy` delay. Mutually
exclusive with the other tooltip setters — this call clears the
other three slots.

#### `pub fn name_signal(mut self, signal: Signal<String>) -> Self`

Bind the user's display name to a signal. The displayed
initials are auto-derived from the current value
(`derive_initials`), and the same value is used as the hash
seed for the background tint. Bound at
`BindingLevel::Rebuild` so the inner children regenerate on
flip — the canonical login-flow pattern:

```ignore
let user_name: Signal<String> = ctx.signal(String::new());
Avatar::with_initials(lit!("?"))        // logged-out fallback
    .name_signal(user_name.clone())
    .image_signal(user_avatar_signal)
```

#### `pub fn image_signal(mut self, signal: Signal<Option<Rc<RasterIcon>>>) -> Self`

Bind the image source. `None` ⇒ initials fallback. Each
non-`None` value is masked to the configured `AvatarShape` by
the inner `ImageWidget`. Bound at `BindingLevel::Rebuild`.

#### `pub fn alt_signal(mut self, signal: Signal<Option<String>>) -> Self`

Bind the image alt text. Bound at `BindingLevel::AccessibilityOnly`
— only the screen-reader projection is affected.

#### `pub fn label_signal(mut self, signal: Signal<Option<String>>) -> Self`

Bind the accessible label. Bound at
`BindingLevel::AccessibilityOnly`.

#### `pub fn presence_signal(mut self, signal: Signal<Option<AvatarPresence>>) -> Self`

Bind the presence indicator. `None` hides the dot. Bound at
`BindingLevel::Rebuild` — the dot's colour and the a11y
`description` flip together so a rebuild keeps both layers in
sync.
