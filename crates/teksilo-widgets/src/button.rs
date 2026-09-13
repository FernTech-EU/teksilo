// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Button — a labelled, activatable action trigger.
//!
//! `Button` is the primary action surface in Teksilo. It renders a text
//! label (optionally with a leading, trailing, top, or bottom icon), fires
//! a closure on click / Space / Enter / AT click, and advertises seven
//! design-language variants via [`ButtonVariant`]. Chrome (fill, border,
//! focus ring, padding) is delegated to the active [`ButtonStyle`]; the
//! default `RecipeButtonStyle` implements the Int UI token ladder.
//!
//! ## When to use
//!
//! - Primary action: `.variant(ButtonVariant::Filled)` — one per context.
//! - Secondary / cancel: default `ButtonVariant::Plain`.
//! - Danger: `ButtonVariant::Destructive` (IntUI maps this to Filled).
//! - Text-only link: `ButtonVariant::Link` / `ButtonVariant::Ghost`.
//!
//! ## Touch and pen
//!
//! The pressed visual is the **framework's**, not the button's own: the router
//! keeps one press record per contact and `Button` mirrors it onto its
//! `InteractionState` (`bind_press_interaction`). That buys four things a
//! `PointerDown` / `PointerUp` pair inside a handler cannot see — a press that
//! slides off its target goes out and comes back on re-entry (WCAG 2.2
//! SC 2.5.2), a press a pan claimant or an ancestor drag wins is cleared with
//! no release to hang the reset on, a cancel clears it, and a press inside a
//! scrollable withholds the visual for 100 ms so a finger that turns out to be
//! scrolling never flashes a highlight. `docs/touch-and-pen.md` §7.1.
//!
//! Activation has always been `on_tap`, so it already lands on the release.
//! After a mouse or pen release the button rests hovered as it always has;
//! after a finger release it rests idle, because a finger sends no
//! hover-leave to correct a hovered state with.
//!
//! The whole family — `IconButton`, `CommandLinkButton`, every `Toolbar`
//! command — shares `build_interaction_handlers` and gets all of this with it.
//!
//! ## Accessibility
//!
//! Announces as `Role::Button` with the resolved label as its AT name.
//! Keyboard: Space / Enter activate; the lone-KeyUp guard prevents spurious
//! re-activation when a shortcut consumes the KeyDown and returns focus here.
//!
//! ```rust
//! # use teksilo_widgets::{Button, ButtonVariant};
//! # use teksilo_i18n::lit;
//! # use teksilo_core::Intent;
//! let _btn = Button::new(lit!("Save"))
//!     .variant(ButtonVariant::Filled)
//!     .on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.save")));
//! ```

use std::rc::Rc;
use teksilo_i18n::lit;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{ButtonStyle, ButtonStyleConfig, SharedButtonStyle};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextRole;

use crate::primitives::icon_widget::IconWidget;
use crate::primitives::{HStack, TextWidget, VStack};

/// Closed enum naming the design-language variants of `Button`. See
/// [`teksilo_core::styles::ButtonVariant`] for the canonical definition.
///
/// Int UI does **not** ship filled red "destructive" buttons —
/// destructive actions in IntelliJ are plain buttons in confirmation
/// dialogs where the title/body carry the warning. The IntUI default
/// `RecipeButtonStyle` collapses `Destructive → Filled`, `Tinted /
/// Outlined → Plain`, and `Link → Ghost` accordingly. Other design
/// languages (Material 3, macOS) honour the variants distinctly.
pub use teksilo_core::styles::ButtonVariant;
use teksilo_i18n::LocalizedString;

/// Internal interaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionState {
    Idle,
    Hovered,
    Pressed,
    Focused,
    Disabled,
}

/// Build the interaction handler set shared by every activatable button
/// (`Button`, `IconButton`, `CommandLinkButton`, and any future sibling).
///
/// Centralizes the parts that MUST stay identical across the family and
/// historically drifted when copy-pasted:
/// - hover/focus state tracking,
/// - keyboard `Space`/`Enter` activation with the **lone-KeyUp guard**
///   (a `KeyUp` with no preceding `KeyDown` — e.g. a shortcut consumed
///   the `KeyDown` and focus returned here — must NOT activate),
/// - the AT `Click` action.
///
/// `on_activate` runs on tap, keyboard activation, and AT click. Callers
/// bundle their command action (and any extra side effect, e.g.
/// `IconButton`'s toggle flip) into this single closure so the guard
/// gates all activation paths uniformly. `focusable` is the node's
/// focusability (`Button` is always focusable; `IconButton` exposes it).
/// A layout-transparent wrapper that lifts its child's **hit** area to the
/// density's target size at every density, for direct pointers only.
///
/// The residue the other three mechanisms cannot serve: a control that is
/// under 24 dp, is composed out of primitives rather than being its own
/// `Widget` (so it has no `hit_outset` of its own to implement), and sits
/// inside something that takes presses (so the miss-only slop pass, which only
/// re-attributes to a candidate strictly closer than the bubble owner, can
/// never reach it). The text field's 16 dp clear affordance is the case this
/// was written for.
///
/// Distinct from [`TouchTarget`](crate::primitives::TouchTarget), which is the
/// wrapper that *moves* things: it reserves layout space and is deliberately
/// the identity below `TargetDensity::Touch`. This one never moves anything
/// and is live at every density, because `min_target_conformance` is 24 dp at
/// every density and is never scaled. The two are candidates for merging into
/// one wrapper with two modes; they are separate here because `TouchTarget` is
/// not this package's file.
pub(crate) struct HitTarget {
    child: Option<WidgetId>,
    size: Option<teksilo_canvas::Size>,
    active: teksilo_core::signal::Prop<bool>,
    bounds: std::cell::Cell<teksilo_canvas::Size>,
}

impl HitTarget {
    pub(crate) fn new() -> Self {
        Self {
            child: None,
            size: None,
            active: teksilo_core::signal::Prop::Static(true),
            bounds: std::cell::Cell::new(teksilo_canvas::Size::ZERO),
        }
    }

    /// Pin the slot's own size instead of forwarding the child's.
    ///
    /// For the shape this exists to serve: the slot has to keep reserving its
    /// room while the affordance inside it is hidden, so the row does not jump
    /// when the affordance appears. Without it a dormant child would collapse
    /// the wrapper to nothing — and, being the wrapper the ring resolves to,
    /// it would take the outset with it.
    pub(crate) fn fixed(mut self, width: f32, height: f32) -> Self {
        self.size = Some(teksilo_canvas::Size::new(width, height));
        self
    }

    /// Whether the wrapper currently claims its widened target.
    ///
    /// `false` withdraws the outset entirely, because a widened node that then
    /// refuses the press is a hole punched in whatever is behind it — the same
    /// rule the splitter handle and the twist arrow apply. Reactive, so a
    /// clear affordance that comes and goes with the field's contents does not
    /// need a rebuild.
    pub(crate) fn active(mut self, active: impl Into<teksilo_core::signal::Prop<bool>>) -> Self {
        self.active = active.into();
        self
    }

    pub(crate) fn child_id(mut self, id: WidgetId) -> Self {
        self.child = Some(id);
        self
    }
}

impl std::fmt::Debug for HitTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitTarget").finish()
    }
}

impl Widget for HitTarget {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        if let Some(size) = self.size {
            return size.into();
        }
        // Fully transparent: the child's whole response, not just its size, so
        // a shrinkable or flexible child stays so through the wrapper.
        self.child
            .and_then(|id| ctx.child_layout_response(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        self.bounds.set(bounds.size());
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn hit_outset(
        &self,
        kind: teksilo_tokens::PointerKind,
        tokens: &teksilo_tokens::InputTokens,
    ) -> teksilo_canvas::EdgeInsets {
        if !self.active.get() {
            return teksilo_canvas::EdgeInsets::ZERO;
        }
        target_outset(self.bounds.get(), kind, tokens)
    }
}

/// The per-edge hit outset that lifts a control painted at `visual` up to the
/// density's target size, for the pointer that is asking.
///
/// The controls sweep's answer to a control that is genuinely smaller than 24
/// dp and cannot grow: a 12 dp twist arrow inside a tree row, a 16 dp clear
/// button inside a text field. The **density rule** forbids routing such a
/// dimension through [`dp`](teksilo_core::styles::density::dp) at layout time
/// — that would raise its paint at Compact and break the programme's
/// Compact-is-unchanged invariant — so the shortfall is made up between the
/// pointer and the arena instead, which is what [`Widget::hit_outset`] is for
/// (`docs/density-and-targets.md`).
///
/// Zero for a precise pointer, always: a mouse hot-spot is exact and a widened
/// node would steal clicks from whatever it overlaps. Zero on an axis that is
/// already at or above the target, so a control that only falls short on one
/// axis grows only on that one.
///
/// Two outset controls close enough for their rings to overlap both claim the
/// space between them; the arena resolves that by distance to the uninflated
/// rect, so the boundary lands halfway, which is the answer a user aiming
/// between them expects.
///
/// [`Widget::hit_outset`]: teksilo_core::widget::Widget::hit_outset
pub(crate) fn target_outset(
    visual: teksilo_canvas::Size,
    kind: teksilo_tokens::PointerKind,
    tokens: &teksilo_tokens::InputTokens,
) -> teksilo_canvas::EdgeInsets {
    use teksilo_core::styles::density::dp;
    use teksilo_tokens::TargetRole;

    if !kind.is_direct() {
        return teksilo_canvas::EdgeInsets::ZERO;
    }
    outset_to(visual, |extent| dp(extent, TargetRole::Target, tokens))
}

/// Half the shortfall between each of `visual`'s extents and the floor `to`
/// puts under it, per edge, never negative.
///
/// Split out from [`target_outset`] so the shortfall arithmetic is stated once
/// and a widget that needs a different floor reuses it rather than re-deriving
/// it. A
/// non-positive or non-finite extent grows by nothing: there is no meaningful
/// centre to grow around.
fn outset_to(
    visual: teksilo_canvas::Size,
    floor: impl Fn(f32) -> f32,
) -> teksilo_canvas::EdgeInsets {
    let grow = |extent: f32| {
        if extent > 0.0 && extent.is_finite() {
            ((floor(extent) - extent) * 0.5).max(0.0)
        } else {
            0.0
        }
    };
    teksilo_canvas::EdgeInsets::symmetric(grow(visual.width), grow(visual.height))
}

/// Drive a button-family control's `Pressed` state from the framework press.
///
/// The family used to keep this itself: `PointerDown` set `Pressed`,
/// `PointerUp` put it back. That is right for a mouse and wrong for a finger
/// in four ways a handler cannot see — a press that slides off its target, a
/// press that slides back on, a press a pan claimant takes away with no
/// release to hang the reset on, and a press that must not light up at all
/// until the pan has been ruled out. `docs/touch-and-pen.md` §7.1 has the
/// rules; the router keeps the state and this mirrors it onto the family's
/// five-state `interaction` signal.
///
/// Only the `Pressed` transitions move: this writes `Pressed` when the
/// framework press lights — and at build time when it is already lit — and,
/// when it goes out, the resting state below. The `Pressed` guard on that
/// second branch is what keeps it from overwriting a resting state the
/// `on_tap` above has already chosen for the release. Hover, focus and the
/// keyboard `Space`/`Enter` machine set their own states, and the framework
/// press never moves for a key — it is a *pointer's* record — so nothing here
/// raises `Pressed` on a key's behalf. It can still clear one, because both
/// write the same signal: a pointer press that ends while `Space` is held
/// finds the signal on `Pressed` and rests it, which also disarms the
/// lone-`KeyUp` guard in `on_key`. Reaching that takes a held key and a
/// pointer press on one control.
///
/// Ending a press with no activation — a slide-off, a cancel, an ancestor
/// drag winning the arbitration — rests the control on the `hovered` cell
/// beside the signal, and the two pointer kinds part ways there.
///
/// **A contact never writes that cell.** The router refuses a contact the
/// hover-owner role outright, so `on_hover` never fires for one, and the
/// `on_tap` write above sits behind `pointer_kind().hovers()`. A finger
/// therefore leaves the cell exactly as it found it: on a touch-only device
/// `false`, so a *revoked* contact — the pan claimant's — leaves the control
/// `Idle`, which is what stops a pan-stolen tap staying lit with nothing
/// touching it. Where a mouse is resting on the same control the cell is that
/// mouse's, and the control rests `Hovered` on the strength of a pointer that
/// really is there.
///
/// **A mouse keeps whatever the cell held when it pressed.** A mouse that
/// pointed at the control before pressing it left the cell `true`, and nothing
/// clears it while the press lasts: the press holds the pointer capture, so
/// moves route straight to the owner and no `PointerLeave` — and so no
/// `on_hover(false)` — is synthesised even while the pointer is off the
/// control. A mouse press that
/// ends without activating therefore rests `Hovered` whether it was revoked
/// under the pointer or had slid off, because the cell records where the
/// pointer was when it pressed rather than where it is now. The `Idle` branch
/// is reached under a mouse by a press that never had the hover to begin
/// with — a `PointerDown` with no `PointerMove` over the control before it.
pub(crate) fn bind_press_interaction(
    ctx: &mut BuildContext,
    interaction: Signal<InteractionState>,
    hovered: Rc<std::cell::Cell<bool>>,
) {
    let pressed = ctx.pressed_signal();
    // Seed from the live state rather than from `false`: a rebuild that
    // happens *during* a press must not blink the visual off. Rare, because
    // `process_pending_rebuilds` defers a rebuild aimed at the widget holding
    // the capture — and a press owner is the capture owner — but a live drag
    // session lifts that deferral for the whole tree, so a second contact
    // dragging elsewhere is enough to land one here.
    if pressed.get() {
        interaction.set(InteractionState::Pressed);
    }
    ctx.effect(&pressed, move |showing| {
        if *showing {
            interaction.set(InteractionState::Pressed);
        } else if interaction.get() == InteractionState::Pressed {
            interaction.set(if hovered.get() {
                InteractionState::Hovered
            } else {
                InteractionState::Idle
            });
        }
    });
}

pub(crate) fn build_interaction_handlers(
    ctx: &mut BuildContext,
    interaction: Signal<InteractionState>,
    on_activate: Rc<dyn Fn(&mut EventContext)>,
    focusable: bool,
) -> HandlerSet {
    let act_tap = on_activate.clone();
    let act_key = on_activate.clone();
    let act_access = on_activate;
    // Whether the pointer is currently over the control, kept beside the
    // interaction signal so the press binding can restore the *right* resting
    // state when a press ends without an activation. `interaction` alone
    // cannot answer it: while the control is `Pressed` the hover truth has
    // nowhere to live.
    let hovered = Rc::new(std::cell::Cell::new(false));
    bind_press_interaction(ctx, interaction.clone(), hovered.clone());
    HandlerSet::new()
        .on_tap({
            let interaction = interaction.clone();
            let hovered = hovered.clone();
            move |_pos: &teksilo_core::TapEvent, ctx: &mut EventContext| {
                act_tap(ctx);
                // Where the control rests after an activation. A mouse or a
                // pen is still over it, so it rests hovered exactly as it
                // always has; a finger is gone the instant it lifts and never
                // sent a hover-leave to correct a `Hovered` state with, so it
                // rests idle.
                interaction.set(if ctx.pointer_kind().hovers() {
                    hovered.set(true);
                    InteractionState::Hovered
                } else {
                    InteractionState::Idle
                });
            }
        })
        .on_hover({
            let interaction = interaction.clone();
            let hovered = hovered.clone();
            move |entered: bool, _ctx: &mut EventContext| {
                hovered.set(entered);
                interaction.set(if entered {
                    InteractionState::Hovered
                } else {
                    InteractionState::Idle
                });
            }
        })
        .on_key({
            let interaction = interaction.clone();
            move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::Space | Key::Enter,
                        ..
                    } => {
                        interaction.set(InteractionState::Pressed);
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyUp {
                        key: Key::Space | Key::Enter,
                        ..
                    } => {
                        // Lone-KeyUp guard: only activate if we saw the
                        // matching KeyDown (state is Pressed).
                        if interaction.get() != InteractionState::Pressed {
                            return EventResponse::Ignored;
                        }
                        act_key(ctx);
                        interaction.set(InteractionState::Focused);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            }
        })
        .on_focus({
            let interaction = interaction.clone();
            move |gained: bool, _ctx: &mut EventContext| {
                if gained {
                    if interaction.get() == InteractionState::Idle {
                        interaction.set(InteractionState::Focused);
                    }
                } else {
                    interaction.set(InteractionState::Idle);
                }
            }
        })
        .on_access_action(
            move |action: teksilo_core::accesskit::Action,
                  ctx: &mut EventContext|
                  -> EventResponse {
                if action == teksilo_core::accesskit::Action::Click {
                    act_access(ctx);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            },
        )
        .focusable(focusable)
        .cursor(CursorIcon::Pointer)
}

/// Test-only helpers for driving a control with a synthetic contact.
///
/// Lives here because the button family is where the framework press first
/// lands; every other control in the controls sweep reaches it as
/// `crate::button::press_test_support`.
#[cfg(test)]
pub(crate) mod press_test_support {
    use teksilo_canvas::Point;
    use teksilo_core::event::Modifiers;
    use teksilo_core::pointer::{
        BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
        PointerSample,
    };
    use teksilo_core::widget_tree::WidgetTree;

    /// A brand-new contact id. Every touch press mints one — winit reuses
    /// `Touch::id`, the allocator does not.
    pub(crate) fn finger() -> PointerId {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        PointerIdAllocator::global().begin(
            BackendDeviceKey::new(0x0B24),
            NEXT.fetch_add(1, Ordering::Relaxed),
        )
    }

    /// One touch sample for `id` at `at`, stamped `ms` into the tree epoch.
    pub(crate) fn touch(id: PointerId, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
        PointerSample {
            pointer: PointerInfo::touch(id, EventTime::from_millis(ms)),
            phase,
            position: at,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// Press, then release, at the same point: the whole touch tap.
    pub(crate) fn touch_tap(tree: &mut WidgetTree, at: Point) {
        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 30));
    }
}

/// Where an optional icon is placed relative to the button label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconLocation {
    /// No icon (default).
    #[default]
    None,
    /// Icon only, no label.
    IconOnly,
    /// Icon to the left of the label (default).
    Leading,
    /// Icon to the right of the label.
    Trailing,
    /// Icon above the label.
    Top,
    /// Icon below the label.
    Bottom,
}

/// Type-erased activation closure. Stored as `Box<dyn Fn>` so the
/// same button type works for any handler — typed intent send,
/// direct side effect, window mutation, etc.
type CommandFactory = Box<dyn Fn(&mut EventContext)>;

/// A labelled action trigger; use [`Button::new`] and chain builder methods.
pub struct Button {
    /// Button label as a `Prop<String>`. `new(tr!(...))` stores a
    /// `Prop::Bound` (locale-reactive) when an i18n manager is installed,
    /// falling back to `Prop::Static` for `lit!(...)` or no manager;
    /// `label(signal)` overrides with a caller-supplied source. Either
    /// way the inner `TextWidget` re-renders reactively without rebuilding
    /// the Button. The accessibility node's `set_name` reads the current
    /// value via `Prop::get()`, keeping AT in sync with bound updates.
    label: teksilo_core::signal::Prop<String>,
    /// Tier-1 design-language variant hint (Filled, Plain, Ghost, …).
    /// The active [`ButtonStyle`] decides what to do with it.
    variant: ButtonVariant,
    /// Optional per-call override for the active [`ButtonStyle`]. When
    /// `None`, falls through to the theme slot or the
    /// built-in [`crate::styles::RecipeButtonStyle`] default.
    style_override: Option<SharedButtonStyle>,
    action: Option<CommandFactory>,
    /// Enabled state, static or reactive. Forwarded into the arena via
    /// `ctx.enabled_when(self_id, self.enabled.clone())` at build time;
    /// not kept as a runtime snapshot. After `build()` the arena's
    /// `enabled_state` is the single source of truth — leaves resolve
    /// colors via `PaintContext::effective_enabled`, events are gated
    /// by `arena.is_enabled()`, the a11y walker reads it for
    /// `set_disabled()`.
    enabled: Prop<bool>,
    icon: Option<IconWidget>,
    icon_location: IconLocation,
    /// Leave the icon's own colour alone instead of tinting it to the label's.
    /// See [`Button::icon_keeps_color`].
    icon_keeps_color: bool,
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    /// Mutually exclusive with `tooltip_text` and `composite_tooltip_content`
    /// — every tooltip setter clears the other two so last-call wins.
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body. Hosts an arbitrary widget
    /// tree (charts, grids, conditional rows). Mutually exclusive
    /// with `tooltip_text` and `rich_tooltip_source` per the
    /// last-call-wins matrix.
    composite_tooltip_content: Option<Box<dyn teksilo_core::widget::Widget>>,
    /// Optional `has_popup` hint used when this button acts as a
    /// disclosure trigger for a popup (menu, dialog, listbox, etc.).
    /// Surfaced via `set_has_popup` in `accessibility()`.
    has_popup: Option<teksilo_core::accesskit::HasPopup>,
    /// Arbitrary widget rendered to the leading edge of the button's
    /// content (left in LTR, right in RTL). Composes with `.icon(...)`:
    /// the order is `[leading_slot, icon+label, trailing_slot]`. Slot
    /// widgets paint and report a11y on their own — Button does not
    /// retint them and does not auto-suppress their AT roles. Apps
    /// whose slot widgets would otherwise pollute the AT tree
    /// (e.g. ColorSwatch with `Role::ColorWell`) should pass
    /// `widget.access_hidden(true)` so the Button's
    /// `Role::Button` stays the single declared role.
    leading: Option<Box<dyn Widget>>,
    /// Same shape as `leading`, rendered to the trailing edge.
    trailing: Option<Box<dyn Widget>>,
    /// Optional signal reporting whether the button's popup is
    /// currently visible. Surfaced via `set_expanded` in
    /// `accessibility()`. Used alongside `has_popup` for the
    /// standard ARIA disclosure pattern.
    expanded_signal: Option<Prop<bool>>,
    /// Optional caller-supplied interaction signal. When set, `build()`
    /// uses this signal instead of allocating its own — letting an
    /// external widget (e.g. `PopoverButton`'s disclosure caret)
    /// observe hover / press / focus / disabled state and match the
    /// label's color exactly. See [`Button::share_interaction`].
    shared_interaction: Option<Signal<InteractionState>>,
    /// Optional caller-supplied label/icon color override. When `Some`,
    /// both the label text and any icon are bound to this `ColorProp`
    /// regardless of `style` / interaction state — the auto-derived
    /// cascade is replaced. Used by chrome that has to match a host's
    /// enforced text role (e.g. tab-bar overflow dropdown trigger
    /// inheriting `idle_text_role`). See [`Button::text_role`].
    text_role_override: Option<teksilo_core::color_prop::ColorProp>,
    /// Optional per-call override for the label's text style (font, size,
    /// weight). When `Some`, applied to the inner label `TextWidget` via
    /// its `.style(...)`; when `None`, the `TextWidget` default is used.
    /// Accepts a `TextStyleRole`, a `TextStyle`, or a `Signal` of either
    /// (anything `Into<TextStyleProp>`). See [`Button::text_style`].
    label_style: Option<teksilo_core::color_prop::TextStyleProp>,
    /// Interaction state signal — set during build().
    interaction: Signal<InteractionState>,
    /// Root child ID — set during build().
    root_child_id: Option<WidgetId>,
}

impl Button {
    /// Construct a button from a `LocalizedString` label. The label may
    /// come from `tr!(...)` (translated) or `lit!(...)`
    /// (explicit non-translated). When an `I18nManager` is installed, a
    /// `tr!(...)` label becomes a `Prop::Bound` that observes the locale
    /// version signal, so the inner `TextWidget` re-renders on a locale
    /// switch without rebuilding the Button — matching `TextWidget::new`.
    /// `lit!(...)` and the no-manager case resolve to a static `String`.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            // `Prop::from(LocalizedString)` yields `Prop::Bound` (reactive)
            // when a manager is installed, `Prop::Static` otherwise — the
            // same conversion `TextWidget::new` uses. A locale change then
            // updates the label live; without this it stayed frozen because
            // `set_locale` marks the tree dirty (relayout/repaint) but does
            // NOT rebuild composites.
            label: teksilo_core::signal::Prop::from(ls),
            // Int UI default is a Plain (non-primary) button; the caller
            // opts into `ButtonVariant::Filled` for the one primary action.
            variant: ButtonVariant::Plain,
            style_override: None,
            action: None,
            enabled: Prop::Static(true),
            icon: None,
            icon_location: IconLocation::None,
            icon_keeps_color: false,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            has_popup: None,
            expanded_signal: None,
            shared_interaction: None,
            text_role_override: None,
            label_style: None,
            leading: None,
            trailing: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    /// Returns the configured visual variant. Used by wrappers like
    /// [`PopoverButton`](crate::popover_widget::PopoverButton) that
    /// derive their own chrome colors from the same recipe-resolution
    /// path the inner Button uses.
    pub fn current_variant(&self) -> ButtonVariant {
        self.variant
    }

    /// Bind the button's internal interaction state to a caller-owned
    /// `Signal<InteractionState>` instead of letting `build()` allocate
    /// its own. Used by wrapper widgets like
    /// [`PopoverButton`](crate::popover_widget::PopoverButton) whose
    /// disclosure caret needs to match the label's color across hover
    /// / press / focus / disabled states.
    ///
    /// The provided signal is reset to `Disabled` when `enabled == false`
    /// during `build()` so the shared signal honors the button's
    /// enabled state without the caller having to seed it.
    pub fn share_interaction(mut self, signal: Signal<InteractionState>) -> Self {
        self.shared_interaction = Some(signal);
        self
    }

    /// Set the Tier-1 design-language variant. The active
    /// [`ButtonStyle`] decides whether to honour or remap it (the IntUI
    /// default `RecipeButtonStyle` collapses Destructive → Filled,
    /// Tinted/Outlined → Plain, Link → Ghost).
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Override the active [`ButtonStyle`] for this widget instance
    /// only. Useful for one-off custom-painted buttons (glassmorphism
    /// CTA, Material-3 ripple, etc.) without forking the Button.
    pub fn style(mut self, style: impl ButtonStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Bind the button's label to a reactive source — replaces the
    /// static label captured at `new(...)`. Accepts any
    /// `impl Into<Prop<String>>`: a `Signal<String>` for live
    /// updates, or a plain `String` (which is the same as constructing
    /// the button with that string). Mirrors
    /// [`TextWidget::text`](crate::primitives::TextWidget::text).
    /// The inner label `TextWidget` is built with the bound prop, so
    /// the visible text refreshes without rebuilding the Button. The
    /// AT node's `set_name` reads the current value via `Prop::get`.
    ///
    /// Translation note: derive the signal with
    /// `state.map(|s| tr!(status_label(value = s)).resolve_now())` for translated
    /// reactive labels — Button only sees the resolved `String`.
    pub fn label(mut self, label: impl Into<teksilo_core::signal::Prop<String>>) -> Self {
        self.label = label.into();
        self
    }

    /// Closure invoked on activation. Use `ctx.send_intent(...)` to
    /// route activation through the Action/Intent system, or inline
    /// the behavior directly.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Box::new(f));
        self
    }

    /// Whether an activation closure has been attached. Used by wrappers
    /// (e.g. `PopoverWidget`) that overwrite the activate slot, so they
    /// can warn when a caller-set handler is about to be discarded.
    pub(crate) fn has_activate_handler(&self) -> bool {
        self.action.is_some()
    }

    /// Attach a tooltip that appears after a hover delay.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip registry.
    /// The `key` is looked up via
    /// [`TooltipRegistry`](crate::tooltip::TooltipRegistry) at build
    /// time; the resolved body text supports inline markup
    /// (`[label](url)`, `*italic*`, `**bold**`) and the entry's
    /// shortcut / long-form "more" fields are rendered automatically.
    ///
    /// Overrides any previously set plain `.tooltip(...)` text.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by inline
    /// [`TooltipContent`](crate::tooltip::TooltipContent) — for
    /// one-off tooltips that aren't worth registering in the central
    /// catalog. Overrides any previously set plain `.tooltip(...)`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — third tier, hosting an arbitrary
    /// widget tree (Crusader Kings 3 style: tabbed sections, charts,
    /// progress bars, conditional rows). Promotes to a focusable
    /// `Role::Dialog` after the user dwells for the standard
    /// promotion threshold. Overrides any plain or rich tooltip
    /// previously set on this button.
    pub fn composite_tooltip(
        mut self,
        content: impl teksilo_core::widget::Widget + 'static,
    ) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Boxed variant of [`composite_tooltip`](Self::composite_tooltip).
    /// Used by `Clone` value types (e.g. `ToolbarAction`) that store a
    /// composite-body factory `Rc<dyn Fn() -> Box<dyn Widget>>` and forward
    /// the produced box through at build time.
    pub(crate) fn composite_tooltip_boxed(
        mut self,
        content: Box<dyn teksilo_core::widget::Widget>,
    ) -> Self {
        self.composite_tooltip_content = Some(content);
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Set the enabled state, statically or reactively. Disabled buttons
    /// ignore input and dim their content (the framework's
    /// `PaintContext::effective_enabled` propagates through to the
    /// label/icon leaves). Forwarded into the arena via
    /// `ctx.enabled_when(self_id, self.enabled.clone())` at build time —
    /// a bound signal updates live as it changes.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Override the label and icon's tint with a static `ColorProp`.
    /// When set, the button ignores its `style` and the auto-derived
    /// idle/hover/press text-role cascade — both the label text and
    /// any icon are bound directly to this prop instead. Use for chrome
    /// whose host enforces a single text role across all of its
    /// sub-widgets (e.g. tab-bar overflow-dropdown triggers that must
    /// match the strip's `idle_text_role` regardless of hover state).
    /// Accepts `Color`, `TextRole`, `Signal<Color>`, or `Signal<TextRole>`.
    pub fn text_role(mut self, role: impl Into<teksilo_core::color_prop::ColorProp>) -> Self {
        self.text_role_override = Some(role.into());
        self
    }

    /// Override the label's text style (font, size, weight). By default the
    /// label uses the inner `TextWidget`'s default style; pass a
    /// `TextStyleRole` (e.g. `TextStyleRole::BodyBold`), a `TextStyle`, or a
    /// `Signal` of either to change it — e.g. to make the label bold.
    /// Orthogonal to [`Button::text_role`], which only sets the color.
    pub fn text_style(mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>) -> Self {
        self.label_style = Some(style.into());
        self
    }

    /// Add an icon to the button at the specified location.
    pub fn icon(mut self, icon: IconWidget, location: IconLocation) -> Self {
        self.icon = Some(icon);
        self.icon_location = location;
        self
    }

    /// Keep the icon's own colour instead of tinting it to the label's.
    ///
    /// The mirror of [`MenuItem::icon_keeps_color`](crate::menu_item::MenuItem::icon_keeps_color),
    /// and it exists for the same reason: an icon whose colour *is* the information.
    /// A filter chip carrying a user-chosen tag colour, a legend swatch, a status
    /// disc — tinting those to the label's foreground destroys the one thing they
    /// carry, while tinting is exactly right for a glyph that merely repeats the
    /// label.
    ///
    /// Two consequences worth knowing, both inherited from
    /// [`ColorProp`](teksilo_core::color_prop::ColorProp)'s own rules rather than
    /// special-cased here:
    ///
    /// * The colour must clear contrast against **every** fill the button takes —
    ///   an accent-filled selected state as well as the resting surface.
    /// * A literal colour **does not dim when the button is disabled**. An icon
    ///   that should dim wants a role instead, and then it does not need this.
    pub fn icon_keeps_color(mut self) -> Self {
        self.icon_keeps_color = true;
        self
    }

    /// Declare that this button is a disclosure trigger for a
    /// popup (menu, dialog, listbox, tree, grid). Surfaced via
    /// `set_has_popup` in the a11y node so screen readers announce
    /// it as leading into the named popup kind.
    pub fn has_popup(mut self, kind: teksilo_core::accesskit::HasPopup) -> Self {
        self.has_popup = Some(kind);
        self
    }

    /// Bind a signal reporting whether this button's popup is
    /// currently visible. The Popover / Dialog wrapper owns the
    /// signal and flips it on show / dismiss; Button reads it in
    /// `accessibility()` to publish `set_expanded`. Only
    /// meaningful alongside `.has_popup(...)`.
    pub fn expanded_when(mut self, signal: impl Into<Prop<bool>>) -> Self {
        self.expanded_signal = Some(signal.into());
        self
    }

    /// Insert a widget at the leading edge of the button's content
    /// (left in LTR, right in RTL). Composes with `.icon(...)`: the
    /// final order is `[leading_slot, icon+label, trailing_slot]`,
    /// separated by `btn::BUTTON_ICON_LABEL_GAP`. Single-slot —
    /// calling `.leading(...)` again replaces the previous slot.
    /// Stack multiple widgets with an explicit `HStack`.
    ///
    /// The slot widget paints itself and emits its own a11y. Button
    /// does **not** retint it (so e.g. a `ColorSwatch` keeps its own
    /// color through every interaction state). If the slot widget
    /// declares an AT role of its own — `ColorSwatch` is the canonical
    /// case (`Role::ColorWell`) — pass `widget.access_hidden(true)`
    /// so the trigger reads as a single Button node instead of a
    /// Button containing a redundant ColorWell child.
    pub fn leading(mut self, widget: impl Widget + 'static) -> Self {
        self.leading = Some(Box::new(widget));
        self
    }

    /// Same as [`leading`](Self::leading) but at the trailing edge
    /// (right in LTR, left in RTL). Common uses: chevron-down hint
    /// on disclosure triggers, clear-X on search fields, status
    /// badges on segmented control segments.
    pub fn trailing(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing = Some(Box::new(widget));
        self
    }

    /// Construct the label `TextWidget` used inside the button's
    /// content layout. Always routes through `text(prop)` —
    /// `Prop::Static` and `Prop::Bound` are both handled uniformly
    /// by the TextWidget. `new(lit!(""))` seeds the placeholder
    /// initial text; `text` immediately overwrites it with the
    /// prop's current value (and tracks updates for `Prop::Bound`).
    fn make_label_text(&self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> TextWidget {
        let mut text = TextWidget::new(lit!(""))
            .text(self.label.clone())
            .color(color)
            .single_line()
            .a11y_hidden();
        if let Some(style) = &self.label_style {
            text = text.style(style.clone());
        }
        text
    }

    /// Take the configured icon, size it, and bind its tint to `color`.
    /// Shared by every icon-bearing `IconLocation` arm so the size /
    /// color wiring lives in one place.
    ///
    /// A non-`None` `icon_location` with no icon set is a programming
    /// error — `.icon(...)` was never called. In debug builds the
    /// `debug_assert!` surfaces the mistake (mirroring how `Checkbox`
    /// asserts a missing accessible label); release falls back to an
    /// empty path so the button still lays out instead of panicking.
    fn make_icon(&mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> IconWidget {
        use crate::styles::recipe_button_style as btn;
        debug_assert!(
            self.icon.is_some(),
            "Button: icon_location is {:?} but no icon was set via .icon(...)",
            self.icon_location,
        );
        let icon = self
            .icon
            .take()
            .unwrap_or_else(|| {
                IconWidget::from_path(teksilo_canvas::Path::new(), btn::BUTTON_ICON_SIZE)
            })
            .icon_size(btn::BUTTON_ICON_SIZE);
        if self.icon_keeps_color {
            icon
        } else {
            icon.color(color)
        }
    }

    /// Assemble the V2 attached-handler set (tap / hover / key / focus /
    /// access-action) wired to `interaction`. Takes `self.action`. The
    /// framework gates pointer / key / access events on
    /// `arena.is_enabled(self_id)` before dispatch and the focus walker
    /// skips disabled subtrees, so none of these closures need a
    /// build-time enabled snapshot — that duality was removed in the
    /// single-sourced-enabled refactor.
    fn build_handler_set(
        &mut self,
        ctx: &mut BuildContext,
        interaction: Signal<InteractionState>,
    ) -> HandlerSet {
        // Bundle the optional command action into the unified
        // `on_activate` closure consumed by the shared family helper.
        let action: Rc<Option<CommandFactory>> = Rc::new(self.action.take());
        let on_activate: Rc<dyn Fn(&mut EventContext)> = Rc::new(move |ctx: &mut EventContext| {
            if let Some(ref action) = *action {
                action(ctx);
            }
        });
        build_interaction_handlers(ctx, interaction, on_activate, true)
    }
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("label", &self.label.get())
            .field("variant", &self.variant)
            .field("enabled", &self.enabled.get())
            .finish()
    }
}

// --- Label / icon color resolution ---
//
// The active `ButtonStyle` owns chrome (background fill, border, focus
// ring) but the inner content (label + icon) belongs to the Button
// itself, so it picks the text role. The mapping is intentionally
// minimal: `OnAccent` for variants that paint an accent fill, `Primary`
// for everything else, `Disabled` when the button is disabled. Custom
// `ButtonStyle` impls that paint a different background can request
// the Button to use a specific text role via `Button::text_role(...)`.

pub(crate) fn resolve_text_role(variant: ButtonVariant, _state: InteractionState) -> TextRole {
    // Disabled substitution happens at the leaf paint via
    // `ColorProp::resolve(theme, ctx.effective_enabled)` — see
    // `crates/teksilo-core/src/color_prop.rs`. The composite no
    // longer carries `InteractionState::Disabled`; the framework's
    // arena enabled-state drives the dim, and the leaves convert it
    // into `TextRole::Disabled` at paint time.
    match variant {
        ButtonVariant::Filled | ButtonVariant::Destructive => TextRole::OnAccent,
        ButtonVariant::Tinted
        | ButtonVariant::Outlined
        | ButtonVariant::Plain
        | ButtonVariant::Ghost => TextRole::Primary,
        ButtonVariant::Link => TextRole::Link,
    }
}

impl teksilo_core::widget::Widget for Button {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Layout constants for the inner content (icon size,
        // icon-label gap) come from the button recipe. The chrome
        // (padding, corner radius, fill, border) lives on the active
        // `ButtonStyle` impl.
        use crate::styles::recipe_button_style as btn;
        let variant = self.variant;
        let self_id = ctx.self_id();

        // Forward the enabled state into the arena. After this point the
        // arena is the single source of truth — events, focus, a11y, and
        // the leaves' role-resolution all consult
        // `arena.is_enabled(self_id)` / `PaintContext::effective_enabled`.
        // The interaction signal no longer carries Disabled: that was
        // the snapshot duality the architecture refactor removed.
        ctx.enabled_when(self_id, self.enabled.clone());

        // Reactive view of "is this widget effectively enabled?".
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Create interaction signal — caller-supplied via
        // `share_interaction` when set (so a wrapping widget's chrome
        // can mirror the label's color), otherwise allocated locally.
        // Seeded to Idle; the arena's enabled-state is consulted
        // separately via `effective_enabled`.
        let interaction = match self.shared_interaction.take() {
            Some(shared) => shared,
            None => ctx.signal(InteractionState::Idle),
        };
        self.interaction = interaction.clone();

        // If an `expanded_signal` was wired up (disclosure
        // pattern — see `.has_popup()` / `.expanded_when()`),
        // register it with the framework so changes trigger a
        // repaint/a11y refresh on this button. Without the
        // binding registration, the signal updates but the
        // widget's `accessibility()` output won't be re-queried.
        if let Some(ref expanded_signal) = self.expanded_signal {
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            expanded_signal.register_if_bound(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // If `label(signal)` was used, register the prop on the
        // Button itself at AccessibilityOnly so `set_name` re-runs
        // when the signal changes. The inner `TextWidget` already
        // re-renders via its own `text` plumbing — this binding
        // is purely for the AT name.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.label.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );

        // Resolve the active `ButtonStyle` (per-call override > theme
        // slot > IntUI default). Both the label color (immediately below)
        // and the chrome (`make_body`, further down) consult it. The
        // lookup reads only `self.style_override` + `ctx.theme()`, so
        // resolving it here instead of just before `make_body` changes
        // nothing for existing styles.
        let style: SharedButtonStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.button.clone())
            .unwrap_or_else(|| {
                Rc::new(crate::styles::RecipeButtonStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });

        // Label/icon color: a caller-supplied override wins over the
        // auto cascade. The override replaces ALL states (idle / hover /
        // press / focus / disabled) — chrome that uses this opts out of
        // interaction-driven color feedback in exchange for matching a
        // host's enforced text role. Both label and icon read this same
        // prop, so a one-line override re-tints the whole button.
        //
        // Chrome (background fill, border, focus ring) is no longer
        // resolved here — the active `ButtonStyle` owns it via
        // `make_body(cfg, ctx)` below. This widget only resolves the
        // CONTENT color (label + icon) since that's part of the inner
        // subtree we hand to the style as `cfg.label`. The active style
        // may also redirect the content role (`label_text_role`) — e.g.
        // Material 3 paints text/outlined buttons in the accent color.
        let text_role: teksilo_core::color_prop::ColorProp =
            if let Some(ref over) = self.text_role_override {
                over.clone()
            } else if let Some(role) = style.label_text_role(variant) {
                role.into()
            } else {
                interaction
                    .map(move |s| resolve_text_role(variant, *s))
                    .into()
            };

        // Build the content (icon + label) based on icon_location. The
        // four directional arms (Leading/Trailing/Top/Bottom) share one
        // body: build the icon + label, then assemble them into an
        // HStack or VStack in icon-first / text-first order. Icon size /
        // color wiring is centralized in `make_icon`.
        let icon_location = self.icon_location;
        let content_id = match icon_location {
            IconLocation::None => ctx.add(self.make_label_text(text_role)),
            IconLocation::IconOnly => {
                let icon = self.make_icon(text_role);
                ctx.add(icon)
            }
            // Leading | Trailing | Top | Bottom
            loc => {
                let icon_first = matches!(loc, IconLocation::Leading | IconLocation::Top);
                let vertical = matches!(loc, IconLocation::Top | IconLocation::Bottom);
                let icon = self.make_icon(text_role.clone());
                let icon_id = ctx.add(icon);
                let text_id = ctx.add(self.make_label_text(text_role));
                let (first, second) = if icon_first {
                    (icon_id, text_id)
                } else {
                    (text_id, icon_id)
                };
                let row: Box<dyn Widget> = if vertical {
                    Box::new(
                        VStack::new()
                            .spacing(btn::BUTTON_ICON_LABEL_GAP)
                            .add_child(first)
                            .add_child(second),
                    )
                } else {
                    Box::new(
                        HStack::new()
                            .spacing(btn::BUTTON_ICON_LABEL_GAP)
                            .add_child(first)
                            .add_child(second),
                    )
                };
                ctx.add_boxed(row)
            }
        };

        // If leading or trailing slots are set, wrap the icon+label
        // content in an HStack: `[leading?, content, trailing?]`. When
        // both slots are absent, the wrap is skipped — the original
        // content node goes straight into the padding, keeping the
        // node count identical to the pre-slot Button for the common
        // case.
        let content_id = if self.leading.is_some() || self.trailing.is_some() {
            let mut row = HStack::new().spacing(btn::BUTTON_ICON_LABEL_GAP);
            if let Some(leading) = self.leading.take() {
                let id = ctx.add_boxed(leading);
                row = row.add_child(id);
            }
            row = row.add_child(content_id);
            if let Some(trailing) = self.trailing.take() {
                let id = ctx.add_boxed(trailing);
                row = row.add_child(id);
            }
            ctx.add(row)
        } else {
            content_id
        };

        // Delegate chrome (background fill, border, focus ring,
        // padding, min size) to the active `ButtonStyle` (resolved
        // above). The four boolean signals derive from the local
        // `interaction` state signal so the style can `.zip` them and
        // pick a per-state recipe slot.
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        // `:focus-visible`: reveal the focus ring during keyboard navigation
        // only, not on a mouse click. Gate raw focus on the input-modality
        // signal (true after a key event, false after pointer-down).
        let is_focused = interaction
            .map(|s| matches!(s, InteractionState::Focused))
            .and(&ctx.focus_visible());
        // `is_disabled` derives from the arena's effective enabled
        // state — NOT from the interaction signal. The interaction
        // signal never carries Disabled anymore (the snapshot-based
        // duality was removed). Style chrome uses this to pick its
        // disabled-background role.
        let is_disabled = effective_enabled.map(|on| !*on);
        let cfg = ButtonStyleConfig {
            label: content_id,
            is_pressed,
            is_hovered,
            is_focused,
            is_disabled,
            variant,
        };
        let root_id = style.make_body(&cfg, ctx);

        // Attach tooltip if configured. The three setters
        // (`tooltip`, `rich_tooltip*`, `composite_tooltip`) are
        // mutually exclusive — every setter clears the other two so
        // exactly one branch runs.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(tooltip_text) = self.tooltip_text.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, root_id, tooltip_text, delay);
        }

        self.root_child_id = Some(root_id);

        let handlers = self.build_handler_set(ctx, interaction);
        ctx.apply_self_handlers(handlers);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // A Button is rigid: it sizes to its content and does NOT shrink in an
        // over-constrained row (a truncated action label reads
        // poorly — the desktop convention is to overflow excess actions into a
        // menu; see `Toolbar`). We therefore take only the content's SIZE and
        // drop its grow/shrink weights. The label still truncates if a caller
        // explicitly constrains the button (e.g. via `FixedSize` / `Shrinkable`).
        match self.root_child_id {
            Some(root_id) => ctx
                .child_size(root_id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Single child fills our bounds
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Button);
        // Read the current label value uniformly through `Prop::get`
        // — Static returns the captured `String`; Bound returns the
        // signal's current value. Keeps AT in sync with `label`.
        builder.set_name(self.label.get());
        // `set_disabled()` is now driven by the framework's
        // accessibility walker from `arena.is_enabled(self_id)`. The
        // composite no longer needs to mirror it — the snapshot path
        // was redundant with the arena and broke under reactive
        // `enabled_when(id, signal)` flips.
        // ARIA disclosure pattern: a button that opens a popup
        // should declare `has_popup` and, if the wrapper tracks
        // it, `expanded`. Both are opt-in — regular buttons with
        // no popup stay silent on these properties.
        if let Some(kind) = self.has_popup {
            builder.set_has_popup(kind);
        }
        if let Some(ref signal) = self.expanded_signal {
            builder.set_expanded(signal.get());
        }
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use teksilo_core::event::{Modifiers, WidgetEvent};
    use teksilo_core::widget_tree::WidgetTree;

    #[test]
    fn focus_ring_only_under_focus_visible() {
        // `:focus-visible`: the focus ring shows during keyboard navigation
        // but not when focus arrived via a pointer click. Programmatic focus
        // leaves `focus_visible` false, so a focused-but-not-keyboard button
        // shows no ring; a key press flips the modality and reveals it.
        let theme = teksilo_core::presets::intui::light();
        let ring = theme.colors.border_focused.to_array();
        let mut tree = WidgetTree::new().with_theme(theme);
        let btn = tree.add(Button::new(lit!("T")).on_activate_fn(|_| {}));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // Focused, but `focus_visible` is still false → ring gated OFF even
        // though the widget holds focus.
        tree.focus(btn);
        assert!(
            !frame_has_color(&tree.render(), ring),
            "no focus ring while focus-visible is false (pointer modality)",
        );

        // A key event flips `focus_visible` true → ring appears (focus held).
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert!(
            frame_has_color(&tree.render(), ring),
            "focus ring shows under keyboard modality",
        );
    }

    /// Whether `color` appears in any color-bearing layer of the frame —
    /// borders land in `shapes` (stroked SDF quads), `decorations`
    /// (`DecorationRect`), or `cosmetic_lines` depending on the widget.
    fn frame_has_color(frame: &teksilo_canvas::RenderFrame, color: [f32; 4]) -> bool {
        frame.shapes.iter().any(|s| s.color == color)
            || frame.decorations.iter().any(|d| d.color == color)
            || frame.cosmetic_lines.iter().any(|l| l.color == color)
    }

    #[test]
    fn filled_button_accent_desaturates_when_window_inactive() {
        // The Filled button bakes its fill via the theme signal
        // (`ColorProp::Bound`), which a plain `theme_signal` resolution would
        // freeze at the active accent — so it must resolve against the
        // window-active palette to grey out like the paint-resolving controls.
        let theme = teksilo_core::presets::intui::light();
        let accent = theme.colors.accent.to_array();
        let inactive_accent = theme.colors.for_inactive_window().accent.to_array();
        assert_ne!(accent, inactive_accent);

        let mut tree = WidgetTree::new().with_theme(theme);
        tree.add(Button::new(lit!("Save")).variant(ButtonVariant::Filled));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // Active: vivid accent fill.
        assert!(
            frame_has_color(&tree.render(), accent),
            "active window: Filled button paints the vivid accent"
        );

        // Inactive: the fill desaturates with every other accent control.
        tree.set_window_active(false);
        let frame = tree.render();
        assert!(
            frame_has_color(&frame, inactive_accent),
            "inactive window: Filled button fill desaturates"
        );
        assert!(
            !frame_has_color(&frame, accent),
            "inactive window: no vivid accent remains"
        );

        // Reactivate: vivid accent returns.
        tree.set_window_active(true);
        assert!(frame_has_color(&tree.render(), accent));
    }

    #[test]
    fn keyup_without_keydown_does_not_fire() {
        // Regression for the MessageBox reopen bug: when a shortcut
        // consumes Enter's KeyDown (dismissing the modal and restoring
        // focus to the trigger button), the trailing KeyUp must not
        // re-activate the trigger.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let fired = Rc::new(Cell::new(0_u32));
        let fired_for_btn = fired.clone();
        let btn = tree.add(Button::new(lit!("T")).on_activate_fn(move |_ctx| {
            fired_for_btn.set(fired_for_btn.get() + 1);
        }));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        tree.focus(btn);

        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            fired.get(),
            0,
            "a lone KeyUp (no matching KeyDown) must not activate the button",
        );

        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            text: None,
        });
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            fired.get(),
            1,
            "a matched KeyDown + KeyUp pair must activate exactly once",
        );
    }

    // Helper: lay out a Target button (left) and an Open trigger (right)
    // side by side, then open a click-opened overlay anchored to the
    // trigger and parked below the bar. Returns the tree plus the pieces
    // the dismiss-passthrough tests assert on.
    fn open_overlay_beside_button() -> (
        WidgetTree,
        teksilo_core::widget_id::WidgetId, // target
        teksilo_core::widget_id::WidgetId, // trigger
        teksilo_core::widget_id::WidgetId, // overlay content
        Rc<Cell<u32>>,                     // target activations
        Rc<Cell<u32>>,                     // trigger activations
    ) {
        use teksilo_core::overlay::{
            DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest,
        };

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let target_fired = Rc::new(Cell::new(0_u32));
        let tf = target_fired.clone();
        let trigger_fired = Rc::new(Cell::new(0_u32));
        let gf = trigger_fired.clone();

        let target =
            tree.add(Button::new(lit!("Target")).on_activate_fn(move |_| tf.set(tf.get() + 1)));
        let trigger =
            tree.add(Button::new(lit!("Open")).on_activate_fn(move |_| gf.set(gf.get() + 1)));
        let content = tree.add(Button::new(lit!("Item")));
        let _root = tree.add(
            crate::primitives::HStack::new()
                .spacing(40.0)
                .add_child(target)
                .add_child(trigger),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.show_overlay(OverlayRequest {
            content_id: content,
            anchor: trigger,
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeOrClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        // Second layout positions the overlay content below the trigger.
        tree.layout(SizeProposal::exact(400.0, 200.0));

        (tree, target, trigger, content, target_fired, trigger_fired)
    }

    #[test]
    fn dismiss_click_activates_button_beneath() {
        // The reported quirk: with a dropdown/menu open, clicking another
        // widget should dismiss the overlay AND activate that widget in a
        // single click — not require a throwaway first click.
        use teksilo_core::event::PointerButton;

        let (mut tree, target, _trigger, _content, target_fired, trigger_fired) =
            open_overlay_beside_button();

        let tb = tree.bounds(target);
        let target_center =
            teksilo_canvas::Point::new(tb.x + tb.width / 2.0, tb.y + tb.height / 2.0);
        // The overlay is parked below the button bar; the dismiss assertion
        // after dispatch confirms this click lands outside it.
        assert_eq!(tree.active_overlays().len(), 1);

        tree.dispatch_event(WidgetEvent::pointer_down(
            target_center,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            target_center,
            PointerButton::Primary,
            Modifiers::NONE,
        ));

        assert!(
            tree.active_overlays().is_empty(),
            "the press should dismiss the open overlay",
        );
        assert_eq!(
            target_fired.get(),
            1,
            "the same press should activate the button beneath the dismissed overlay",
        );
        assert_eq!(trigger_fired.get(), 0);
    }

    #[test]
    fn dismiss_click_on_trigger_is_consumed_not_reactivated() {
        // The anchor guard: clicking the trigger that owns an open overlay
        // must merely close it. The press is consumed, so it can't reach
        // the trigger's own tap handler and reopen what it just closed.
        use teksilo_core::event::PointerButton;

        let (mut tree, _target, trigger, _content, _target_fired, trigger_fired) =
            open_overlay_beside_button();

        let gb = tree.bounds(trigger);
        let trigger_center =
            teksilo_canvas::Point::new(gb.x + gb.width / 2.0, gb.y + gb.height / 2.0);
        assert_eq!(tree.active_overlays().len(), 1);

        tree.dispatch_event(WidgetEvent::pointer_down(
            trigger_center,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            trigger_center,
            PointerButton::Primary,
            Modifiers::NONE,
        ));

        assert!(
            tree.active_overlays().is_empty(),
            "clicking the trigger should close its overlay",
        );
        assert_eq!(
            trigger_fired.get(),
            0,
            "the dismiss press on the anchor must be consumed, not delivered to the trigger",
        );
    }

    #[test]
    fn label_updates_at_name_when_signal_changes() {
        // Regression for the calendar header use case: a Button bound
        // to a `Signal<String>` must (1) display the signal's current
        // value and (2) refresh its accessibility name when the
        // signal changes — without rebuilding the parent.
        use teksilo_core::accessibility::widget_id_to_node_id;
        let label = Signal::new("May 2026".to_string());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            Button::new(lit!(""))
                .label(label.clone())
                .on_activate_fn(|_| {}),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let target = widget_id_to_node_id(id);
        let update = tree.sync_accessibility();
        let (_, node) = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == target)
            .expect("button node");
        assert_eq!(node.label().unwrap_or_default(), "May 2026");

        // Flip the signal — AT name should refresh after the next
        // layout pass (the label registration triggers a
        // re-evaluation of `accessibility()`).
        label.set("2026".to_string());
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let update = tree.sync_accessibility();
        let (_, node) = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == target)
            .expect("button node after relabel");
        assert_eq!(node.label().unwrap_or_default(), "2026");
    }

    #[test]
    fn slots_widen_button_to_accommodate_their_intrinsic_size() {
        // A button with leading + trailing slots reports a wider
        // intrinsic size than the same button without slots — proves
        // the slots actually entered the layout pass. Layout uses
        // `unspecified()` so each button reports its intrinsic width
        // rather than getting stretched to a parent proposal. Both
        // sides also clear the theme's `min_width` (~72dp) which
        // would otherwise mask the slot contribution on the plain
        // button.
        use crate::primitives::MinSize;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let plain = tree.add(Button::new(lit!("X")).on_activate_fn(|_| {}));
        let with_slots = tree.add(
            Button::new(lit!("X"))
                .leading(MinSize::new(120.0, 12.0))
                .trailing(MinSize::new(120.0, 12.0))
                .on_activate_fn(|_| {}),
        );
        tree.layout(SizeProposal::unspecified());
        let plain_w = tree.bounds(plain).width;
        let slot_w = tree.bounds(with_slots).width;
        assert!(
            slot_w >= plain_w + 200.0,
            "expected slot button to be at least 200dp wider than plain (plain={plain_w}, slot={slot_w})",
        );
    }

    #[test]
    fn button_is_rigid_and_does_not_shrink_in_a_tight_row() {
        // A Button is rigid: in an over-constrained row it keeps its natural
        // width (overflows) rather than truncating its action label. The
        // desktop convention is to overflow excess actions into a menu (see
        // `Toolbar`), not to silently truncate buttons.
        use crate::primitives::hstack::HStack;
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                teksilo_canvas::MockTextBackend::new(),
            )));
        let btn = tree.add(Button::new(lit!("Save Document As…")).on_activate_fn(|_| {}));
        let _row = tree.add(HStack::new().add_child(btn));

        tree.layout(SizeProposal::unspecified());
        let natural = tree.bounds(btn).width;
        // Squeeze the row far below natural — the Button keeps its full width.
        tree.layout(SizeProposal::exact(70.0, 40.0));
        let squeezed = tree.bounds(btn).width;

        assert!(
            natural > 100.0,
            "expected a wide natural button, got {natural}"
        );
        assert!(
            (squeezed - natural).abs() < 0.5,
            "button should stay rigid at its natural width \
             (natural={natural}, squeezed={squeezed})"
        );
    }

    #[test]
    fn framework_default_blocks_secondary_tap_on_button() {
        // Framework default: `TapRecognizer::accept = ButtonMask::PRIMARY`.
        // A right-click on a Button does NOT activate. Generalises the
        // tab-specific `primary_click_activates_tab_secondary_does_not`
        // regression to every widget that wires `on_tap`.
        use teksilo_core::event::PointerButton;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let fired = Rc::new(Cell::new(0_u32));
        let fired_for_btn = fired.clone();
        let btn = tree.add(Button::new(lit!("T")).on_activate_fn(move |_ctx| {
            fired_for_btn.set(fired_for_btn.get() + 1);
        }));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let center = tree.bounds(btn).center();

        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.pointer_up_button(center, PointerButton::Secondary);
        assert_eq!(fired.get(), 0, "right-click must not activate a Button");

        tree.pointer_down_button(center, PointerButton::Middle);
        tree.pointer_up_button(center, PointerButton::Middle);
        assert_eq!(fired.get(), 0, "middle-click must not activate a Button");

        // Sanity: primary click still activates.
        tree.pointer_down_button(center, PointerButton::Primary);
        tree.pointer_up_button(center, PointerButton::Primary);
        assert_eq!(fired.get(), 1, "primary-click must activate a Button");
    }

    #[test]
    fn framework_accept_tap_buttons_secondary_fires_handler() {
        // `accept_tap_buttons` opts the auto-wired `TapRecognizer` into
        // a wider button set. With `Secondary` allowed, right-click
        // activates.
        use teksilo_core::event::{ButtonMask, PointerButton};
        use teksilo_core::widget_builder::WidgetBuilder;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let fired = Rc::new(Cell::new(0_u32));
        let fired_for_btn = fired.clone();
        let btn = tree.add(
            Button::new(lit!("T"))
                .on_activate_fn(move |_ctx| {
                    fired_for_btn.set(fired_for_btn.get() + 1);
                })
                .accept_tap_buttons(ButtonMask::PRIMARY | ButtonMask::SECONDARY),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let center = tree.bounds(btn).center();

        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.pointer_up_button(center, PointerButton::Secondary);
        assert_eq!(
            fired.get(),
            1,
            "right-click must activate a Button when accept_tap_buttons includes Secondary",
        );

        tree.pointer_down_button(center, PointerButton::Primary);
        tree.pointer_up_button(center, PointerButton::Primary);
        assert_eq!(fired.get(), 2, "primary-click still activates");
    }

    #[test]
    fn hidden_slot_marks_swatch_node_as_at_hidden() {
        // ColorSwatch declares `Role::ColorWell`. Dropped raw into a
        // Button slot it would appear as a redundant ColorWell child
        // under the Button's node. `.access_hidden(true)` is the
        // documented escape hatch — confirm the swatch's AT node
        // carries the hidden flag (AT readers skip nodes flagged
        // hidden, even though the node still exists in the tree).
        use crate::color_picker::ColorSwatch;
        use teksilo_core::accessibility::widget_id_to_node_id;
        use teksilo_core::accesskit::Role;
        use teksilo_core::widget_builder::WidgetBuilder;
        use teksilo_tokens::Color;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            Button::new(lit!("Pick"))
                .leading(ColorSwatch::new(Color::RED).access_hidden(true))
                .on_activate_fn(|_| {}),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let target = widget_id_to_node_id(id);
        let update = tree.sync_accessibility();
        let (_, btn_node) = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == target)
            .expect("button node");
        assert_eq!(btn_node.role(), Role::Button);
        let color_well_visible = update
            .nodes
            .iter()
            .any(|(_, n)| n.role() == Role::ColorWell && !n.is_hidden());
        assert!(
            !color_well_visible,
            "hidden swatch should not emit a non-hidden ColorWell node",
        );
    }

    #[test]
    fn plain_button_is_a_leaf_no_group_node() {
        // Regression: a Button's chrome is composed from layout primitives
        // (Padding/Center/HStack/…) that emit empty GenericContainer /
        // Unknown AT nodes. VoiceOver announces a GenericContainer as
        // "group", so the button read as "<label>, button, group". The AT
        // walker now collapses presentational nodes — assert the button is
        // a clean leaf and no grouping node survives anywhere.
        use teksilo_core::accessibility::widget_id_to_node_id;
        use teksilo_core::accesskit::Role;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(Button::new(lit!("Valider")).on_activate_fn(|_| {}));
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let _ = tree.render();
        let update = tree.sync_accessibility();

        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, n)| n.role() == Role::GenericContainer),
            "no GenericContainer ('group') node should remain in the AT tree"
        );

        let (_, btn) = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == widget_id_to_node_id(id))
            .expect("button node present");
        assert_eq!(btn.role(), Role::Button);
        assert_eq!(btn.label(), Some("Valider"));
        let has_visible_child = btn.children().iter().any(|cid| {
            update
                .nodes
                .iter()
                .find(|(nid, _)| nid == cid)
                .is_some_and(|(_, n)| !n.is_hidden())
        });
        assert!(
            !has_visible_child,
            "button should expose no visible AT child node (it is a leaf)"
        );
    }

    #[test]
    fn theme_slot_supplies_button_style_when_no_override() {
        // End-to-end check that `theme.style_slots.button = Some(rc)`
        // actually feeds the widget when no per-call `.style(...)`
        // override is present. Uses a custom `ButtonStyle` that adds a
        // sentinel `RectWidget` we can spot in the rendered frame.
        use teksilo_core::styles::{ButtonStyle, ButtonStyleConfig};
        use teksilo_tokens::Color;

        struct SentinelButton;
        impl ButtonStyle for SentinelButton {
            fn make_body(
                &self,
                cfg: &ButtonStyleConfig,
                ctx: &mut teksilo_core::build_context::BuildContext,
            ) -> teksilo_core::widget_id::WidgetId {
                // Distinctive bright-magenta background nobody else paints.
                let bg = ctx.add(
                    crate::primitives::RectWidget::new()
                        .background(Color::new(1.0, 0.0, 1.0, 1.0))
                        .corner_radius(teksilo_tokens::CornerRadius::uniform(0.0)),
                );
                ctx.add(
                    crate::primitives::ZStack::new()
                        .add_child(bg)
                        .add_child(cfg.label),
                )
            }
        }

        let mut theme = teksilo_core::presets::intui::light();
        theme.style_slots.button = Some(Rc::new(SentinelButton));
        let mut tree = WidgetTree::new().with_theme(theme);
        let _btn = tree.add(Button::new(lit!("T")).on_activate_fn(|_| {}));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();

        let sentinel = [1.0_f32, 0.0, 1.0, 1.0];
        assert!(
            frame.shapes.iter().any(|s| s.color == sentinel),
            "the theme's `style_slots.button` impl should drive Button chrome \
             — saw no sentinel magenta rect in the rendered frame",
        );
    }

    #[test]
    fn style_label_text_role_overrides_default_label_color() {
        // A `ButtonStyle` returning `Some(role)` from `label_text_role`
        // redirects the label/icon color — the Material 3 "text and
        // outlined buttons are accent-colored" need. Styles that return
        // `None` (the IntUI default) keep the Button's built-in mapping,
        // so this is purely additive (the rest of the suite covers the
        // default path).
        use std::cell::RefCell;
        use teksilo_canvas::MockTextBackend;
        use teksilo_core::styles::{ButtonStyle, ButtonStyleConfig, ButtonVariant};
        use teksilo_tokens::TextRole;

        struct LabelRoleSentinel;
        impl ButtonStyle for LabelRoleSentinel {
            fn make_body(
                &self,
                cfg: &ButtonStyleConfig,
                ctx: &mut teksilo_core::build_context::BuildContext,
            ) -> teksilo_core::widget_id::WidgetId {
                ctx.add(crate::primitives::ZStack::new().add_child(cfg.label))
            }
            fn label_text_role(&self, _variant: ButtonVariant) -> Option<TextRole> {
                Some(TextRole::Error)
            }
        }

        let want = teksilo_core::presets::intui::light()
            .colors
            .text_error
            .to_array();
        let mut theme = teksilo_core::presets::intui::light();
        theme.style_slots.button = Some(Rc::new(LabelRoleSentinel));
        let mut tree = WidgetTree::new()
            .with_theme(theme)
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        let _btn = tree.add(Button::new(lit!("T")).on_activate_fn(|_| {}));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();

        assert!(
            frame.glyphs.iter().any(|g| g.color == want),
            "style.label_text_role(...) should drive the label glyph color; \
             expected the theme error color {want:?}, saw {:?}",
            frame.glyphs.iter().map(|g| g.color).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn per_call_style_override_wins_over_theme_slot() {
        // When both `Button::style(...)` AND `theme.style_slots.button`
        // are set, the per-call wins. Verified by installing a sentinel
        // style on the theme then a *different* sentinel via `.style()`.
        use teksilo_core::styles::{ButtonStyle, ButtonStyleConfig};
        use teksilo_tokens::Color;

        struct ThemeSentinel;
        impl ButtonStyle for ThemeSentinel {
            fn make_body(
                &self,
                cfg: &ButtonStyleConfig,
                ctx: &mut teksilo_core::build_context::BuildContext,
            ) -> teksilo_core::widget_id::WidgetId {
                let bg = ctx.add(
                    crate::primitives::RectWidget::new()
                        .background(Color::new(1.0, 0.0, 1.0, 1.0)) // magenta
                        .corner_radius(teksilo_tokens::CornerRadius::uniform(0.0)),
                );
                ctx.add(
                    crate::primitives::ZStack::new()
                        .add_child(bg)
                        .add_child(cfg.label),
                )
            }
        }

        struct CallSentinel;
        impl ButtonStyle for CallSentinel {
            fn make_body(
                &self,
                cfg: &ButtonStyleConfig,
                ctx: &mut teksilo_core::build_context::BuildContext,
            ) -> teksilo_core::widget_id::WidgetId {
                let bg = ctx.add(
                    crate::primitives::RectWidget::new()
                        .background(Color::new(0.0, 1.0, 0.0, 1.0)) // green
                        .corner_radius(teksilo_tokens::CornerRadius::uniform(0.0)),
                );
                ctx.add(
                    crate::primitives::ZStack::new()
                        .add_child(bg)
                        .add_child(cfg.label),
                )
            }
        }

        let mut theme = teksilo_core::presets::intui::light();
        theme.style_slots.button = Some(Rc::new(ThemeSentinel));
        let mut tree = WidgetTree::new().with_theme(theme);
        let _btn = tree.add(
            Button::new(lit!("T"))
                .style(CallSentinel)
                .on_activate_fn(|_| {}),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();

        let magenta = [1.0_f32, 0.0, 1.0, 1.0];
        let green = [0.0_f32, 1.0, 0.0, 1.0];
        assert!(
            frame.shapes.iter().any(|s| s.color == green),
            "per-call .style(...) override should drive chrome — no green rect found",
        );
        assert!(
            !frame.shapes.iter().any(|s| s.color == magenta),
            "theme slot must be ignored when per-call override is set — magenta should not appear",
        );
    }
    // -----------------------------------------------------------------
    // The framework press (docs/touch-and-pen.md §7.1)
    // -----------------------------------------------------------------

    /// A `ButtonStyle` that hands the interaction signals its chrome reads back
    /// to the test, so the press *visual* and the resting state can be asserted
    /// through the surface a real style sees rather than through the router's
    /// own bookkeeping.
    struct PressProbe(Rc<RefCell<Option<(Signal<bool>, Signal<bool>)>>>);

    impl teksilo_core::styles::ButtonStyle for PressProbe {
        fn make_body(
            &self,
            cfg: &teksilo_core::styles::ButtonStyleConfig,
            ctx: &mut BuildContext,
        ) -> WidgetId {
            *self.0.borrow_mut() = Some((cfg.is_pressed.clone(), cfg.is_hovered.clone()));
            ctx.add(crate::primitives::ZStack::new().add_child(cfg.label))
        }
    }

    /// A button, its press-visual signal, its hover-visual signal, and how many
    /// times it activated.
    fn probed_button_with_hover() -> (
        WidgetTree,
        WidgetId,
        Signal<bool>,
        Signal<bool>,
        Rc<Cell<u32>>,
    ) {
        let probe: Rc<RefCell<Option<(Signal<bool>, Signal<bool>)>>> = Rc::new(RefCell::new(None));
        let hits = Rc::new(Cell::new(0_u32));
        let counter = hits.clone();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let btn = tree.add(
            Button::new(lit!("Save"))
                .style(PressProbe(probe.clone()))
                .on_activate_fn(move |_| counter.set(counter.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let (pressed, hovered) = probe.borrow().clone().expect("style ran");
        (tree, btn, pressed, hovered, hits)
    }

    /// A button, its press-visual signal, and how many times it activated.
    fn probed_button() -> (WidgetTree, WidgetId, Signal<bool>, Rc<Cell<u32>>) {
        let (tree, btn, pressed, _hovered, hits) = probed_button_with_hover();
        (tree, btn, pressed, hits)
    }

    fn mouse_at(tree: &mut WidgetTree, at: teksilo_canvas::Point, down: bool) {
        let event = if down {
            WidgetEvent::pointer_down(
                at,
                teksilo_core::event::PointerButton::Primary,
                Modifiers::NONE,
            )
        } else {
            WidgetEvent::pointer_up(
                at,
                teksilo_core::event::PointerButton::Primary,
                Modifiers::NONE,
            )
        };
        tree.dispatch_event(event);
    }

    /// The mouse path, unchanged: press lights the visual, release puts it out
    /// and activates once.
    #[test]
    fn a_mouse_click_presses_then_activates_on_release() {
        let (mut tree, btn, pressed, hits) = probed_button();
        let at = tree.bounds(btn).center();
        tree.dispatch_event(WidgetEvent::pointer_move(at));
        mouse_at(&mut tree, at, true);
        assert!(pressed.get(), "a mouse press lights the pressed visual");
        assert_eq!(hits.get(), 0, "nothing has activated on the press");
        mouse_at(&mut tree, at, false);
        assert!(!pressed.get(), "the release puts the visual out");
        assert_eq!(hits.get(), 1, "activation lands on the release");
    }

    /// A finger: the same two steps, with no hover anywhere in them.
    #[test]
    fn a_touch_tap_activates_on_release() {
        use super::press_test_support::{finger, touch};
        use teksilo_core::pointer::PointerPhase;

        let (mut tree, btn, pressed, hits) = probed_button();
        let at = tree.bounds(btn).center();
        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        assert!(pressed.get(), "a contact on a button lights it at once");
        assert_eq!(hits.get(), 0, "a press is not an activation");
        tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 40));
        assert_eq!(hits.get(), 1, "the release activates");
        assert!(!pressed.get(), "and clears the visual");
    }

    /// WCAG 2.2 SC 2.5.2: sliding off abandons the press and sliding back on
    /// restores the *visual*.
    ///
    /// The activation does not come back with it, and that is the framework's
    /// contract rather than this control's choice:
    /// [`TapRecognizer`](teksilo_core::gesture::TapRecognizer) clears its
    /// recorded press position the moment the pointer leaves the tap boundary
    /// (`gesture/tap.rs`, the `Move` arm), so the failure is terminal, while
    /// the router's press record is reversible. Pinned here so a later change
    /// to either half has to change this test deliberately —
    /// `docs/touch-and-pen.md` §7.1 currently says the two "can never
    /// disagree", which holds for the predicate but not for its latching.
    #[test]
    fn a_touch_press_disarms_on_slide_off_and_re_arms_on_re_entry() {
        use super::press_test_support::{finger, touch};
        use teksilo_core::pointer::PointerPhase;

        let (mut tree, btn, pressed, hits) = probed_button();
        let bounds = tree.bounds(btn);
        let at = bounds.center();
        let away = teksilo_canvas::Point::new(at.x, bounds.y + bounds.height + 60.0);
        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        assert!(pressed.get());
        tree.dispatch_pointer(touch(id, PointerPhase::Move, away, 20));
        assert!(!pressed.get(), "the press slid off its target");
        tree.dispatch_pointer(touch(id, PointerPhase::Move, at, 40));
        assert!(pressed.get(), "and came back — the visual is reversible");
        tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 60));
        assert_eq!(
            hits.get(),
            0,
            "the tap recognizer's failure is terminal, so the release that \
             follows an excursion activates nothing",
        );
    }

    /// A release that lands off the button activates nothing and leaves no
    /// visual behind.
    #[test]
    fn a_touch_release_off_the_button_activates_nothing() {
        use super::press_test_support::{finger, touch};
        use teksilo_core::pointer::PointerPhase;

        let (mut tree, btn, pressed, hits) = probed_button();
        let bounds = tree.bounds(btn);
        let at = bounds.center();
        let away = teksilo_canvas::Point::new(at.x, bounds.y + bounds.height + 60.0);
        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        tree.dispatch_pointer(touch(id, PointerPhase::Move, away, 20));
        tree.dispatch_pointer(touch(id, PointerPhase::Up, away, 40));
        assert_eq!(hits.get(), 0, "a slid-off release is not an activation");
        assert!(!pressed.get());
    }

    /// Where the button comes to rest **after an activation**, in both
    /// directions.
    ///
    /// A mouse or a pen is still over the control when it lifts, so the button
    /// rests hovered exactly as it always has. A finger is gone the instant it
    /// lifts and never sends the hover-leave that would correct a `Hovered`
    /// state, so it rests idle — leaving a finger-tapped button lit is the
    /// stuck-highlight every touch port of a desktop toolkit ships first.
    ///
    /// This is the decision inside `on_tap`, and it is asserted through
    /// `ButtonStyleConfig::is_hovered` — the signal a style's chrome actually
    /// reads — rather than through the interaction enum, because the enum is
    /// the button's private business and the tint is not.
    #[test]
    fn a_mouse_release_rests_hovered_and_a_finger_release_rests_idle() {
        use super::press_test_support::touch_tap;

        let (mut tree, btn, pressed, hovered, hits) = probed_button_with_hover();
        let at = tree.bounds(btn).center();
        tree.dispatch_event(WidgetEvent::pointer_move(at));
        assert!(hovered.get(), "the pointer arrived over the button");
        mouse_at(&mut tree, at, true);
        assert!(
            pressed.get() && !hovered.get(),
            "pressed supersedes hovered"
        );
        mouse_at(&mut tree, at, false);
        assert_eq!(hits.get(), 1, "the release activated");
        assert!(!pressed.get(), "and put the press visual out");
        assert!(
            hovered.get(),
            "a mouse that clicked a button is still on it, so the button rests hovered",
        );

        // The same release, made by a finger. A fresh tree: the mouse above
        // still owns a hover this one must not inherit.
        let (mut tree, btn, pressed, hovered, hits) = probed_button_with_hover();
        let at = tree.bounds(btn).center();
        touch_tap(&mut tree, at);
        assert_eq!(hits.get(), 1, "the contact activated on its release");
        assert!(!pressed.get());
        assert!(
            !hovered.get(),
            "a finger leaves nothing behind, so the button must rest idle",
        );
    }

    /// Where the button comes to rest when the press ends with **no**
    /// activation — the other decision site, in `bind_press_interaction`.
    ///
    /// A pan claimant or an ancestor drag winning the arbitration revokes the
    /// press with no release to hang a reset on, so the binding has to restore
    /// the resting state itself. A mouse is still sitting on the control and
    /// must go back to hovered; a finger has no hover to go back to and must go
    /// to idle. Getting either wrong is invisible until it is on screen: a
    /// mouse-cancelled button that resets to idle loses its hover tint until
    /// the pointer moves again, and a finger-cancelled one that resets to
    /// hovered stays lit with nothing touching it.
    #[test]
    fn a_press_taken_away_rests_hovered_under_a_mouse_and_idle_under_a_finger() {
        use super::press_test_support::{finger, touch};
        use teksilo_core::pointer::{CancelReason, PointerId, PointerPhase};

        let (mut tree, btn, pressed, hovered, hits) = probed_button_with_hover();
        let at = tree.bounds(btn).center();
        tree.dispatch_event(WidgetEvent::pointer_move(at));
        mouse_at(&mut tree, at, true);
        assert!(pressed.get());
        tree.cancel_pointer(
            PointerId::MOUSE,
            CancelReason::PeerClaimed,
            &mut teksilo_core::window::NoopWindowOps,
        );
        assert_eq!(hits.get(), 0, "a revoked press activates nothing");
        assert!(!pressed.get(), "and the press visual goes out");
        assert!(
            hovered.get(),
            "the mouse never left the button, so it rests hovered",
        );

        let (mut tree, btn, pressed, hovered, hits) = probed_button_with_hover();
        let at = tree.bounds(btn).center();
        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        assert!(pressed.get());
        tree.cancel_pointer(
            id,
            CancelReason::PeerClaimed,
            &mut teksilo_core::window::NoopWindowOps,
        );
        assert_eq!(hits.get(), 0);
        assert!(!pressed.get());
        assert!(
            !hovered.get(),
            "a finger hovers nothing, so a revoked contact must leave the button idle",
        );
    }

    /// A rebuild that lands **during** a press must not blink the press visual
    /// off — the third decision in `bind_press_interaction`, and the reason it
    /// seeds the interaction signal from the live press instead of from
    /// `false`.
    ///
    /// Every `build()` allocates a fresh interaction signal and derives a fresh
    /// `is_pressed` for the style from it, while the press itself lives on the
    /// arena node and outlives any number of rebuilds. So a button rebuilt with
    /// a contact still on it comes back reading `Idle` unless the binding
    /// re-seeds it, and the chrome goes dark under a finger that never lifted.
    ///
    /// Reaching that needs a **live drag session**, and not by contrivance:
    /// `process_pending_rebuilds` defers any rebuild aimed at a widget holding
    /// a pointer capture, and a press owner *is* the capture owner
    /// (`adopt_press_owner`). The one documented exception is a drag — "a
    /// mid-drag rebuild is safe regardless of topology" — which lifts the
    /// deferral for every widget at once. Two contacts is what puts a real app
    /// there: one finger dragging a row while another rests on a button, which
    /// a data-driven rebuild then reaches.
    ///
    /// The probe is re-read after the rebuild, and the builds are counted, so
    /// the assertion cannot pass on the handles the *first* pass published.
    #[test]
    fn a_rebuild_during_a_press_keeps_the_press_visual_lit() {
        use super::press_test_support::{finger, touch};
        use teksilo_core::drag_payload::DragPayload;
        use teksilo_core::gesture::DragPhase;
        use teksilo_core::pointer::PointerPhase;
        use teksilo_core::widget::LayoutResponse;

        /// `PressProbe`'s counting twin: republishes the config's press signal
        /// on every build, and says how many builds there have been.
        struct CountingProbe(Rc<RefCell<Option<Signal<bool>>>>, Rc<Cell<u32>>);

        impl teksilo_core::styles::ButtonStyle for CountingProbe {
            fn make_body(
                &self,
                cfg: &teksilo_core::styles::ButtonStyleConfig,
                ctx: &mut BuildContext,
            ) -> WidgetId {
                *self.0.borrow_mut() = Some(cfg.is_pressed.clone());
                self.1.set(self.1.get() + 1);
                ctx.add(crate::primitives::ZStack::new().add_child(cfg.label))
            }
        }

        /// The other contact's target: anything that opens a drag session.
        #[derive(Debug)]
        struct DragSource(Rc<Cell<bool>>);

        impl Widget for DragSource {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                let self_id = ctx.self_id();
                let started = self.0.clone();
                ctx.apply_self_handlers(HandlerSet::new().on_drag(
                    move |phase, ctx: &mut EventContext| {
                        if let DragPhase::Started { .. } = phase {
                            started.set(true);
                            ctx.start_drag(self_id, DragPayload::typed(42_u32));
                        }
                    },
                ));
                Vec::new()
            }

            fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
                teksilo_canvas::Size::new(120.0, 80.0).into()
            }
        }

        let probe: Rc<RefCell<Option<Signal<bool>>>> = Rc::new(RefCell::new(None));
        let builds = Rc::new(Cell::new(0_u32));
        let dragging = Rc::new(Cell::new(false));

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let btn = tree.add(
            Button::new(lit!("Save"))
                .style(CountingProbe(probe.clone(), builds.clone()))
                .on_activate_fn(|_| {}),
        );
        let idle_probe: Rc<RefCell<Option<Signal<bool>>>> = Rc::new(RefCell::new(None));
        let builds_idle = Rc::new(Cell::new(0_u32));
        let idle = tree.add(
            Button::new(lit!("Open"))
                .style(CountingProbe(idle_probe.clone(), builds_idle.clone()))
                .on_activate_fn(|_| {}),
        );
        let src = tree.add(DragSource(dragging.clone()));
        let _row = tree.add(HStack::new().add_child(btn).add_child(idle).add_child(src));
        tree.layout(SizeProposal::exact(400.0, 100.0));
        let first_pass = builds.get();
        assert_eq!(first_pass, 1, "the style ran once for the first build");

        // One finger on the button.
        let on_button = tree.bounds(btn).center();
        let held = finger();
        tree.dispatch_pointer(touch(held, PointerPhase::Down, on_button, 0));
        assert!(
            probe.borrow().clone().expect("style ran").get(),
            "the contact lit the press visual",
        );

        // A second finger opens a drag elsewhere, which is what lets a rebuild
        // through while the first contact is still down.
        let on_source = tree.bounds(src).center();
        let dragger = finger();
        tree.dispatch_pointer(touch(dragger, PointerPhase::Down, on_source, 5));
        for (step, ms) in [(60.0_f32, 20_u64), (90.0, 30)] {
            let to = teksilo_canvas::Point::new(on_source.x + step, on_source.y);
            tree.dispatch_pointer(touch(dragger, PointerPhase::Move, to, ms));
        }
        assert!(dragging.get(), "the second contact opened a drag session");
        assert!(
            tree.is_pressed(btn),
            "the first contact still holds the button's press",
        );

        // Now the rebuild — a data change, a bound signal at `Rebuild`, a
        // parent re-emitting its children. It lands with the finger still down.
        tree.arena_mark_needs_rebuild_for_testing(btn);
        tree.arena_mark_needs_rebuild_for_testing(idle);
        tree.layout(SizeProposal::exact(400.0, 100.0));
        assert!(
            builds.get() > first_pass,
            "the rebuild never reached the style, so there is no second config to read",
        );
        assert!(
            tree.is_pressed(btn),
            "the router still holds the press across the rebuild",
        );

        let after = probe.borrow().clone().expect("the style ran again");
        assert!(
            after.get(),
            "the rebuilt button handed its style a config saying it is not pressed, \
             while the finger holding it has not lifted",
        );

        // …and the seed reads the live press rather than lighting every rebuild
        // up: the untouched button rebuilt in the same pass comes back dark.
        assert!(
            !tree.is_pressed(idle),
            "nothing is pressing the second button"
        );
        assert!(
            builds_idle.get() > 1,
            "the second button's rebuild never reached the style either",
        );
        assert!(
            !idle_probe.borrow().clone().expect("style ran").get(),
            "an unpressed button must not come out of a rebuild looking pressed",
        );
    }

    /// Keyboard activation is untouched by the press migration: `Space` still
    /// drives the pressed visual through the family's own key machine, and the
    /// lone-`KeyUp` guard still holds.
    #[test]
    fn keyboard_activation_is_unchanged_by_the_framework_press() {
        let (mut tree, btn, pressed, hits) = probed_button();
        tree.focus(btn);
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Space,
            modifiers: Modifiers::NONE,
            text: Key::Space.to_text().map(str::to_string),
        });
        assert!(pressed.get(), "Space holds the button pressed");
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Space,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(hits.get(), 1);
        assert!(!pressed.get());
        // A stray KeyUp with no matching KeyDown must not activate.
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Space,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(hits.get(), 1, "the lone-KeyUp guard still holds");
    }

    /// The Button's own node is the target the audit measures, and it clears
    /// the 24 dp conformance floor at Compact — the density every existing
    /// layout golden was recorded at.
    #[test]
    fn a_compact_button_clears_the_conformance_floor() {
        let theme = teksilo_core::presets::intui::light();
        let floor = theme.input.min_target_conformance;
        let mut tree = WidgetTree::new().with_theme(theme);
        let btn = tree.add(Button::new(lit!("Save")).on_activate_fn(|_| {}));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let b = tree.bounds(btn);
        assert!(
            b.width >= floor && b.height >= floor,
            "a Compact Button measured {}x{}, under the {floor} dp floor",
            b.width,
            b.height,
        );
    }
}

/// [`Button::icon_keeps_color`] — the icon's own colour survives, or it does not.
#[cfg(test)]
mod icon_color_tests {
    use super::*;
    use teksilo_core::widget_tree::WidgetTree;

    /// A disc in a colour no theme role would ever produce, so finding it in the frame
    /// can only mean the icon kept it.
    const SWATCH: [f32; 4] = [0.93, 0.29, 0.60, 1.0];

    fn swatch_icon() -> IconWidget {
        let centre = teksilo_canvas::Point::new(5.0, 5.0);
        IconWidget::from_path(teksilo_canvas::Path::circle(centre, 4.5), 10.0).color(
            teksilo_tokens::Color::from_rgba(SWATCH[0], SWATCH[1], SWATCH[2], SWATCH[3]),
        )
    }

    fn painted(button: Button) -> bool {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let _ = tree.add(button);
        tree.layout(SizeProposal::exact(240.0, 60.0));
        let frame = tree.render();
        // An `IconWidget::from_path` lands in `paths`, not `shapes` — the button's
        // own chrome is what fills `shapes`.
        frame.paths.iter().any(|p| p.color == SWATCH)
            || frame.shapes.iter().any(|s| s.color == SWATCH)
            || frame.decorations.iter().any(|d| d.color == SWATCH)
    }

    /// The default: an icon repeats the label, so it takes the label's colour and the
    /// button stays one legible unit under every variant and state.
    #[test]
    fn an_icon_is_tinted_to_the_label_by_default() {
        assert!(
            !painted(Button::new(lit!("Tag")).icon(swatch_icon(), IconLocation::Leading)),
            "the icon kept its own colour without being asked to"
        );
    }

    /// And the opt-out, for an icon whose colour *is* the information — a filter chip
    /// carrying a user-chosen tag colour has nothing left if it is tinted away.
    #[test]
    fn icon_keeps_color_survives_the_buttons_tint() {
        assert!(
            painted(
                Button::new(lit!("Tag"))
                    .icon(swatch_icon(), IconLocation::Leading)
                    .icon_keeps_color()
            ),
            "icon_keeps_color did not reach the painted icon"
        );
    }
}
