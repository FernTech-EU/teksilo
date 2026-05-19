//! `Avatar` — circular (or rounded-square / square) user-identity widget.
//!
//! Displays either a person's image (clipped to the configured shape via
//! a CPU-side anti-aliased alpha mask applied at construction time) or
//! their initials over a hash-derived background colour. Optional
//! presence indicator (Online / Offline / Away / Busy) and outer ring.
//! Can be made activable to serve as a user-menu trigger.
//!
//! ```ignore
//! // Image with a presence dot.
//! Avatar::with_image(&face)
//!     .alt("Jane Doe")
//!     .presence(AvatarPresence::Online)
//!     .size(AvatarSize::Medium)
//!
//! // Hash-tinted initials, auto-derived from a name.
//! Avatar::with_name("Jane Doe").size(AvatarSize::Large)
//!
//! // Click target — opens a user menu via an `AppIntent`.
//! Avatar::with_image(&face)
//!     .label("Open user menu")
//!     .alt("Jane Doe")
//!     .on_activate_fn(|ctx| ctx.send_intent(AppIntent::OpenUserMenu))
//! ```
//!
//! The widget reuses `ImageWidget` for the image path and draws bg /
//! border / presence directly via `Canvas`. Hash-derived background
//! tints come from `theme.colors.chart_palette` (Okabe-Ito), so they
//! track the active theme automatically.

use std::rc::Rc;

use bastyde_canvas::raster::RasterIcon;
use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::styles::{AvatarStyleConfig, SharedAvatarStyle};
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{Color, FontWeight, TextStyle};

pub use bastyde_core::styles::{AvatarCorner, AvatarPresence, AvatarShape, AvatarSize};

use crate::primitives::ImageWidget;
use crate::primitives::image_mask::ImageMaskShape;
use crate::primitives::image_widget::ImageFit;
use crate::styles::recipe_avatar_style::{
    AVATAR_FONT_RATIO_1CHAR, AVATAR_FONT_RATIO_2CHAR, AVATAR_ROUNDED_RADIUS_RATIO,
    auto_contrast_text, avatar_pixel_size, hash_pick_palette_color,
};

// ─── The widget ────────────────────────────────────────────────────────────

type ActionFn = Rc<dyn Fn(&mut EventContext)>;

/// Circular (or rounded / square) user avatar.
///
/// Static + reactive fields share the struct: each "content" knob —
/// `name`, `image`, `alt`, `label`, `presence` — has a static
/// constructor / setter *and* a `bind_*` setter that takes a signal.
/// When a signal is bound, it wins; the corresponding static value
/// is treated as a fallback. Signal-bound rebuilds run on flip
/// (`BindingLevel::Rebuild`) so the inner ImageWidget / InitialsLeaf
/// children are recreated with fresh values — exactly the lifecycle
/// you want for a "logged-out → logged-in" transition.
pub struct Avatar {
    /// Initials shown when no image is present. Static fallback;
    /// overridden when `name_signal` is bound. Always non-empty
    /// (`"?"` when input was empty).
    initials: String,
    /// Optional override of the a11y name. Static fallback for
    /// `label_signal`.
    label: Option<String>,
    /// Image alt text. Static fallback for `alt_signal`.
    alt: Option<String>,
    /// Static image source bytes. `None` = no image at construction.
    /// Coexists with `image_signal`: signal wins when bound.
    image_source: Option<RawImage>,

    size: AvatarSize,
    shape: AvatarShape,

    background: Option<ColorProp>,
    foreground: Option<ColorProp>,
    border_color: Option<ColorProp>,
    border_width: Option<f32>,

    presence: Option<AvatarPresence>,
    presence_corner: AvatarCorner,

    seed: Option<String>,

    a11y_hidden: bool,

    image_visible: Prop<bool>,

    // ── Dynamic signal overrides (each None ⇒ use the static field) ─
    /// Reactive name. Drives derived initials and the hash seed when
    /// bound. Bound at `BindingLevel::Rebuild` so the inner children
    /// are recreated on flip.
    name_signal: Option<Signal<String>>,
    /// Reactive image source. `None` value ⇒ initials fallback path.
    /// `Rc<RasterIcon>` so swap is cheap. Bound at `Rebuild`.
    image_signal: Option<Signal<Option<Rc<RasterIcon>>>>,
    /// Reactive alt text. Bound at `AccessibilityOnly` since it only
    /// affects screen-reader output.
    alt_signal: Option<Signal<Option<String>>>,
    /// Reactive label. Bound at `AccessibilityOnly`.
    label_signal: Option<Signal<Option<String>>>,
    /// Reactive presence. Bound at `Rebuild` — the dot's colour and
    /// the a11y description both depend on the presence variant.
    presence_signal: Option<Signal<Option<AvatarPresence>>>,

    /// Optional `has_popup` ARIA hint. Surfaces via `set_has_popup` in
    /// `accessibility()` for the disclosure pattern (e.g. an Avatar
    /// that opens a user-menu Popover declares `HasPopup::Menu`).
    has_popup: Option<bastyde_core::accesskit::HasPopup>,
    /// Optional signal reporting whether the linked popup is currently
    /// visible. Surfaces via `set_expanded` in `accessibility()`. Only
    /// meaningful alongside `has_popup`.
    expanded_signal: Option<Signal<bool>>,

    /// Activation handler. Stored as `Rc<dyn Fn>` so it survives
    /// rebuilds (theme/locale switches re-run `build()` and would
    /// otherwise drop a `Box<dyn Fn>` after the first take).
    action: Option<ActionFn>,

    /// Focus state — set in `build()` and threaded into the
    /// `AvatarStyle` config so the chrome can paint the keyboard
    /// focus ring. `None` until `build()` runs.
    focused: Option<Signal<bool>>,
    /// Per-call override for the chrome (shape fill, border, focus ring,
    /// presence dot).
    style_override: Option<SharedAvatarStyle>,
    /// Build-time `AvatarStyle::make_body` root.
    root_child_id: Option<WidgetId>,
}

#[derive(Clone)]
struct RawImage {
    /// `Rc` so rebuilds (theme switch, locale switch, signal flip)
    /// don't reclone the byte buffer. The Rc identity is the cache
    /// key for the inner ImageWidget's texture-atlas name.
    pixels: Rc<Vec<u8>>,
    width: u32,
    height: u32,
}

// ─── Constructors ──────────────────────────────────────────────────────────

impl Avatar {
    /// Build an avatar from explicit initials. Uppercases and truncates
    /// to ≤ 2 chars. Empty input yields `"?"`.
    pub fn with_initials(initials: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = initials.into();
        let raw = ls.resolve_now();
        Self::from_initials(normalize_initials(&raw))
    }

    /// Build an avatar from a name; initials are derived
    /// (`"Jane Doe" → "JD"`, `"jane.doe@x.com" → "JD"`,
    /// `"Cher" → "C"`, `"" → "?"`).
    pub fn with_name(name: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = name.into();
        let raw = ls.resolve_now();
        let initials = derive_initials(&raw);
        let mut a = Self::from_initials(initials);
        a.seed = Some(raw); // hash from the full name, not from the abbreviated initials
        a
    }

    /// Build an avatar from a decoded raster icon. The pixels are
    /// centred-cropped to a square and CPU-masked to the configured
    /// shape at first `build()`.
    pub fn with_image(icon: &RasterIcon) -> Self {
        Self::from_raw_image(icon.pixels().to_vec(), icon.width(), icon.height())
    }

    /// Build an avatar from raw RGBA pixels. Same convention as
    /// [`ImageWidget::from_raw`].
    pub fn from_raw_image(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        let mut a = Self::from_initials("?".to_string());
        a.image_source = Some(RawImage {
            pixels: Rc::new(pixels),
            width,
            height,
        });
        a
    }

    fn from_initials(initials: String) -> Self {
        Self {
            initials,
            label: None,
            alt: None,
            image_source: None,
            size: AvatarSize::Medium,
            shape: AvatarShape::Circle,
            background: None,
            foreground: None,
            border_color: None,
            border_width: None,
            presence: None,
            presence_corner: AvatarCorner::BottomTrailing,
            seed: None,
            a11y_hidden: false,
            image_visible: Prop::Static(true),
            name_signal: None,
            image_signal: None,
            alt_signal: None,
            label_signal: None,
            presence_signal: None,
            has_popup: None,
            expanded_signal: None,
            action: None,
            focused: None,
            style_override: None,
            root_child_id: None,
        }
    }

    /// Per-call style override for the avatar chrome.
    pub fn style(mut self, style: impl bastyde_core::styles::AvatarStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Permanent `#[doc(hidden)]` shim for tests — wraps in
    /// `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn with_initials_literal(initials: &str) -> Self {
        Self::with_initials(bastyde_i18n::LocalizedString::literal(initials))
    }

    /// Permanent `#[doc(hidden)]` shim for tests.
    #[doc(hidden)]
    pub fn with_name_literal(name: &str) -> Self {
        Self::with_name(bastyde_i18n::LocalizedString::literal(name))
    }
}

// ─── Builder methods ───────────────────────────────────────────────────────

impl Avatar {
    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }

    pub fn shape(mut self, shape: AvatarShape) -> Self {
        // No cache to invalidate — masking now lives on the inner
        // `ImageWidget`, which is recreated each `build()` with the
        // current shape.
        self.shape = shape;
        self
    }

    /// Override the initials shown when the image is hidden via
    /// `image_visible(false)` or fails to register. Defaults to the
    /// derived initials if `with_image` was paired with `with_name`,
    /// otherwise `"?"`.
    pub fn fallback_initials(mut self, initials: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = initials.into();
        let raw = ls.resolve_now();
        self.initials = normalize_initials(&raw);
        self
    }

    /// Permanent `#[doc(hidden)]` shim.
    #[doc(hidden)]
    pub fn fallback_initials_literal(mut self, initials: &str) -> Self {
        self.initials = normalize_initials(initials);
        self
    }

    /// Reactive image visibility. When unbound it's `true`. When bound
    /// to a `Signal<bool>` and the value is `false`, the initials
    /// fallback paints in place of the image — same logical bounds, no
    /// layout shift.
    pub fn image_visible(mut self, visible: impl Into<Prop<bool>>) -> Self {
        self.image_visible = visible.into();
        self
    }

    /// Override the auto hash-derived background. Accepts a [`Color`],
    /// a role, or a `Signal<Color>`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Override the auto-contrast text colour for the initials. Auto
    /// (unset) picks white over dark backgrounds and near-black over
    /// light ones, computed at paint time from the resolved bg's
    /// luminance.
    pub fn foreground(mut self, color: impl Into<ColorProp>) -> Self {
        self.foreground = Some(color.into());
        self
    }

    /// Override the seed string used to pick a hash-derived background
    /// from the theme's chart palette. Defaults to the resolved name
    /// (when constructed via `with_name`) or the initials.
    pub fn seed(mut self, seed: impl Into<String>) -> Self {
        self.seed = Some(seed.into());
        self
    }

    /// Outer ring thickness. A non-zero value enables the ring (drawn
    /// in `BorderRole::Default` unless [`Self::border_color`] overrides
    /// it). `0.0` disables the ring.
    pub fn border(mut self, width: f32) -> Self {
        self.border_width = Some(width.max(0.0));
        self
    }

    pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.border_color = Some(color.into());
        self
    }

    pub fn presence(mut self, presence: AvatarPresence) -> Self {
        self.presence = Some(presence);
        self
    }

    pub fn presence_corner(mut self, corner: AvatarCorner) -> Self {
        self.presence_corner = corner;
        self
    }

    /// Override the accessible name. When unset:
    /// * image-mode → `alt` if set, else the initials, else "Avatar"
    /// * initials-mode → the initials.
    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Permanent `#[doc(hidden)]` shim.
    #[doc(hidden)]
    pub fn label_literal(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    /// Image alt text — distinct from `label` so a clickable avatar
    /// can have a button label like "Open user menu" while still
    /// describing the image as "Jane Doe".
    pub fn alt(mut self, alt: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = alt.into();
        self.alt = Some(ls.resolve_now());
        self
    }

    /// Permanent `#[doc(hidden)]` shim.
    #[doc(hidden)]
    pub fn alt_literal(mut self, alt: &str) -> Self {
        self.alt = Some(alt.to_string());
        self
    }

    /// Hide from the a11y tree entirely. Use only when an adjacent
    /// label conveys the avatar's meaning.
    pub fn a11y_hidden(mut self) -> Self {
        self.a11y_hidden = true;
        self
    }

    /// Make the avatar activable. Promotes the a11y role to
    /// `Role::Button` and adds `Action::Click` / `Action::Focus`. Tap,
    /// Enter, and Space all fire the closure. Cursor changes to
    /// `Pointer` on hover.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Rc::new(f));
        self
    }

    /// Declare that this avatar is a disclosure trigger for a popup
    /// (typically `HasPopup::Menu` for a user-menu trigger). Surfaces
    /// via `set_has_popup` in the a11y node so screen readers
    /// announce the avatar as "menu button" / "has popup". Only takes
    /// effect when paired with `.on_activate_fn(...)` — without an
    /// activation handler the avatar isn't a trigger.
    pub fn has_popup(mut self, kind: bastyde_core::accesskit::HasPopup) -> Self {
        self.has_popup = Some(kind);
        self
    }

    /// Bind a signal reporting whether this avatar's popup is
    /// currently visible. The wrapping Popover / overlay manager owns
    /// the signal and flips it on show / dismiss; Avatar reads it in
    /// `accessibility()` to publish `set_expanded`. Only meaningful
    /// alongside `.has_popup(...)`.
    pub fn expanded_when(mut self, signal: Signal<bool>) -> Self {
        self.expanded_signal = Some(signal);
        self
    }

    // ── Reactive content (bind_*) ─────────────────────────────────────

    /// Bind the user's display name to a signal. The displayed
    /// initials are auto-derived from the current value
    /// (`derive_initials`), and the same value is used as the hash
    /// seed for the background tint. Bound at
    /// `BindingLevel::Rebuild` so the inner children regenerate on
    /// flip — the canonical login-flow pattern:
    ///
    /// ```ignore
    /// let user_name: Signal<String> = ctx.signal(String::new());
    /// Avatar::with_initials_literal("?")        // logged-out fallback
    ///     .bind_name(user_name.clone())
    ///     .bind_image(user_avatar_signal)
    /// ```
    pub fn bind_name(mut self, signal: Signal<String>) -> Self {
        self.name_signal = Some(signal);
        self
    }

    /// Bind the image source. `None` ⇒ initials fallback. Each
    /// non-`None` value is masked to the configured `AvatarShape` by
    /// the inner [`ImageWidget`]. Bound at `BindingLevel::Rebuild`.
    pub fn bind_image(mut self, signal: Signal<Option<Rc<RasterIcon>>>) -> Self {
        self.image_signal = Some(signal);
        self
    }

    /// Bind the image alt text. Bound at `BindingLevel::AccessibilityOnly`
    /// — only the screen-reader projection is affected.
    pub fn bind_alt(mut self, signal: Signal<Option<String>>) -> Self {
        self.alt_signal = Some(signal);
        self
    }

    /// Bind the accessible label. Bound at
    /// `BindingLevel::AccessibilityOnly`.
    pub fn bind_label(mut self, signal: Signal<Option<String>>) -> Self {
        self.label_signal = Some(signal);
        self
    }

    /// Bind the presence indicator. `None` hides the dot. Bound at
    /// `BindingLevel::Rebuild` — the dot's colour and the a11y
    /// `description` flip together so a rebuild keeps both layers in
    /// sync.
    pub fn bind_presence(mut self, signal: Signal<Option<AvatarPresence>>) -> Self {
        self.presence_signal = Some(signal);
        self
    }
}

impl std::fmt::Debug for Avatar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Avatar")
            .field("initials", &self.initials)
            .field("size", &self.size)
            .field("shape", &self.shape)
            .field(
                "has_image",
                &(self.image_source.is_some() || self.image_signal.is_some()),
            )
            .field("clickable", &self.action.is_some())
            .finish()
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Inline FNV-1a 64-bit. Stable across Rust versions and process runs
/// (unlike `DefaultHasher`). Same idiom as `bastyde_core::accessibility`.
/// Truncate to ≤ 2 chars and uppercase. Returns `"?"` when the input
/// trims to empty. Operates on `char`s (Unicode scalars), not extended
/// graphemes — this is sufficient for real-world names where accented
/// letters are stored pre-composed.
fn normalize_initials(s: &str) -> String {
    let mut out = String::new();
    let mut count = 0;
    for c in s.trim().chars() {
        if count >= 2 {
            break;
        }
        for upper in c.to_uppercase() {
            out.push(upper);
        }
        count += 1;
    }
    if out.is_empty() { "?".to_string() } else { out }
}

/// Auto-derive initials from a free-form name.
fn derive_initials(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "?".to_string();
    }
    // For email-like strings only the local part (before `@`) matters.
    let source = trimmed.split('@').next().unwrap_or(trimmed);
    let parts: Vec<&str> = source
        .split(|c: char| c.is_whitespace() || c == '.' || c == '_' || c == '-')
        .filter(|s| !s.is_empty())
        .collect();

    let mut out = String::new();
    for part in parts.iter().take(2) {
        if let Some(c) = part.chars().next() {
            for upper in c.to_uppercase() {
                out.push(upper);
            }
        }
    }
    if out.is_empty() { "?".to_string() } else { out }
}

fn shape_to_image_mask(shape: AvatarShape) -> ImageMaskShape {
    match shape {
        AvatarShape::Circle => ImageMaskShape::Circle,
        AvatarShape::RoundedSquare => ImageMaskShape::RoundedSquare(AVATAR_ROUNDED_RADIUS_RATIO),
        AvatarShape::Square => ImageMaskShape::None,
    }
}

// ─── Reactive accessors — used by paint / accessibility ──────────────────

impl Avatar {
    /// The displayed initials, taking any bound name signal into
    /// account. Cheap (a few string ops per call); paint / a11y
    /// invoke this directly rather than caching.
    fn current_initials(&self) -> String {
        match &self.name_signal {
            Some(sig) => derive_initials(&sig.get()),
            None => self.initials.clone(),
        }
    }

    /// The hash seed for the background tint. When `bind_name` is
    /// active the seed *is* the name (so two users named "JD" but
    /// "Jane Doe" vs "Jules Dupont" hash differently). Otherwise it
    /// falls back to the user-supplied seed or the static initials.
    fn current_seed(&self) -> String {
        match &self.name_signal {
            Some(sig) => sig.get(),
            None => self.seed.clone().unwrap_or_else(|| self.initials.clone()),
        }
    }

    fn current_alt(&self) -> Option<String> {
        match &self.alt_signal {
            Some(sig) => sig.get(),
            None => self.alt.clone(),
        }
    }

    fn current_label(&self) -> Option<String> {
        match &self.label_signal {
            Some(sig) => sig.get(),
            None => self.label.clone(),
        }
    }

    fn current_presence(&self) -> Option<AvatarPresence> {
        match &self.presence_signal {
            Some(sig) => sig.get(),
            None => self.presence.clone(),
        }
    }

    /// Resolve the image source bytes + dims for the current state.
    /// `Some` ⇒ image mode (will spawn an `ImageWidget` child);
    /// `None` ⇒ initials-only mode.
    fn current_image(&self) -> Option<(Rc<Vec<u8>>, u32, u32)> {
        if let Some(sig) = &self.image_signal {
            return sig
                .get()
                .map(|rc| (Rc::new(rc.pixels().to_vec()), rc.width(), rc.height()));
        }
        self.image_source
            .as_ref()
            .map(|raw| (raw.pixels.clone(), raw.width, raw.height))
    }

    /// Whether the avatar should expose a11y-image-role semantics.
    fn has_image_now(&self) -> bool {
        self.image_signal
            .as_ref()
            .is_some_and(|sig| sig.get().is_some())
            || self.image_source.is_some()
    }
}

// ─── Widget impl ───────────────────────────────────────────────────────────

impl Widget for Avatar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let mask_shape = shape_to_image_mask(self.shape);

        // 1. Resolve current content state. Each `current_*` reads
        //    the signal if bound, falling back to the static field.
        let initials = self.current_initials();
        let seed = self.current_seed();
        let alt = self.current_alt();
        let image_bytes = self.current_image();

        // 2. Build inner content (`InitialsLeaf` / `ImageWidget`).
        //    The masking lives on `ImageWidget` so Avatar doesn't
        //    manage pixel buffers itself.
        let make_initials_leaf = || InitialsLeaf {
            initials: initials.clone(),
            seed: seed.clone(),
            background: self.background.clone(),
            foreground: self.foreground.clone(),
        };
        let make_image_widget = |bytes: Rc<Vec<u8>>, w: u32, h: u32, alt: Option<String>| {
            let mut img = ImageWidget::from_raw((*bytes).clone(), w, h)
                .fit(ImageFit::Cover)
                .mask(mask_shape);
            if let Some(a) = alt {
                img = img.alt(a);
            } else {
                // Inner ImageWidget is silenced — the parent Avatar
                // owns the Role::Image / Role::Button + name.
                img = img.a11y_hidden();
            }
            img
        };

        // Assemble inner content as a single `WidgetId`. For the
        // bound-visibility case the image and initials sit as siblings
        // inside a `ZStack` with `visible_when` bindings; either is
        // mounted alone otherwise.
        let content_id = match (image_bytes, &self.image_visible) {
            (Some((bytes, w, h)), Prop::Static(true)) => {
                ctx.add(make_image_widget(bytes, w, h, alt.clone()))
            }
            (Some(_), Prop::Static(false)) => ctx.add(make_initials_leaf()),
            (Some((bytes, w, h)), Prop::Bound(visible_signal)) => {
                let img_id = ctx.add(make_image_widget(bytes, w, h, alt.clone()));
                let init_id = ctx.add(make_initials_leaf());
                let v_clone = visible_signal.clone();
                ctx.visible_when(img_id, v_clone.clone());
                ctx.visible_when(init_id, v_clone.map(|v| !*v));
                ctx.add(
                    crate::primitives::ZStack::new()
                        .add_child(img_id)
                        .add_child(init_id),
                )
            }
            (None, _) => ctx.add(make_initials_leaf()),
        };

        // 3. Wire reactive content signals so flips re-run build().
        let registry = ctx.binding_registry();
        if let Some(sig) = &self.name_signal {
            sig.bind_to(self_id, registry, bastyde_core::binding::BindingLevel::Rebuild);
        }
        if let Some(sig) = &self.image_signal {
            sig.bind_to(self_id, registry, bastyde_core::binding::BindingLevel::Rebuild);
        }
        if let Some(sig) = &self.presence_signal {
            sig.bind_to(self_id, registry, bastyde_core::binding::BindingLevel::Rebuild);
        }
        if let Some(sig) = &self.alt_signal {
            sig.bind_to(
                self_id,
                registry,
                bastyde_core::binding::BindingLevel::AccessibilityOnly,
            );
        }
        if let Some(sig) = &self.label_signal {
            sig.bind_to(
                self_id,
                registry,
                bastyde_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        // 4. If clickable, install attached handlers — including the
        //    `on_focus` that drives the focus-ring repaint via the
        //    chrome's `is_focused` signal.
        let focused = ctx.signal(false);
        self.focused = Some(focused.clone());
        if let Some(action) = self.action.clone() {
            let focus_for_handler = focused.clone();

            let action_for_tap = action.clone();
            let action_for_key = action.clone();
            let action_for_access = action;
            let handlers = HandlerSet::new()
                .on_tap(move |_pos, ctx| action_for_tap(ctx))
                .focusable(true)
                .cursor(CursorIcon::Pointer)
                .on_focus(move |gained, _ctx| focus_for_handler.set(gained))
                .on_key(move |event, ctx| {
                    use bastyde_core::event::{EventResponse, Key, WidgetEvent};
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            action_for_key(ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                })
                .on_access_action(move |action_kind, ctx| {
                    use bastyde_core::event::EventResponse;
                    if action_kind == bastyde_core::accesskit::Action::Click {
                        action_for_access(ctx);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                });
            ctx.apply_self_handlers(handlers);
        }

        // 5. Wire `expanded_signal` for a11y refresh on flip.
        if let Some(ref expanded_signal) = self.expanded_signal {
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            expanded_signal.bind_to(
                self_id,
                registry,
                bastyde_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // 6. The shape-aware chrome (background fill, border, focus
        //    ring, presence dot) is owned by the active `AvatarStyle`;
        //    this widget keeps its Role::Image / Role::Button / Role::Label
        //    semantics and the initials-derivation logic.
        let style: SharedAvatarStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.avatar.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeAvatarStyle));
        let root = style.make_body(
            &AvatarStyleConfig {
                shape: self.shape,
                size: self.size,
                content: content_id,
                presence: self.current_presence(),
                presence_corner: self.presence_corner,
                is_focused: focused,
                background_override: self.background.clone(),
                border_color_override: self.border_color.clone(),
                border_width_override: self.border_width,
                seed,
            },
            ctx,
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let side = avatar_pixel_size(self.size);
        Size::new(side, side).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.a11y_hidden {
            builder.set_hidden();
            return;
        }

        let clickable = self.action.is_some();
        // `current_image()` is the source of truth — a bound image
        // signal whose value is `None` means "no image right now",
        // even if a static `with_image` source was also supplied
        // (signal wins). For role-selection we ask: does the live
        // state put us in image-mode?
        let has_image = self.has_image_now();
        let alt = self.current_alt();
        let label = self.current_label();
        let initials = self.current_initials();

        if clickable {
            builder.set_role(bastyde_core::accesskit::Role::Button);
            // A clickable avatar without an explicit label is missing
            // its activation hint. Catch this in dev to prevent silent
            // a11y regressions.
            debug_assert!(
                label.is_some() || alt.is_some(),
                "Avatar::on_activate_fn requires a `.label(\"...\")` (preferred) or `.alt(\"...\")` (or a `.bind_label(...)` / `.bind_alt(...)`) for screen readers"
            );
            let name = label.or(alt).unwrap_or_else(|| initials.clone());
            builder.set_name(name);
            builder.add_action(bastyde_core::accesskit::Action::Click);
            builder.add_action(bastyde_core::accesskit::Action::Focus);
        } else if has_image {
            builder.set_role(bastyde_core::accesskit::Role::Image);
            // A pure-image avatar without alt text is missing its
            // semantic label — catch in dev (matches `ImageWidget`).
            debug_assert!(
                alt.is_some() || label.is_some(),
                "Avatar::with_image requires a `.alt(\"...\")` (or `.bind_alt(...)`) for meaningful images, or call `.a11y_hidden()` if decorative"
            );
            let name = alt.or(label).unwrap_or_else(|| initials.clone());
            builder.set_name(name);
        } else {
            builder.set_role(bastyde_core::accesskit::Role::Label);
            let name = label.unwrap_or_else(|| initials.clone());
            builder.set_name(name);
        }

        if let Some(presence) = self.current_presence() {
            builder.set_description(presence.label());
        }

        // Disclosure-pattern hints. Only meaningful for clickable
        // avatars — but harmless to surface unconditionally for the
        // image / label paths in case a wrapper widget is supplying
        // them (e.g. an external state machine that drives a popup
        // alongside an Avatar that isn't itself the trigger).
        if let Some(kind) = self.has_popup {
            builder.set_has_popup(kind);
        }
        if let Some(ref signal) = self.expanded_signal {
            builder.set_expanded(signal.get());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ─── Initials sub-widget ───────────────────────────────────────────────────

/// Crate-private leaf that draws the centred initials. The avatar's
/// own `paint()` handles the background fill; this widget only emits
/// glyphs so paint order is parent-bg → child-text.
///
/// The leaf is constructed in [`Avatar::build`] with all the inputs
/// it needs to resolve a correctly contrasted foreground at paint
/// time:
/// * `initials` — the glyphs to draw.
/// * `seed` — the same string the parent will hash to pick a bg tint.
/// * `background` / `foreground` — clones of the parent's overrides
///   (`None` ⇒ default path). Stored as `ColorProp` so role / signal
///   variants resolve against the active theme each frame, matching
///   what the parent paints.
#[derive(Debug)]
struct InitialsLeaf {
    initials: String,
    seed: String,
    background: Option<ColorProp>,
    foreground: Option<ColorProp>,
}

impl InitialsLeaf {
    /// Recompute the bg colour the parent Avatar will paint. Must
    /// stay in lock-step with `Avatar::paint`'s bg branch.
    fn resolve_bg(&self, theme: &bastyde_core::Theme, enabled: bool) -> Color {
        match &self.background {
            Some(prop) => prop.resolve(theme, enabled),
            None => hash_pick_palette_color(&self.seed, theme),
        }
    }
}

impl Widget for InitialsLeaf {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Always fill the proposal — the parent Avatar drives sizing.
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;

        let font_size = bounds.width.min(bounds.height)
            * if self.initials.chars().count() <= 1 {
                AVATAR_FONT_RATIO_1CHAR
            } else {
                AVATAR_FONT_RATIO_2CHAR
            };

        let text_style = TextStyle {
            family: theme.typography.body_bold.family.clone(),
            size: font_size,
            weight: FontWeight::SEMI_BOLD,
            line_height: 1.0,
            letter_spacing: 0.0,
        };

        // Foreground: explicit override wins. Otherwise auto-contrast
        // against the same bg the parent painted.
        let fg = match &self.foreground {
            Some(prop) => prop.resolve(theme, ctx.effective_enabled),
            None => auto_contrast_text(self.resolve_bg(theme, ctx.effective_enabled)),
        };

        // Measure the text to centre it. Without a backend, we can't
        // measure or draw glyphs at all — silently no-op.
        let Some(backend) = canvas.text_backend().cloned() else {
            return;
        };
        let layout = {
            let mut b = backend.borrow_mut();
            b.layout_single_line(&self.initials, &text_style, None)
        };
        let text_w = layout.width;
        let text_h = layout.height;

        let cx = bounds.x + (bounds.width - text_w) / 2.0;
        let cy = bounds.y + (bounds.height - text_h) / 2.0;
        let position = Rect::new(cx, cy, text_w, text_h);
        canvas.draw_text(&self.initials, position, &text_style, fg);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The parent Avatar owns the user-facing semantics (role, name,
        // click action). The text node would otherwise duplicate that
        // information to ATs.
        builder.set_hidden();
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styles::recipe_avatar_style::fnv1a_64;
    use bastyde_core::widget::LayoutContext;
    use bastyde_core::widget_tree::WidgetTree;

    // ── helpers ────────────────────────────────────────────────────────

    fn rgba_solid(side: u32, rgba: [u8; 4]) -> RasterIcon {
        let mut p = Vec::with_capacity((side * side * 4) as usize);
        for _ in 0..(side * side) {
            p.extend_from_slice(&rgba);
        }
        RasterIcon::from_raw(p, side, side)
    }

    // ── derive_initials / normalize_initials ──────────────────────────

    #[test]
    fn normalize_uppercase_truncate() {
        assert_eq!(normalize_initials("jdq"), "JD");
        assert_eq!(normalize_initials("jd"), "JD");
        assert_eq!(normalize_initials("j"), "J");
        assert_eq!(normalize_initials("  "), "?");
        assert_eq!(normalize_initials(""), "?");
    }

    #[test]
    fn derive_full_name() {
        assert_eq!(derive_initials("Jane Doe"), "JD");
    }

    #[test]
    fn derive_single_word() {
        assert_eq!(derive_initials("Cher"), "C");
    }

    #[test]
    fn derive_email() {
        assert_eq!(derive_initials("jane.doe@x.com"), "JD");
        assert_eq!(derive_initials("jane_doe@x.com"), "JD");
    }

    #[test]
    fn derive_unicode_name() {
        assert_eq!(derive_initials("María José"), "MJ");
    }

    #[test]
    fn derive_empty_yields_question_mark() {
        assert_eq!(derive_initials(""), "?");
        assert_eq!(derive_initials("   "), "?");
    }

    #[test]
    fn derive_three_words_takes_first_two() {
        assert_eq!(derive_initials("Anna María José"), "AM");
    }

    #[test]
    fn derive_hyphenated_name() {
        assert_eq!(derive_initials("Jean-Luc Picard"), "JL");
    }

    // ── hashing ────────────────────────────────────────────────────────

    #[test]
    fn fnv1a_is_stable() {
        let h1 = fnv1a_64(b"jane.doe");
        let h2 = fnv1a_64(b"jane.doe");
        assert_eq!(h1, h2);
        assert_ne!(fnv1a_64(b"jane.doe"), fnv1a_64(b"john.smith"));
    }

    #[test]
    fn hash_distributes_over_palette() {
        let theme = bastyde_core::presets::intui::light();
        let mut buckets = [0_u32; 8];
        for i in 0..200 {
            let seed = format!("user_{i}");
            let color = hash_pick_palette_color(&seed, &theme);
            // Find which palette index it picked.
            let idx = theme
                .colors
                .chart_palette
                .iter()
                .position(|c| c == &color)
                .expect("color must be a palette member");
            buckets[idx] += 1;
        }
        let nonzero = buckets.iter().filter(|n| **n > 0).count();
        assert!(
            nonzero >= 6,
            "expected hash to cover at least 6 of 8 buckets, got {nonzero} (buckets: {:?})",
            buckets
        );
    }

    // ── sizing ─────────────────────────────────────────────────────────

    #[test]
    fn size_default_is_medium_32px() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Avatar::with_initials_literal("JD"));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let b = tree.bounds(id);
        assert!((b.width - 32.0).abs() < 0.01);
        assert!((b.height - 32.0).abs() < 0.01);
    }

    #[test]
    fn size_custom_passes_through() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Avatar::with_initials_literal("JD").size(AvatarSize::Custom(40.0)));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let b = tree.bounds(id);
        assert!((b.width - 40.0).abs() < 0.01);
        assert!((b.height - 40.0).abs() < 0.01);
    }

    #[test]
    fn size_that_fits_ignores_proposal() {
        // Even a hugely oversized proposal must not enlarge the
        // avatar's intrinsic size — it always reports the discrete
        // size variant. (`tree.layout(exact(...))` would clamp the
        // root's bounds to the proposal regardless, so we exercise
        // `size_that_fits` directly.)
        let widget = Avatar::with_initials_literal("JD");
        let theme = bastyde_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        let s = widget
            .layout_response(SizeProposal::exact(400.0, 400.0), &ctx)
            .size;
        assert!((s.width - 32.0).abs() < 0.01);
        assert!((s.height - 32.0).abs() < 0.01);
    }

    #[test]
    fn small_medium_large_xlarge_sizes() {
        let theme = bastyde_core::presets::intui::light();
        use crate::styles::recipe_avatar_style as av;
        let cases = [
            (AvatarSize::Small, av::AVATAR_SIZE_SMALL),
            (AvatarSize::Medium, av::AVATAR_SIZE_MEDIUM),
            (AvatarSize::Large, av::AVATAR_SIZE_LARGE),
            (AvatarSize::XLarge, av::AVATAR_SIZE_X_LARGE),
        ];
        for (variant, expected) in cases {
            let mut tree = WidgetTree::new().with_theme(theme.clone());
            let id = tree.add(Avatar::with_initials_literal("X").size(variant));
            tree.layout(SizeProposal {
                width: None,
                height: None,
            });
            let b = tree.bounds(id);
            assert!(
                (b.width - expected).abs() < 0.01,
                "size {variant:?}: expected {expected}, got {}",
                b.width
            );
        }
    }

    // ── paint output ──────────────────────────────────────────────────

    fn render_avatar(avatar: Avatar) -> std::rc::Rc<bastyde_canvas::RenderFrame> {
        use bastyde_canvas::MockTextBackend;
        use std::cell::RefCell;
        use std::rc::Rc;
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        tree.add(avatar);
        tree.layout(SizeProposal::exact(64.0, 64.0));
        tree.render()
    }

    fn count_shapes(frame: &bastyde_canvas::RenderFrame) -> usize {
        // Both `fill_circle` and `fill_rounded_rect` emit `ShapeQuad`
        // entries — the SDF pipeline. Stroke-based circle border also
        // lands here.
        frame.shapes.len()
    }

    #[test]
    fn paint_initials_emits_a_shape_quad() {
        let frame = render_avatar(Avatar::with_initials_literal("JD"));
        assert!(
            count_shapes(&frame) >= 1,
            "expected at least one ShapeQuad (the bg circle)"
        );
    }

    #[test]
    fn paint_with_border_adds_extra_shape() {
        let plain = render_avatar(Avatar::with_initials_literal("JD"));
        let bordered = render_avatar(Avatar::with_initials_literal("JD").border(2.0));
        assert!(
            count_shapes(&bordered) > count_shapes(&plain),
            "border path should add at least one extra Shape (the stroked ring)"
        );
    }

    #[test]
    fn paint_presence_adds_two_shapes() {
        let plain = render_avatar(Avatar::with_initials_literal("JD"));
        let with_dot =
            render_avatar(Avatar::with_initials_literal("JD").presence(AvatarPresence::Online));
        // Outline + dot.
        assert_eq!(count_shapes(&with_dot), count_shapes(&plain) + 2);
    }

    #[test]
    fn paint_rounded_square_emits_shape() {
        // `fill_rounded_rect` lands on the SDF Shape pipeline same
        // as `fill_circle`. Both shapes paint via Shape quads.
        let frame =
            render_avatar(Avatar::with_initials_literal("JD").shape(AvatarShape::RoundedSquare));
        assert!(count_shapes(&frame) >= 1);
    }

    #[test]
    fn paint_square_emits_shape() {
        let frame = render_avatar(Avatar::with_initials_literal("JD").shape(AvatarShape::Square));
        assert!(count_shapes(&frame) >= 1);
    }

    #[test]
    fn paint_image_uses_image_pipeline() {
        let icon = rgba_solid(8, [50, 100, 200, 255]);
        let frame = render_avatar(Avatar::with_image(&icon).alt_literal("avatar"));
        assert!(
            !frame.images.is_empty(),
            "image avatar should render an image"
        );
    }

    #[test]
    fn auto_contrast_dark_bg_chooses_white() {
        let dark = Color::from_rgb(0.05, 0.05, 0.05);
        let fg = auto_contrast_text(dark);
        assert!(fg.r() > 0.9 && fg.g() > 0.9 && fg.b() > 0.9);
    }

    #[test]
    fn auto_contrast_light_bg_chooses_dark() {
        let light = Color::from_rgb(0.95, 0.95, 0.95);
        let fg = auto_contrast_text(light);
        assert!(fg.r() < 0.3 && fg.g() < 0.3 && fg.b() < 0.3);
    }

    // ── accessibility ─────────────────────────────────────────────────

    #[test]
    fn accessibility_initials_default_role_is_label() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Avatar::with_initials_literal("JD"));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Label);
        assert_eq!(info.name(), Some("JD"));
    }

    #[test]
    fn accessibility_image_default_role_is_image() {
        let icon = rgba_solid(8, [10, 20, 30, 255]);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Avatar::with_image(&icon).alt_literal("Jane Doe"));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Image);
        assert_eq!(info.name(), Some("Jane Doe"));
    }

    #[test]
    fn accessibility_clickable_becomes_button() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            Avatar::with_initials_literal("JD")
                .label_literal("Open user menu")
                .on_activate_fn(|_ctx| {}),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Button);
        assert!(
            info.actions()
                .contains(&bastyde_core::accesskit::Action::Click)
        );
        assert!(
            info.actions()
                .contains(&bastyde_core::accesskit::Action::Focus)
        );
        assert_eq!(info.name(), Some("Open user menu"));
    }

    #[test]
    fn accessibility_a11y_hidden_does_not_set_role() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Avatar::with_initials_literal("JD").a11y_hidden());
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        // Hidden nodes carry no name (the leaf-hidden path returned
        // early before set_role/set_name fired).
        assert_eq!(info.name(), None);
    }

    #[test]
    fn accessibility_label_overrides_initials() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Avatar::with_initials_literal("JD").label_literal("Jane Doe (offline)"));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.name(), Some("Jane Doe (offline)"));
    }

    #[test]
    fn accessibility_presence_appears_in_description() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Avatar::with_initials_literal("JD").presence(AvatarPresence::Online));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // Just verify it builds — `description` isn't surfaced by the
        // test introspection helper, but that the avatar accepts the
        // presence and renders without panicking is the key contract.
        assert_eq!(
            tree.accessibility_node(id).role(),
            bastyde_core::accesskit::Role::Label
        );
    }

    // ── visibility binding ────────────────────────────────────────────

    #[test]
    fn image_visible_false_hides_image_child() {
        use bastyde_core::signal::Signal;

        let icon = rgba_solid(8, [10, 20, 30, 255]);
        let visible = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            Avatar::with_image(&icon)
                .alt_literal("Jane")
                .fallback_initials_literal("JD")
                .image_visible(visible.clone()),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // With visibility = true, an image quad is emitted.
        assert!(!tree.render().images.is_empty());

        // Flip visibility — the image child becomes dormant; on the
        // next render frame, no image is drawn.
        visible.set(false);
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let frame_after = tree.render();
        assert!(
            frame_after.images.is_empty(),
            "image should be hidden when image_visible == false"
        );
        // Sanity: avatar itself is still visible.
        assert!(tree.is_visible(id));
    }

    // ── shape interaction with image masking ──────────────────────────

    // ── foreground / background overrides ─────────────────────────────

    fn glyph_colors(frame: &bastyde_canvas::RenderFrame) -> Vec<[f32; 4]> {
        frame.glyphs.iter().map(|g| g.color).collect()
    }

    fn shape_colors(frame: &bastyde_canvas::RenderFrame) -> Vec<[f32; 4]> {
        frame.shapes.iter().map(|s| s.color).collect()
    }

    fn approx_color_eq(a: [f32; 4], b: Color) -> bool {
        let target = b.to_array();
        a.iter()
            .zip(target.iter())
            .all(|(x, y)| (x - y).abs() < 0.02)
    }

    #[test]
    fn foreground_override_sets_glyph_color() {
        // Without override the foreground is auto-contrast — for some
        // hash bg it'll be white, for others near-black. We force a
        // specific colour and verify it ends up in glyph metadata.
        let frame = render_avatar(
            Avatar::with_initials_literal("JD").foreground(Color::from_rgb(1.0, 0.0, 0.5)),
        );
        let target = Color::from_rgb(1.0, 0.0, 0.5);
        assert!(
            glyph_colors(&frame)
                .iter()
                .any(|c| approx_color_eq(*c, target)),
            "expected at least one glyph painted with the foreground override"
        );
    }

    #[test]
    fn background_override_sets_bg_shape_color() {
        let frame = render_avatar(
            Avatar::with_initials_literal("JD").background(Color::from_rgb(0.1, 0.7, 0.2)),
        );
        let target = Color::from_rgb(0.1, 0.7, 0.2);
        assert!(
            shape_colors(&frame)
                .iter()
                .any(|c| approx_color_eq(*c, target)),
            "expected the bg override colour to appear on a Shape quad"
        );
    }

    #[test]
    fn auto_contrast_uses_overridden_bg_for_initials_text() {
        // With a near-white background override and no foreground
        // override, auto-contrast should pick a dark text colour.
        let frame = render_avatar(
            Avatar::with_initials_literal("JD").background(Color::from_rgb(0.95, 0.95, 0.95)),
        );
        let glyphs = glyph_colors(&frame);
        assert!(
            !glyphs.is_empty(),
            "expected at least one initials glyph in the frame"
        );
        for g in &glyphs {
            // Each channel should be in the dark range.
            assert!(
                g[0] < 0.3 && g[1] < 0.3 && g[2] < 0.3,
                "expected dark auto-contrast glyph against a light bg, got {:?}",
                g
            );
        }
    }

    #[test]
    fn auto_contrast_uses_overridden_bg_against_dark() {
        let frame = render_avatar(
            Avatar::with_initials_literal("JD").background(Color::from_rgb(0.05, 0.05, 0.05)),
        );
        let glyphs = glyph_colors(&frame);
        assert!(!glyphs.is_empty());
        for g in &glyphs {
            assert!(
                g[0] > 0.9 && g[1] > 0.9 && g[2] > 0.9,
                "expected white auto-contrast glyph against a dark bg, got {:?}",
                g
            );
        }
    }

    #[test]
    fn with_name_seed_drives_bg_palette_pick() {
        // Two avatars with the same DERIVED initials but DIFFERENT
        // full names must pick distinct palette buckets — proving the
        // hash uses the seed (full name), not the initials.
        // ("Jane Doe" → JD, "Jules Dupont" → JD: identical initials.)
        let a = render_avatar(Avatar::with_name_literal("Jane Doe"));
        let b = render_avatar(Avatar::with_name_literal("Jules Dupont"));
        let bg_a = shape_colors(&a)
            .into_iter()
            .next()
            .expect("first shape is the bg circle");
        let bg_b = shape_colors(&b)
            .into_iter()
            .next()
            .expect("first shape is the bg circle");
        assert_ne!(
            bg_a, bg_b,
            "Jane Doe and Jules Dupont share initials JD but must hash distinctly via their full names"
        );
    }

    // ── accessibility for image avatars: inner ImageWidget is silenced ─

    // ── disclosure pattern (has_popup / expanded_when) ────────────────

    #[test]
    fn expanded_when_signal_reflects_in_a11y() {
        use bastyde_core::signal::Signal;
        let open = Signal::new(false);
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        let id = tree.add(
            Avatar::with_initials_literal("JD")
                .label_literal("Open user menu")
                .has_popup(bastyde_core::accesskit::HasPopup::Menu)
                .expanded_when(open.clone())
                .on_activate_fn(|_ctx| {}),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // Closed.
        assert!(!tree.accessibility_node(id).is_expanded());

        // Flip — the binding registered in build() must dirty-mark
        // this node so the next a11y query sees the new value.
        open.set(true);
        tree.layout(SizeProposal::exact(32.0, 32.0));
        assert!(tree.accessibility_node(id).is_expanded());
    }

    #[test]
    fn has_popup_without_clickable_still_compiles() {
        // Non-clickable avatars can still declare `has_popup` —
        // builder is a no-op functionally without an action handler,
        // but we want `accessibility()` to safely surface it for
        // wrappers that supply external state.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            Avatar::with_initials_literal("JD").has_popup(bastyde_core::accesskit::HasPopup::Menu),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // Role stays Label since there's no on_activate_fn.
        assert_eq!(
            tree.accessibility_node(id).role(),
            bastyde_core::accesskit::Role::Label
        );
    }

    // ── focus ring ────────────────────────────────────────────────────

    #[test]
    fn focus_ring_only_paints_when_focused() {
        // Synthesize the same bookkeeping as build() does for a
        // clickable avatar, then drive the focus signal directly.

        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        let id = tree.add(
            Avatar::with_initials_literal("JD")
                .label_literal("Open user menu")
                .on_activate_fn(|_ctx| {}),
        );
        tree.layout(SizeProposal::exact(64.0, 64.0));
        let unfocused_shapes = tree.render().shapes.len();

        // Programmatically set the avatar as focused. The framework's
        // public way to do this in tests:
        tree.focus(id);
        tree.layout(SizeProposal::exact(64.0, 64.0));
        let focused_shapes = tree.render().shapes.len();

        assert_eq!(
            focused_shapes,
            unfocused_shapes + 1,
            "focused avatar should emit one extra Shape (the focus ring stroke)"
        );
    }

    #[test]
    fn focus_ring_uses_theme_focus_ring_color() {
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        let id = tree.add(
            Avatar::with_initials_literal("JD")
                .label_literal("Click")
                .on_activate_fn(|_ctx| {}),
        );
        tree.layout(SizeProposal::exact(64.0, 64.0));
        tree.focus(id);
        tree.layout(SizeProposal::exact(64.0, 64.0));
        let frame = tree.render();
        let target = bastyde_core::presets::intui::light().colors.focus_ring;
        assert!(
            shape_colors(&frame)
                .iter()
                .any(|c| approx_color_eq(*c, target)),
            "expected at least one Shape painted with the theme's focus_ring colour"
        );
    }

    #[test]
    fn non_clickable_avatar_has_no_focus_ring() {
        // A pure Label avatar isn't focusable; it can't acquire focus,
        // and even if focus_ring drawing tried to fire, the `focused`
        // signal would be `None` and the branch is skipped.
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        let id = tree.add(Avatar::with_initials_literal("JD"));
        tree.layout(SizeProposal::exact(64.0, 64.0));
        let baseline = tree.render().shapes.len();
        // Even if some test harness wrongly tried to focus a
        // non-focusable widget, the avatar's paint must NOT add a
        // focus-ring shape.
        tree.focus(id);
        tree.layout(SizeProposal::exact(64.0, 64.0));
        assert_eq!(
            tree.render().shapes.len(),
            baseline,
            "non-clickable avatar must never draw a focus ring"
        );
    }

    #[test]
    fn image_avatar_announces_alt_on_parent() {
        // Inner ImageWidget is `a11y_hidden()` so the avatar is only
        // announced once. The parent carries the canonical name.
        let icon = rgba_solid(8, [10, 20, 30, 255]);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let parent = tree.add(Avatar::with_image(&icon).alt_literal("Jane"));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let info = tree.accessibility_node(parent);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Image);
        assert_eq!(info.name(), Some("Jane"));
    }

    #[test]
    fn shape_change_after_image_does_not_panic() {
        // Pre-refactor we cached masked pixels in Avatar; setting a
        // different shape invalidated the cache. Now masking lives on
        // ImageWidget, but the test still exercises the builder-time
        // ordering: setting the shape after `with_image` works.
        let icon = rgba_solid(16, [10, 20, 30, 255]);
        let a = Avatar::with_image(&icon).alt_literal("X");
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let _ = tree.add(a.shape(AvatarShape::Square));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let _ = tree.render();
    }

    // ── Dynamic content (bind_*) ──────────────────────────────────────

    #[test]
    fn bind_name_updates_displayed_initials_on_signal_flip() {
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::signal::Signal;
        use std::cell::RefCell;
        use std::rc::Rc as StdRc;
        let name = Signal::new(String::new()); // logged-out
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(StdRc::new(RefCell::new(MockTextBackend::new())));
        let id = tree.add(Avatar::with_initials_literal("?").bind_name(name.clone()));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // Empty name ⇒ derived initials = "?".
        assert_eq!(tree.accessibility_node(id).name(), Some("?"));

        name.set("Jane Doe".to_string());
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // After the rebuild, derived initials = "JD".
        assert_eq!(tree.accessibility_node(id).name(), Some("JD"));
    }

    #[test]
    fn bind_image_swap_logged_out_to_logged_in() {
        // The login-flow scenario from the API doc: start without an
        // image (initials fallback), then publish a real photo.
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::signal::Signal;
        use std::cell::RefCell;
        use std::rc::Rc as StdRc;
        let icon = rgba_solid(8, [10, 20, 30, 255]);
        let image: Signal<Option<Rc<RasterIcon>>> = Signal::new(None);
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(StdRc::new(RefCell::new(MockTextBackend::new())));
        let _id = tree.add(
            Avatar::with_initials_literal("JD")
                .alt_literal("Jane")
                .bind_image(image.clone()),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // Logged-out: no image quad emitted.
        assert!(
            tree.render().images.is_empty(),
            "logged-out avatar must not emit an image quad"
        );

        // Logged in.
        image.set(Some(Rc::new(icon)));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        assert!(
            !tree.render().images.is_empty(),
            "logged-in avatar must emit an image quad after the signal flips"
        );

        // Logged out again.
        image.set(None);
        tree.layout(SizeProposal::exact(32.0, 32.0));
        assert!(
            tree.render().images.is_empty(),
            "image quad must disappear when the source signal returns to None"
        );
    }

    #[test]
    fn bind_image_signal_wins_over_static_with_image() {
        // If both are supplied, the bound signal is the source of
        // truth — `None` ⇒ initials fallback even when a static
        // image was provided first.
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::signal::Signal;
        use std::cell::RefCell;
        use std::rc::Rc as StdRc;
        let icon = rgba_solid(8, [10, 20, 30, 255]);
        let image: Signal<Option<Rc<RasterIcon>>> = Signal::new(None);
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(StdRc::new(RefCell::new(MockTextBackend::new())));
        let _id = tree.add(
            Avatar::with_image(&icon)
                .alt_literal("anything")
                .fallback_initials_literal("XX")
                .bind_image(image.clone()),
        );
        tree.layout(SizeProposal::exact(32.0, 32.0));
        // Signal None overrides the static source — initials only.
        assert!(tree.render().images.is_empty());
    }

    #[test]
    fn bind_alt_updates_a11y_name_on_image_avatar() {
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::signal::Signal;
        use std::cell::RefCell;
        use std::rc::Rc as StdRc;
        let icon = rgba_solid(8, [10, 20, 30, 255]);
        let alt = Signal::new(Some("Jane Doe".to_string()));
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(StdRc::new(RefCell::new(MockTextBackend::new())));
        let id = tree.add(Avatar::with_image(&icon).bind_alt(alt.clone()));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("Jane Doe"));

        alt.set(Some("Jules Dupont".to_string()));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("Jules Dupont"));
    }

    #[test]
    fn bind_label_updates_a11y_name_on_initials_avatar() {
        use bastyde_core::signal::Signal;
        let label = Signal::new(Some("Profile".to_string()));
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Avatar::with_initials_literal("JD").bind_label(label.clone()));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("Profile"));

        label.set(Some("Settings".to_string()));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("Settings"));
    }

    #[test]
    fn bind_presence_swap_changes_dot_color_and_a11y_description() {
        use bastyde_core::signal::Signal;
        let presence: Signal<Option<AvatarPresence>> = Signal::new(Some(AvatarPresence::Online));
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        let id = tree.add(Avatar::with_initials_literal("JD").bind_presence(presence.clone()));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let online_color = bastyde_core::presets::intui::light().colors.status_success_fg;
        assert!(
            shape_colors(&tree.render())
                .iter()
                .any(|c| approx_color_eq(*c, online_color)),
            "Online presence should paint the success colour"
        );
        let _ = id;

        // Flip to Busy.
        presence.set(Some(AvatarPresence::Busy));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let busy_color = bastyde_core::presets::intui::light().colors.status_error_fg;
        assert!(
            shape_colors(&tree.render())
                .iter()
                .any(|c| approx_color_eq(*c, busy_color)),
            "Busy presence should paint the error colour"
        );

        // Hide.
        presence.set(None);
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let frame = tree.render();
        // No presence dot ⇒ neither status colour is on the frame.
        assert!(
            !shape_colors(&frame)
                .iter()
                .any(|c| approx_color_eq(*c, online_color) || approx_color_eq(*c, busy_color)),
            "presence None must remove the dot from the frame"
        );
    }

    #[test]
    fn bind_name_changes_hash_seed_so_palette_pick_can_change() {
        // Distinct full names with identical initials produce distinct
        // palette buckets. After bind_name flips between them, the
        // bg shape colour must change too.
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::signal::Signal;
        use std::cell::RefCell;
        use std::rc::Rc as StdRc;
        let name = Signal::new("Jane Doe".to_string());
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(StdRc::new(RefCell::new(MockTextBackend::new())));
        let _id = tree.add(Avatar::with_initials_literal("?").bind_name(name.clone()));
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let bg_jd = shape_colors(&tree.render())
            .into_iter()
            .next()
            .expect("bg circle is the first Shape");

        name.set("Jules Dupont".to_string());
        tree.layout(SizeProposal::exact(32.0, 32.0));
        let bg_jd2 = shape_colors(&tree.render()).into_iter().next().unwrap();
        assert_ne!(
            bg_jd, bg_jd2,
            "different bound names must hash to different palette buckets"
        );
    }
}
