<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# A horizontal activity rail for top and bottom — backlog

**Status: not started.** Nothing on this page exists. `DockActivityBar` is
vertical-only end to end — the item column is a `VStack`, overflow capacity is
computed from `bounds.height`, the drop indicator draws a horizontal line,
`rail_insertion` is y-only, and the a11y node hardcodes
`Orientation::Vertical`. This is a note for whoever takes the work up, not a
plan that landed.

## The scar it would remove

A top or bottom side's rail is a *vertical* column pinned to the band's leading
cross-edge, and a hidden top/bottom band collapses **completely — rail
included**. So a top/bottom rail is not a persistent reopen affordance the way a
leading/trailing one is: an app whose bottom band ships hidden must hand-wire
its own external button to bring it back. That is an admitted, tested design
scar, not a bug — see `hidden_top_with_rail_fully_collapses`.

A narrower fix was proposed (keep the existing vertical rail alive at zero band
depth), accepted, and then found unsound on a closer reading of the geometry.
**The argument for why lives at its site**, in the `band_depth` / `split_side`
reasoning in
[`docking/geometry.rs`](../crates/teksilo-widgets/src/docking/geometry.rs) —
read it before re-proposing one. The conclusion it reaches is the reason this
page exists: the reopen affordance for top/bottom requires a *horizontal* rail,
and there is no cheap version.

## Why it is worth scheduling rather than dropping

The scar is hit today. A downstream consumer (Skribisto) puts `DockSide::Bottom`
in Rail presentation at 36 dp `Compact`, then hides the band by default — so the
rail is invisible in that app's own default state. It pays for this with two
hand-wired workarounds: a toggle command and a menu item bound to
`side_visible_signal(DockSide::Bottom)`. That menu item *is* the "external
button" `geometry.rs` tells apps to supply. A horizontal rail would let it
delete both and get the affordance for free.

(`DockSide::Top` has no consumer, there or here.)

That makes this a backlog item with a named beneficiary rather than speculative
framework investment — but not a small one. It rewrites `geometry.rs`'s
top/bottom `split_side` arm, **deletes** rather than extends four regression
tests (`top_rail_is_a_leading_column_not_a_band`,
`top_rail_column_mirrors_to_the_right_in_rtl`,
`hidden_top_with_rail_fully_collapses`,
`bottom_rail_column_keeps_handle_inboard`), and changes runtime behaviour for
any existing top/bottom-rail consumer. It is its own design pass and its own
behaviour-change sign-off.

## Scope notes, so the next pass starts warm

- `resize_handle.rs` is **already** fully orientation-generic (it branches on
  `is_horizontal_axis`) — no work there.
- Arrow-key navigation in `activity_bar.rs` already accepts both axes
  (`ArrowUp`/`ArrowLeft` = Prev, `ArrowDown`/`ArrowRight` = Next) — dead
  flexibility today that becomes correct for free, though it needs an RTL pass
  for a horizontal rail, where reading order reverses.
- `RotatedLabel` should be **dropped**, not rotated the other way. It exists
  because a vertical rail has a fixed narrow cross-axis and text runs against
  the flow; on a horizontal rail the flow axis already matches text, so Labeled
  mode is just icon-above-caption.
- `TooltipPlacement::Side` must become `Below` — rail items use `Side` today
  precisely because a `Below` tooltip on a vertical rail drops onto the next
  stacked item, and that stops being true once the items sit side by side.
  `teksilo-core`'s existing two-variant enum already covers it with edge-flip,
  so no new variant is needed.
- The one genuinely open design question is whether a horizontal top/bottom rail
  makes `TabPresentation::Strip` redundant for those sides, or whether the two
  stay complementary.

## Reference

- [Docking](docking.md) — the consumer guide; §4 covers side visibility and the
  external-button workaround this would replace.
- [`docking/geometry.rs`](../crates/teksilo-widgets/src/docking/geometry.rs) —
  the region engine, and the geometry argument against the cheap fix.
- [`docking/activity_bar.rs`](../crates/teksilo-widgets/src/docking/activity_bar.rs)
  — the rail widget that would gain an axis.
