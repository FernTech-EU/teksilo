<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Overlays: placement and dismissal

An overlay is content that renders outside the layout hierarchy — a menu, a
dropdown, a tooltip, a modal, a toast, a selection handle. `OverlayManager`
owns the stack; this page is the contract for the two decisions it makes on
every one of them: **where does it go** and **what closes it**.

Source: [`crates/teksilo-core/src/overlay.rs`](../crates/teksilo-core/src/overlay.rs),
[`overlay/placement_impl.rs`](../crates/teksilo-core/src/overlay/placement_impl.rs),
[`overlay/viewport.rs`](../crates/teksilo-core/src/overlay/viewport.rs),
[`overlay/direction.rs`](../crates/teksilo-core/src/overlay/direction.rs),
[`overlay/text_affordance.rs`](../crates/teksilo-core/src/overlay/text_affordance.rs).

---

## 1. The area an overlay may occupy

Placement clamps into `OverlayViewport::usable(direction)`, not into the window.

```rust
OverlayViewport {
    size: Size,                 // the window
    safe_area: EdgeInsets,      // notch, rounded corner, home indicator (RTL-aware)
    occluded: Option<Rect>,     // soft keyboard, IME candidate window
}
```

`usable()` insets the window by `safe_area` and then, when something is
covering it, keeps the **largest rectangle the occlusion leaves behind**. A
keyboard spanning the bottom leaves the band above it; a candidate window
docked to a side leaves the band beside it; and neither case needs the platform
to name an edge, because platforms report a rectangle. An occlusion that covers
everything is ignored — nowhere is better than anywhere, and collapsing the
area would send every overlay to the origin.

`OverlayViewport: From<(f32, f32)>`, and that default — whole window, no
insets, no occlusion — reduces to the pre-existing behaviour to the pixel. A
desktop window's placement is unchanged.

The one placement that ignores all of it is `FullViewport`, the modal scrim: a
scrim that respected the safe area would leave the notch undimmed and the
content behind it legible, which is the one thing a scrim exists to prevent.

## 2. Placements

| Placement | Anchored to | Notes |
| --- | --- | --- |
| `Below` | anchor rect | leading-edge aligned; **x** clamped into `usable()`, y is not — it drops below the anchor and may run past the bottom |
| `Above` | anchor rect | leading-edge aligned, x clamped; sits above the anchor, shrunk to the room there rather than clamped down onto it — see §2.1 |
| `TrailingEdge` | anchor rect | submenus; flips to the leading side when it does not fit; y clamped at both ends |
| `BelowPreferred` | anchor rect | drops below, flips above; when it fits on neither side, the roomier side, shrunk to it — see §2.1 |
| `NearAnchor { offset }` | anchor rect | tooltips; direction-aware horizontal anchoring, x clamped at both ends, y flips above and clamps to the top |
| `AtPointer(point)` | a point | the mouse context menu, unchanged; x clamped at both ends, y flips above and clamps to the top |
| `AtPointerAvoiding { point, avoid }` | a point | the **touch** context menu — see §3 |
| `AboveSelection { selection }` | a text range | the selection toolbar — see §4 |
| `Centered` | viewport | modals; centres inside `usable()` |
| `BottomCenter` | viewport | snackbars |
| `ViewportCorner { corner, margin }` | viewport | toasts; `corner` is RTL-resolved |
| `FullViewport` | window | the scrim; ignores insets and occlusion |

### 2.1 A panel never covers its own anchor

`Above` and `BelowPreferred` flip upward, and a panel taller than the room above
its anchor lands at a negative `y` with its *first* rows — a popover's title, a
combo box's first item — clipped by the window. The obvious repair, clamping `y`
to the top of `usable()`, is worse than the disease: for a rigid panel the clamp
can only ever bite when the panel is taller than the room above, which is
exactly when clamping slides it down **onto the control that opened it**. A user
who cannot see the combo box cannot see what they are choosing for.

So the clamp yields to the anchor, and the panel is shrunk instead:

1. **While it fits, nothing changes.** Below if the room below holds it, else
   above if the room above does — the same `y`, to the pixel, as before.
2. **Neither side holds it → the roomier side, shrunk to that room.** A tie
   keeps the flip upward. The shrink is real rather than cosmetic: the overlay
   pass lays content out at `SizeProposal::exact(bounds.width, bounds.height)`,
   so a shorter rect is a shorter list that scrolls, not a full-height list
   drawn off the edge of the window with rows nobody can reach.
3. **No room on either side → the ideal position is kept.** An anchor that
   reaches the top of the usable area — a control occupying the whole window —
   leaves nothing to shrink into, and an empty panel is not an improvement on a
   badly placed one. The panel hangs off the anchor's edge; the anchor stays
   visible either way.

`AtPointer` and `NearAnchor` do clamp to the top, and keep doing so: what they
hang off is a pointer position or a tooltip's target, neither of which the panel
is being read *against*.

## 3. Contact avoidance

A mouse cursor is an arrow drawn *beside* the pixel it names, so a menu whose
corner lands on that pixel is fully visible. A finger is an opaque disc centred
on it, so the same menu opens with its first two rows underneath the hand.

The fix is not an offset — one large enough for a thumb is absurd for a stylus
— but a **rectangle to keep clear**, sized from the contact patch the digitiser
reported:

```rust
OverlayPlacement::at_pointer_for(point, &pointer)
```

* precise pointer (mouse, pen) → `AtPointer(point)`, byte for byte as before;
* coarse pointer (finger) → `AtPointerAvoiding { point, avoid }` where `avoid`
  is the reported contact patch centred on `point`, floored at 24 × 24 dp
  (`ASSUMED_CONTACT_PATCH` — the smallest thing a finger is ever asked to hit
  is therefore the smallest rectangle it can be assumed to cover).

This decision lives in `at_pointer_for`, not at the call sites. A menu that
forgot to ask opens under the finger, and there is no way to notice that from a
mouse. In core the single point-anchored menu route is
`WidgetTree::show_context_menu_for`, which every `.context_menu(..)` factory,
the `ShowContextMenu` AT action and the context-menu key all pass through.

**Quadrant preference.** Four candidates, in order: inline-start of the
contact, inline-end, below it, above it. The first two clear on the horizontal
axis and so accept any vertical position, which is what lets the vertical clamp
run without ever undoing the clearance; the last two are the transpose. Only a
panel as large as the usable area falls through to the both-axes clamp, where
overlap is unavoidable by construction.

Inline-start first because a hand reaches in from the reader's own side, so the
far side is the one that stays visible under it.

## 4. The selection toolbar

`AboveSelection { selection }` floats 8 dp above the selection's bounding
rectangle, centred on it, flipping below when the selection is against the top
of the usable area. What it hangs off is a range of text, not a widget, so its
`anchor` field is bookkeeping and it does not follow focus out of that widget.

When it cannot be centred, the edge that survives is the one the line starts
at: left under LTR, right under RTL.

## 5. The text-affordance band

Overlays sort by **band** first and show order second:

```
OverlayBand::TextAffordance   selection handles, magnifier, selection toolbar
OverlayBand::Standard         menus, dialogs, popovers, tooltips, toasts (default)
```

A selection handle raised while a menu is open therefore lands *under* the
menu, not on top of it. Raise one with `WidgetTree::show_overlay_in_band`.

The band exists for two reasons a plain overlay cannot satisfy:

* **Clipping.** Every real editor sits inside something with `clips_children`.
  A handle hangs below the last line and a magnifier above the caret — exactly
  the geometry the clip removes.
* **Dismissal.** Every tap that moves a caret is "outside" a selection handle,
  so outside-press dismissal would retire the handles on the first tap that
  used them. Overlays in this band are exempt from it; their lifetime belongs
  to the controller that raised them.

## 6. Outside-press dismissal

Dismissal is **layered**, not stack-wide: a press inside overlay *k* is still
outside every overlay above *k*, so those close and *k* and below survive.

### 6.1 A mouse dismisses on the press

Unchanged. The press closes what it is outside of and then **falls through** to
the widget beneath, so one click both closes the menu and actuates the control
under it. The single exception is a primary press on a click-opened overlay's
own anchor, which is consumed — the anchor's tap handler would otherwise
reopen what the press just closed.

That is defensible for a cursor: it names one pixel, and the user could see
that pixel the whole time they were aiming at it.

### 6.2 A direct pointer dismisses on the release

It is not defensible for a finger. The menu is the only thing the user was
looking at; the control underneath is one they never saw. So a direct pointer
**arms** instead:

```rust
OverlayManager::arm_dismiss(pointer, point, busy) -> DismissArm { will_dismiss, suppress_beneath }
OverlayManager::commit_dismiss(pointer, point)    -> (dismissed, focus_restore, anchors)
OverlayManager::abort_dismiss(pointer)            -> bool
```

Arms are tracked per `PointerId`. While one stands:

* the arming `Down` is **withheld** from the widget beneath — nothing is
  pressed, nothing is captured, no arbitration opens;
* the release commits it, and the `Up` is consumed too, so nothing beneath ever
  sees half a press;
* a **cancel** aborts it, having delivered nothing at all;
* a release that landed **inside** an overlay the arm named aborts it as well —
  the finger slid onto the menu and changed its mind.

A press that would close nothing arms nothing; otherwise every touch in a
window with no overlay open would be eaten.

### 6.3 A busy contact protects what it is working in

`arm_dismiss` and the mouse path both take `busy`: the points at which *other*
pointers hold a live press. Each raises the floor of the layered rule to the
overlay it is inside, so a second finger landing on the page while the first is
dragging a menu's scrollbar is not a dismissal gesture.

"Live press" is
[`press_is_revocable`](../crates/teksilo-core/src/widget_tree/pointer_cancel.rs)
— the same predicate the cancel funnel uses, deliberately reused rather than
restated. A pointer with neither a sequence nor a capture has no interaction
that could be taken away, so it has none to protect either. Two consequences
fall out of that: a contact that is merely *holding an arm* does not block
another contact's dismissal, and a pointer inside its terminal `Up` is exempt.

## 7. RTL

One resolver, in
[`overlay/direction.rs`](../crates/teksilo-core/src/overlay/direction.rs), turns
a reading direction into a physical side. Deriving that answer separately in
each consumer is how two of them end up disagreeing.

| Concept | LTR | RTL |
| --- | --- | --- |
| `InlineDirection::Forward` | right | left |
| `InlineDirection::Backward` | left | right |
| `AtPointerAvoiding` preferred quadrant | left of the contact | right of the contact |
| `AboveSelection` overflow keeps | the left edge | the right edge |
| `SelectionHandleKind::Start` | left of the selection | right of the selection |
| `SelectionHandleKind::End` | right of the selection | left of the selection |
| `SwipeDirection::Right` means | `Forward` | `Backward` |
| DnD auto-scroll `Backward` band | leading (left) strip | leading (right) strip |
| `ViewportCorner::TopTrailing` | top-right | top-left |
| `TouchAction::PAN_X` | horizontal | horizontal — **not mirrored** |

The pan axes are **axis-relative and stay that way**. "This subtree may pan
horizontally" is a statement about the x axis, not about reading order, and
mirroring it under RTL would silently forbid the gesture the author permitted.
`InlineDirection::from_swipe` answering `None` for a vertical swipe is the same
rule from the other end: only the inline axis resolves through a direction.

Helpers: `InlineDirection::{side, to_swipe, from_swipe}`,
`SelectionHandleKind::{inline, side}`, `inline_edge_band`, `inline_band_at`.
