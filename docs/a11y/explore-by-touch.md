<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Explore by touch, and what the framework says out loud

Two accessibility questions that only arise once an application is driven by a
finger: *is a touch a tap or a probe?*, and *what does the framework itself
announce when a gesture ends?* This page answers both, plus the third thing the
two depend on — the coordinate space Teksilo reports rectangles in.

## 1. One transform, at the root

AccessKit reads a node's `bounds` in the coordinate space of the nearest
ancestor carrying a `transform`, and expects the transformed result to be **in
physical pixels**, relative to the window origin. Teksilo lays out, paints,
hit-tests and drives automation in **logical** pixels throughout, and no
emitter — the core walker, `teksilo-charts`' marks, `teksilo-scene`'s items and
magnets — knows the display scale.

The reconciliation is a single `Affine::scale(device_scale_factor)` on the root
`Role::Window` node. `accesskit_consumer` accumulates a node's transform with
every ancestor's up to the root, so one transform reaches every descendant,
**including synthetic children** — text runs, chart marks, scene items — that
hang off a widget with no transform of its own. It also scales the
per-character positions and widths a text run carries, which live in the same
node space and which a per-rectangle fix-up would silently miss.

The rule this creates, and the reason it is worth stating: **no emitter may
multiply a rectangle by a scale factor.** Doing so double-scales it. The core
walker's conformance is pinned by `emitted_bounds_stay_logical_at_scale_two`;
the charts and scene emitters are in crates the core test cannot reach — the
dependency runs the other way — so each is pinned in its own crate:
`a_mark_emits_its_own_rectangle_and_not_a_scaled_one` for charts, and, for
scene, the pre-existing `a11y_bounds_default_to_screen_projection` and
`a11y_bounds_scene_mode_reports_raw_scene_coords`.

Two consequences worth knowing:

* **Automation is unaffected.** `teksilo-automation` reads a node's *raw*
  bounds and injects logical coordinates, so it keeps agreeing with the pointer
  path. Rewriting the emitted rectangles into physical pixels instead would
  have broken every `drag_node` and point-addressed operation.
* **The AccessKit hit test stays exact.** Teksilo's touch slop (`hit_outset`
  and the miss-only slop pass) lives in `WidgetTree::hit_test` and never
  touches an emitted rectangle. A future enlargement of touch targets must not
  be implemented by inflating `set_bounds`.

**Not** fixed by this, and not claimed to be: a subtree wrapped in `Scale` or
`Rotate` still reports its *un*transformed bounds to assistive technology.
`BuildContext::set_transform` writes an arena property that the renderer and
the hit-test path both read (`WidgetArena::effective_transform`) and that the
accessibility walker does not. `teksilo-scene` avoids the problem by projecting
through its own view transform before emitting; a widget-tier transform scope
has no such projection. That is a separate, pre-existing defect.

## 2. The explore-by-touch gate

While a platform's touch-exploration mode is on — VoiceOver on iOS, TalkBack's
"Explore by touch", Narrator touch mode — a touch is a *probe*: the first tap
announces what is under the finger and a second activates it.

`WidgetTree::set_explore_by_touch` chooses the policy:

| Mode | In force when |
| --- | --- |
| `Off` (default) | never |
| `Auto` | the **operating system** reports an active screen reader |
| `On` | always |

`WidgetTree::explore_by_touch_active()` resolves it.

### Why `Auto` asks the OS and not AccessKit

An AccessKit adapter activates for anything that walks the accessibility tree:
a screen magnifier, a voice-control front end, a UI-automation inspector, a
tree browser like Accerciser. None of those want a touch to become a probe.
Treating activation as evidence of a screen reader would switch exploring on
under an inspector and make the application untouchable.

(Teksilo's own automation bridge is *not* one of them: it calls
`WidgetTree::sync_accessibility` in process and never goes through the platform
adapter, so it activates nothing.)

So the flag comes from the platform, through
`teksilo_platform::AccessibilityPreferences::screen_reader`:

| Platform | Source | Caveat |
| --- | --- | --- |
| Linux | AT-SPI `org.a11y.Status.ScreenReaderEnabled` on `org.a11y.Bus` — the same status object `accesskit_unix` watches for the neighbouring `IsEnabled` | Advisory. Orca sets it; an assistive technology that never touches the status object will not. No bus, no `busctl`, or a sandbox that cannot reach the session bus all answer `Unknown`. |
| Windows | `SPI_GETSCREENREADER` | Narrator / NVDA / JAWS set it, Magnifier does not — the discrimination we want. Windows does not reliably clear it if an assistive technology terminates without doing so itself, so a stale `true` is the failure mode. |
| macOS | `NSWorkspace::isVoiceOverEnabled` | VoiceOver only, which is the only screen reader on the platform. Zoom and Voice Control do not set it. |
| other | — | `Unknown` |

`ScreenReaderState::Unknown` behaves as "no": a platform that cannot answer
must not switch the interaction model.

### The asymmetry: attaching proves nothing, detaching proves something

AccessKit *activation* never sets `ScreenReaderState::Active`, for the reasons
above. AccessKit *deactivation* does set `Inactive`: when the last client
detaches there is no screen reader attached either, whatever the OS flag still
says — which is also the answer to Windows' stale-flag caveat.
`WidgetTree::set_at_client_attached` holds that rule, and only the
`true` → `false` edge acts, so the per-frame push of the flag cannot undo a
fresh OS reading.

### What is *not* implemented

Nothing in the framework branches on `explore_by_touch_active()` yet. The
touch-as-probe interaction it gates — first tap announces, second activates,
and the gesture recognizers stepping aside for both — has no owner. This page
documents a supply, a policy and a query; a test
(`touch_still_reaches_a_handler_with_an_accesskit_client_attached`) fences the
default so that attaching an assistive technology today changes nothing about
where a tap lands.

## 3. What the framework announces

Framework announcements go through `EventContext::announce` /
`BuildContext::announce` and the machinery in `teksilo_core::announcer`. There
is no parallel mechanism; `WidgetTree::announce_unless_widget_speaks` below is
a guard in front of that same door, not a second one.

Three rules govern them.

**One utterance per gesture *end*.** Not per frame, not per delta. A gesture in
progress is not news; its outcome is. This has to be enforced by the producer:
`Announcer` deliberately **queues** rather than coalesces, because
last-write-wins drops the first of two things the user needed to hear.

**Quiet when the widget already speaks.**
`WidgetTree::announce_unless_widget_speaks(widget, message)` skips the message
if the last built accessibility tree carried a non-empty live region inside
`widget`'s subtree. It reads *one tree behind* — the announcement is queued
during dispatch, and the live text it would duplicate belongs to the update
built before it. A widget speaking for the first time in the same dispatch is
therefore not yet visible to the check, which errs toward saying something
rather than toward silence.

**The words are the application's.** `teksilo-i18n` depends on `teksilo-core`,
so nothing in core can name a `LocalizedString` or reach a translation bundle.
An English sentence spoken into a French screen reader is worse than silence,
so core-originated announcements take their wording from a closure the
application registers.

### The list

| Announcement | Producer | State |
| --- | --- | --- |
| Density switched | `WidgetTree::set_input_density`, via the wording registered with `set_density_announcement` | **implemented.** Fires once per real switch (the no-op guard on a repeated set is what makes it once), and is silent if no wording is registered. |
| Long-press opened a context menu | the long-press → context-menu route | not implemented: no such route exists. `show_context_menu_for` is reached only from a secondary-button press, the keyboard, and the AccessKit `ShowContextMenu` action. |
| Selection changed by a drag handle | the touch-text controller | not implemented: the controller does not exist yet. |
| Fling settled, naming the new first visible item | the scrollable data views | not implemented: "first visible item" is `teksilo-widgets` state that `teksilo-core` cannot name. |
| Each committed text-editing command | the touch text-editing commands | not implemented: the commands do not exist yet. |

Every unimplemented row is unimplemented because its *producer* does not exist,
not because the announcement machinery is missing. Each will call
`announce_unless_widget_speaks` at its gesture's terminal edge.

## 4. Reviewed rather than tested

Three statements on this page are true by review of one-line platform calls
that no Linux CI host can execute, and belong on a hardware sign-off checklist:

* `SPI_GETSCREENREADER` returns non-zero while Narrator is running on Windows.
* `NSWorkspace::isVoiceOverEnabled` returns true while VoiceOver is running on
  macOS.
* An `accesskit_winit` adapter really does call the activation, action and
  deactivation handlers Teksilo installs. The *policy* those handlers carry —
  the published-tree snapshot, the attached flag, the redraw wakeup — is unit
  tested headless in `teksilo-platform`; that the adapter invokes them at all
  needs a real window.

The Linux `busctl` reply parser **is** tested, over recorded reply strings
rather than a live bus.
