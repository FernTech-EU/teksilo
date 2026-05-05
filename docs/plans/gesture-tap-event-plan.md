# Gesture API rewrite — `TapEvent` for the four tap-family recognizers

## Context

The current `on_tap` / `on_double_tap` / `on_triple_tap` / `on_long_press` callbacks all take `(Point, &mut EventContext)`. That signature drops two pieces of state every caller eventually wants.

1. **Which mouse button fired the gesture.** [`TapRecognizer::process`](crates/fern-core/src/gesture.rs#L203-L243) is button-agnostic, so any Down→Up sequence — primary, secondary, middle, mouse 4/5 — fires `Tap` and runs `on_tap`. This is the root cause of the "right-click activates a tab" bug fixed earlier this session by a per-widget [`on_pointer_event`](crates/fern-widgets/src/tab_widget/header.rs#L567-L601) workaround. The same footgun exists silently on Button, Checkbox, Toggle, MenuItem, Link, Accordion, every Calendar cell, every TableView row, every ListView item — 60+ widgets.

2. **Modifier state at gesture completion.** Shift-click extends a selection; Ctrl-click toggles or opens-in-background. Today, callers that want this fall back to `on_pointer_event` (matching `WidgetEvent::PointerUp { modifiers, .. }`) and drop the recognizer's distance/timing/button-mismatch logic. Modifiers are **dropped at the dispatch boundary** at [event_dispatch_impl.rs:862-865, 888-891](crates/fern-core/src/widget_tree/event_dispatch_impl.rs#L862-L891) when constructing `RawPointerEvent` from `WidgetEvent::PointerDown/Up`.

The fix is to thread `PointerButton` + `Modifiers` through the recognizer machinery, expose them via a `TapEvent` struct passed to all four tap-family callbacks, and default the recognizers to Primary-button only with an opt-in `accept_buttons` knob (Qt's `acceptedButtons` model).

**Scope intentionally limited to the four "click-style" recognizers** (`TapRecognizer`, `DoubleTapRecognizer`, `TripleTapRecognizer`, `LongPressRecognizer`). The drag/pinch/swipe family already exposes button info where relevant (`GestureEvent::DragStarted { button, .. }`) and uses richer phase enums; they're untouched.

**No deferred work.** Every callsite migrates in one PR. Every doc updates in the same PR. The per-widget right-click guard added to TabHeader becomes redundant and is removed.

---

## Design

### 1. The `TapEvent` struct

New public type in [crates/fern-core/src/gesture.rs](crates/fern-core/src/gesture.rs) (re-exported from `fern_core` and from `fern_ui::prelude`):

```rust
/// Information about a recognized click-style gesture, passed to the
/// four tap-family handlers (`on_tap`, `on_double_tap`, `on_triple_tap`,
/// `on_long_press`).
///
/// The struct is non-exhaustive so future fields (timestamp, click count
/// for a hypothetical `on_n_tap`, pressure for stylus events) can land
/// without breaking existing match patterns or constructor calls.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct TapEvent {
    /// Pointer position in widget-local coords, captured at the
    /// finalising event (the `Up` of the last tap for tap/double/triple,
    /// the `Down` of the held press for long-press).
    pub position: Point,

    /// Which button finalised the gesture. Multi-tap recognizers require
    /// every tap in the sequence to use the same button — mixed-button
    /// sequences fail rather than spuriously firing.
    pub button: PointerButton,

    /// Modifier keys held at the finalising event. Same source as
    /// `WidgetEvent::PointerUp { modifiers, .. }` / `PointerDown { modifiers, .. }`.
    pub modifiers: Modifiers,
}
```

One struct, used by all four handlers. They all carry the same shape; a separate `LongPressEvent` would only fragment the API. `#[non_exhaustive]` so we can grow it without churn.

### 2. `RawPointerEvent` — thread modifiers through

[crates/fern-core/src/gesture.rs:20-32](crates/fern-core/src/gesture.rs#L20-L32):

```rust
pub enum RawPointerEvent {
    Down {
        position: Point,
        button: PointerButton,
        modifiers: Modifiers,   // NEW
    },
    Move {
        position: Point,
        // No modifiers — WidgetEvent::PointerMove doesn't carry them
        // and no recognizer needs them at Move time.
    },
    Up {
        position: Point,
        button: PointerButton,
        modifiers: Modifiers,   // NEW
    },
}
```

Dispatch construction sites at [event_dispatch_impl.rs:862, 888, 912](crates/fern-core/src/widget_tree/event_dispatch_impl.rs) get `modifiers: *modifiers` added to the `Down`/`Up` arms.

### 3. Recognizer internals — track button + modifiers, enforce match

Each of the four recognizers gains:

- **An `accept` mask** (default `ButtonMask::PRIMARY`) — which buttons are allowed to fire this recognizer. A non-matching `Down` is treated like the press never happened (returns `Pending`, leaves state untouched).
- **`down_button: Option<PointerButton>`** to carry the press button forward. The `Up` only recognizes when `Up.button == Down.button`. Mismatch → `Failed`. This is what implements "button-match required" cleanly.
- **`down_modifiers: Modifiers`** captured at `Down`, replaced at the finalising `Up`. The recognized event reads modifiers from the most recent Up (or, for `LongPress`, the held Down).

For the multi-tap recognizers (`DoubleTapRecognizer`, `TripleTapRecognizer`), the cross-tap rule is: each new `Down` must match the prior taps' button, otherwise the accumulated state resets. Same logic that already enforces distance/timing.

The `process` body for `TapRecognizer` becomes:

```rust
fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
    match event {
        RawPointerEvent::Down { position, button, modifiers } => {
            if !self.accept.contains(*button) {
                return GestureResult::Pending;
            }
            self.down_position = Some(*position);
            self.down_button = Some(*button);
            self.down_modifiers = *modifiers;
            GestureResult::Pending
        }
        RawPointerEvent::Move { position } => {
            // (existing distance check unchanged)
        }
        RawPointerEvent::Up { position, button, modifiers } => {
            let Some(down) = self.down_position.take() else {
                return GestureResult::Failed;
            };
            let Some(down_button) = self.down_button.take() else {
                return GestureResult::Failed;
            };
            if *button != down_button {
                return GestureResult::Failed;
            }
            // (existing distance check unchanged)
            GestureResult::Recognized(GestureEvent::Tap(TapEvent {
                position: *position,
                button: *button,
                modifiers: *modifiers,
            }))
        }
    }
}
```

`DoubleTapRecognizer` / `TripleTapRecognizer` do the same — capture button on each Down, require the new Down's button to match the accumulated `first_tap_button`, fall through to `Failed` on mismatch (resetting state). `LongPressRecognizer` reads `modifiers` from the held `Down` (since the `tick`-based timeout fires before any `Up`).

### 4. `GestureEvent` — variant payloads switch to `TapEvent`

[crates/fern-core/src/gesture.rs:47-84](crates/fern-core/src/gesture.rs#L47-L84):

```rust
pub enum GestureEvent {
    Tap(TapEvent),         // was: Tap { position: Point }
    DoubleTap(TapEvent),   // was: DoubleTap { position: Point }
    TripleTap(TapEvent),   // was: TripleTap { position: Point }
    LongPress(TapEvent),   // was: LongPress { position: Point }
    DragStarted { /* unchanged */ },
    DragMoved { /* unchanged */ },
    /* …other variants unchanged… */
}
```

Tuple-struct variants (rather than struct variants) keep call sites compact: `GestureEvent::Tap(e)` and `if let GestureEvent::Tap(e) = ...` rather than spreading every field.

### 5. `ButtonMask` — typed bitmask

New public type in [crates/fern-core/src/event.rs](crates/fern-core/src/event.rs) (next to `PointerButton`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ButtonMask(u8);

impl ButtonMask {
    pub const NONE:      Self = Self(0);
    pub const PRIMARY:   Self = Self(1 << 0);
    pub const SECONDARY: Self = Self(1 << 1);
    pub const MIDDLE:    Self = Self(1 << 2);
    pub const FORWARD:   Self = Self(1 << 3);   // mouse 4 ("forward")
    pub const BACK:      Self = Self(1 << 4);   // mouse 5 ("back")
    pub const ALL:       Self = Self(0b0001_1111);

    pub const fn contains(self, button: PointerButton) -> bool;
    pub const fn is_empty(self) -> bool;
    pub const fn union(self, other: Self) -> Self;
    pub const fn intersection(self, other: Self) -> Self;
}

impl From<PointerButton> for ButtonMask { /* single-bit mask */ }
impl<const N: usize> From<[PointerButton; N]> for ButtonMask { /* fold-or */ }
impl std::ops::BitOr for ButtonMask { /* … */ }
impl std::ops::BitAnd for ButtonMask { /* … */ }
```

Usage: `ButtonMask::PRIMARY | ButtonMask::SECONDARY` or `[PointerButton::Primary, PointerButton::Secondary].into()`.

No `bitflags` crate dep — fully ad-hoc. Surface is small enough to maintain.

### 6. Recognizer constructors — opt-in to wider button sets

```rust
impl TapRecognizer {
    pub fn new() -> Self;                              // default: ButtonMask::PRIMARY
    pub fn max_distance(self, dp: f32) -> Self;        // unchanged
    pub fn accept_buttons(self, mask: impl Into<ButtonMask>) -> Self;  // NEW
    pub fn accept_any_button(self) -> Self;            // NEW: shorthand for ButtonMask::ALL
}
```

Same builder family on `DoubleTapRecognizer`, `TripleTapRecognizer`, `LongPressRecognizer`.

### 7. Auto-wired recognizer customization on the WidgetBuilder side

The framework auto-wires recognizers from the handler types attached via `HandlerSet` / `WidgetBuilder` ([widget_builder.rs:1259-1286](crates/fern-core/src/widget_builder.rs)). To customise the auto-wired recognizer's button mask without bypassing the auto-wire entirely, add a parallel knob:

```rust
// On HandlerSet AND on the WidgetBuilder blanket impl AND on WidgetWithHandlers:
.accept_tap_buttons(impl Into<ButtonMask>)
.accept_double_tap_buttons(impl Into<ButtonMask>)
.accept_triple_tap_buttons(impl Into<ButtonMask>)
.accept_long_press_buttons(impl Into<ButtonMask>)
```

Stored as `Option<ButtonMask>` on `EventHandlers` (one field per recognizer family). When the framework's `ensure_gesture_arena` constructs the recognizer, it reads this field and applies it; absent → `ButtonMask::PRIMARY`.

Most callers ignore this knob and get safe defaults; the rare "I want left + right" case is a one-liner.

### 8. Handler type aliases and signatures

[crates/fern-core/src/event_handlers.rs:18-23](crates/fern-core/src/event_handlers.rs#L18-L23) — replace the four tap-family closure types with a single shared alias:

```rust
pub type TapHandler = Box<dyn FnMut(&TapEvent, &mut EventContext)>;

pub struct EventHandlers {
    pub on_tap:        Option<TapHandler>,
    pub on_double_tap: Option<TapHandler>,
    pub on_triple_tap: Option<TapHandler>,
    pub on_long_press: Option<TapHandler>,
    pub tap_buttons:        Option<ButtonMask>,   // NEW
    pub double_tap_buttons: Option<ButtonMask>,   // NEW
    pub triple_tap_buttons: Option<ButtonMask>,   // NEW
    pub long_press_buttons: Option<ButtonMask>,   // NEW
    /* …other handler fields unchanged… */
}
```

The closure receives `&TapEvent` (borrowed — TapEvent is `Copy`, but `&TapEvent` is conventional for "event you observe, don't move"). All four signatures are identical, so a single alias replaces four near-duplicate `Box<dyn FnMut(...)>` types.

Builder methods on `HandlerSet` ([widget_builder.rs:314-341](crates/fern-core/src/widget_builder.rs#L314-L341)) and `WidgetWithHandlers` ([widget_builder.rs:600-618](crates/fern-core/src/widget_builder.rs#L600-L618)) and the `WidgetBuilder` blanket trait ([widget_builder.rs:1260-1286](crates/fern-core/src/widget_builder.rs#L1260-L1286)) accept `impl FnMut(&TapEvent, &mut EventContext) + 'static`.

### 9. Dispatch — pass the `TapEvent` through

[event_dispatch_impl.rs:1002-1135](crates/fern-core/src/widget_tree/event_dispatch_impl.rs) — `dispatch_recognized_gesture` rewrites to:

```rust
GestureEvent::Tap(event) => {
    if let Some(h) = node.external_handlers.on_tap.as_mut() {
        h(&event, ctx);
    }
    if let Some(h) = node.handlers.on_tap.as_mut() {
        h(&event, ctx);
    }
}
// same shape for DoubleTap, TripleTap, LongPress
```

`event` is owned `TapEvent` (Copy); we hand `&event` to each handler (borrows are cheap; no need to clone).

### 10. Migration of the 76 callsites

From the audit:

- **7 sites use `position` directly** (color picker hsv_canvas / alpha_strip / hue_strip, slider, scroll_bar, text_widget, inspector locale/tree/data_models, scene minimap, rich_text double/triple-tap, text_input_field double/triple-tap). Each rewrites `|position, ctx|` → `|event, ctx|` and `position` → `event.position` (or via `let TapEvent { position, .. } = *event;` destructure).
- **69 sites ignore the position** (`|_pos, ctx|` or `|_pos, _ctx|`). Each rewrites to `|_, ctx|` / `|_, _ctx|` — same arity, same body, the `_pos` is just renamed.

Mechanical migration; no logic changes. The five widgets the audit highlighted:

| File | Action |
| --- | --- |
| [tab_widget/header.rs:402](crates/fern-widgets/src/tab_widget/header.rs#L402) | Migrate closure; **also** remove the now-redundant non-Primary `on_pointer_event` guard installed in this session (lines ~567-601) — the framework default-Primary subsumes it. |
| [color_picker/hsv_canvas.rs:145](crates/fern-widgets/src/color_picker/hsv_canvas.rs#L145), [alpha_strip.rs:159](crates/fern-widgets/src/color_picker/alpha_strip.rs#L159), [hue_strip.rs:171](crates/fern-widgets/src/color_picker/hue_strip.rs#L171) | `position` → `event.position`. |
| [primitives/text_widget.rs:266](crates/fern-widgets/src/primitives/text_widget.rs#L266) | `pt` → `event.position`. |
| [scroll_bar.rs:369](crates/fern-widgets/src/scroll_bar.rs#L369) | `position` → `event.position`. |
| [slider.rs:244](crates/fern-widgets/src/slider.rs#L244) | `position` → `event.position`. |
| [rich_text.rs:1124, 1128](crates/fern-widgets/src/rich_text.rs#L1124-L1128) (double + triple tap) | `pos` → `event.position`. |
| [primitives/text_input_field.rs:915, 918](crates/fern-widgets/src/primitives/text_input_field.rs#L915-L918) | `pos` → `event.position`. |
| [scene/minimap.rs:232](crates/fern-scene/src/minimap.rs#L232) | `local` → `event.position`. |
| [inspector/tabs/locale.rs:56](crates/fern-inspector/src/tabs/locale.rs#L56), [data_models.rs:73](crates/fern-inspector/src/tabs/data_models.rs#L73), [tree.rs:153](crates/fern-inspector/src/tabs/tree.rs#L153) | `position` → `event.position`. |

The other 60+ widget callsites just have their closure args renamed.

### 11. Tests

[crates/fern-core/src/gesture.rs](crates/fern-core/src/gesture.rs) — every recognizer test (lines 1064-1703) updates assertions:

- Before: `assert_eq!(rec.process(&Down{..}), Pending); assert!(matches!(rec.process(&Up{..}), Recognized(GestureEvent::Tap{..})))`.
- After: `Down/Up` constructors take an extra `modifiers: Modifiers::NONE`; recognized variants destructure `GestureEvent::Tap(TapEvent { position, button, modifiers })` and assert each field.

**New tests added** (covers the design contract):

| Test | Asserts |
| --- | --- |
| `tap_default_filters_secondary_button` | Down{Secondary} → Pending; Up{Secondary} → Failed. No tap recognized. |
| `tap_default_filters_middle_button` | Same shape, Middle button. |
| `tap_accept_secondary_recognises_right_click` | `TapRecognizer::new().accept_buttons(ButtonMask::SECONDARY)` recognises right-click. |
| `tap_button_mismatch_fails` | Down{Primary} then Up{Secondary} → Failed. |
| `tap_carries_modifiers_from_up` | Up with `Modifiers::CTRL` → recognized event has `modifiers.ctrl() == true`. |
| `double_tap_button_mismatch_fails_at_second_down` | First tap Primary, second tap Secondary → no DoubleTap. |
| `triple_tap_button_mismatch_fails_at_third_down` | Third tap mismatch → no TripleTap (and the in-flight DoubleTap's state is reset). |
| `long_press_carries_modifiers_from_down` | Down with `Modifiers::SHIFT`, no movement, tick past min_duration → recognized event has shift set. |
| `long_press_default_filters_secondary` | Default-Primary applies to LongPressRecognizer too. |
| `accept_any_button_recognises_all_three_main_buttons` | One recognizer instance, three sequential clicks Primary/Secondary/Middle, three Tap events. |

Plus end-to-end widget tests:

| Test | File | Asserts |
| --- | --- | --- |
| `framework_default_blocks_secondary_tap_on_button` | [crates/fern-widgets/src/button.rs](crates/fern-widgets/src/button.rs) (tests module) | `Button::new(...).on_tap(...)` does NOT fire on right-click without `accept_tap_buttons(...)`. Replaces / generalises the tab-specific test. |
| `framework_accept_tap_buttons_secondary_fires_handler` | same | `Button::new(...).accept_tap_buttons(ButtonMask::SECONDARY).on_tap(|e, _| { e.button is Secondary })`. |
| `tab_header_no_longer_needs_per_widget_guard` | [crates/fern-widgets/src/tab_widget/tests.rs](crates/fern-widgets/src/tab_widget/tests.rs) | The existing `primary_click_activates_tab_secondary_does_not` regression test still passes after the per-widget guard is removed. |

### 12. `fern!` DSL

The DSL desugars `on_tap: |…, ctx| …` to `.on_tap(|…, ctx| …)`. Closure arity is unchanged (still two args). The DSL machinery doesn't introspect closure types — fern-ui-macros emits the closure verbatim. **No DSL grammar / lowering changes needed.** Doc examples are still updated (the closure's first arg is now `&TapEvent`, not `Point`).

### 13. The previously-added per-widget right-click guard

[crates/fern-widgets/src/tab_widget/header.rs](crates/fern-widgets/src/tab_widget/header.rs) currently filters non-Primary `PointerDown` / `PointerUp` via an extended `on_pointer_event` (added earlier this session). After the framework migration:

- Remove the non-Primary branch from the `on_pointer_event` arms.
- Keep the Middle-click PointerUp arm (still needed: middle-click closes the tab — that's a tab-specific behaviour distinct from "tap activates"). The `on_pointer_event` returning `Handled` for Middle Up is independent of the tap path.
- The existing regression test `primary_click_activates_tab_secondary_does_not` keeps working with no changes — it now exercises the framework default rather than the per-widget guard.

---

## Files to modify

### Core types and dispatch (~6 files)

- [crates/fern-core/src/event.rs](crates/fern-core/src/event.rs) — add `ButtonMask` near `PointerButton`. Pub-export.
- [crates/fern-core/src/gesture.rs](crates/fern-core/src/gesture.rs) — add `TapEvent` type, change `RawPointerEvent::Down/Up` to carry `modifiers`, change `GestureEvent::Tap/DoubleTap/TripleTap/LongPress` to tuple variants holding `TapEvent`, update all four recognizer impls (`down_button`, `down_modifiers`, button-match check, `accept` field, `accept_buttons` builder method), update tests.
- [crates/fern-core/src/event_handlers.rs](crates/fern-core/src/event_handlers.rs) — replace four closure fields with `TapHandler` alias; add the four `*_buttons: Option<ButtonMask>` fields.
- [crates/fern-core/src/widget_builder.rs](crates/fern-core/src/widget_builder.rs) — update `HandlerSet` builder methods (lines 314-341), `WidgetWithHandlers` impls (lines 600-618), `WidgetBuilder` blanket trait (lines 1260-1286). Add `accept_tap_buttons` / `accept_double_tap_buttons` / `accept_triple_tap_buttons` / `accept_long_press_buttons` knobs on all three.
- [crates/fern-core/src/widget_tree/event_dispatch_impl.rs](crates/fern-core/src/widget_tree/event_dispatch_impl.rs) — pass `modifiers` into `RawPointerEvent::Down/Up` constructors at lines 862, 888 (Move at line 912 unchanged); update `dispatch_recognized_gesture` (lines 1002-1135) to destructure `GestureEvent::Tap(event)` and pass `&event`; update `ensure_gesture_arena` to apply `*_buttons` overrides when present.
- [crates/fern-core/src/lib.rs](crates/fern-core/src/lib.rs) (or the umbrella re-export module) — export `TapEvent`, `ButtonMask`. Confirm `fern_ui::prelude` re-exports both.

### Callsite migration — closure rename / position-extract

The 7 position-using widgets (file paths in §10 above), plus the 69 placeholder-only callsites listed in the audit. Mechanical edits — no logic changes.

### Per-widget guard removal

- [crates/fern-widgets/src/tab_widget/header.rs](crates/fern-widgets/src/tab_widget/header.rs) — strip the non-Primary `on_pointer_event` branch (kept session-temporary), keep the Middle-Up branch.

### Test updates and additions

- [crates/fern-core/src/gesture.rs](crates/fern-core/src/gesture.rs) — update existing 11 recognizer tests (signatures + assertions); add 10 new tests per §11.
- [crates/fern-widgets/src/button.rs](crates/fern-widgets/src/button.rs) — add the two end-to-end framework-default tests.
- [crates/fern-widgets/src/tab_widget/tests.rs](crates/fern-widgets/src/tab_widget/tests.rs) — verify `primary_click_activates_tab_secondary_does_not` still passes after the per-widget guard is removed (no changes expected to the test itself).
- Run the existing 776+ fern-widgets tests; expect zero regressions modulo the migrated signatures.

### Documentation — update in the same PR

- **[docs/events-and-gestures.md](docs/events-and-gestures.md)** — primary user-facing reference. Rewrite the tap-handler section (lines 72-98, 126-146): show `TapEvent`, the default-Primary semantic, the `accept_tap_buttons` opt-in, the button-match contract for multi-tap, the modifier-from-Up-or-Down rule.
- **[docs/fern-ui-architecture.md](docs/fern-ui-architecture.md)** — update the V2 attached-handler signature block (lines 3898-3913), the `EventHandlers` field listing (lines 4069-4071), and the "auto-wired recognizers" passage (lines 1061, 4083, 4110, 4112, 4162).
- **[docs/fern-macro-reference.md](docs/fern-macro-reference.md)** — update closure examples (lines 212-223, 337-344, 413-436) to show the new first-arg type. Closure syntax in the DSL is unchanged.
- **[docs/fern-language-spec-v3.md](docs/fern-language-spec-v3.md)** — same: lines 154-175, 292-300, 471-483, 935-1023, 1308-1337.
- **[docs/fern-scene.md](docs/fern-scene.md)** — line 153-154, fix the `.on_tap()` / `.on_double_tap()` example.
- **[docs/plans/title-bar-plan.md](docs/plans/title-bar-plan.md)** and **[docs/plans/scene-refactor-plan.md](docs/plans/scene-refactor-plan.md)** — these are landed-feature plans. Update the API-signature passages (title-bar lines 77, 93, 101-117; scene-refactor lines 184-185) so they don't mislead a future reader.
- **[.claude/CLAUDE.md](.claude/CLAUDE.md)** — update the "Event System (V2 Attached Handlers)" section to mention `TapEvent`, the default-Primary filter, and `accept_tap_buttons`. Lines 346, 350, 747 (the `.on_tap` example).
- **Module-level rustdoc** on [gesture.rs](crates/fern-core/src/gesture.rs), [event_handlers.rs](crates/fern-core/src/event_handlers.rs), [widget_builder.rs](crates/fern-core/src/widget_builder.rs) — refresh the contract description.

---

## Verification

### Unit / integration tests

```bash
cargo test -p fern-core gesture::                # 11 updated + 10 new recognizer tests
cargo test -p fern-core widget_tree::            # dispatch unchanged: should pass clean
cargo test -p fern-widgets                       # 780+ widget tests, all migrated callsites
cargo test --workspace                           # belt-and-braces: catches anything else
```

Expected outcome: all green. The audit confirms zero callsites currently rely on right-click firing `on_tap` accidentally; the default-Primary change is a strict bug fix, not a behaviour shift for any working widget.

### Manual end-to-end

```bash
cargo run -p tab-widget          # right-click no longer activates; left-click still does
cargo run -p color-picker        # position-using callsites still drag the swatch correctly
cargo run -p data-grid           # TableView row click still selects (heavy on_tap user)
cargo run -p widget-catalog      # broad regression sweep
cargo run -p drag-and-drop       # drag/swipe path unchanged — sanity-check the boundary
cargo run -p rich_text_editor    # double-tap word + triple-tap line still work
```

Manual checks per app:

1. **tab_widget**: right-click on a tab — context menu opens (or nothing happens if no factory), tab does NOT activate. Middle-click still closes. Left-click activates.
2. **color_picker**: drag the SV canvas, alpha strip, hue strip — color readout follows the pointer (regression check: `event.position` correctly extracted).
3. **rich_text_editor**: double-tap selects a word; triple-tap selects a line.
4. **inspector** (F12): clicking a node in the tree tab still selects it; clicking a row in data-models tab still highlights it.
5. **scene minimap**: tap-to-pan still recenters the viewport.

### Type-check sweep

```bash
cargo check --workspace --all-targets    # no callsite missed
cargo clippy --workspace --all-targets   # closure-arg-naming consistency
cargo doc --no-deps                       # rustdoc renders the new TapEvent signature without warnings
```

### Plan-file resolution

After the rewrite lands, this plan file is moved to `docs/plans/gesture-tap-event-plan.md` (matching the convention used by [docs/plans/widgets-plan.md](docs/plans/widgets-plan.md), [docs/plans/charts-plan.md](docs/plans/charts-plan.md), etc.) so it survives as the design log. The eponymous review file at `/home/cyril/.claude/plans/do-a-full-review-merry-comet.md` retains a single sentence pointing at the moved doc.
