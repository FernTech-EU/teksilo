<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Touch and pen

Standard widgets use the framework pointer and gesture system for mouse, touch,
and stylus input. Custom widgets should use the same event context rather than
track a global mouse position.

## Minimal example

This helper can be called from a pointer handler:

```rust
use teksilo::prelude::*;

fn inspect_pointer(ctx: &EventContext) {
    let kind = ctx.pointer_kind();
    let position = ctx.pointer_position();
    println!("{kind:?} at {position:?}");
}
```

`pointer_position()` is optional and uses window-logical coordinates;
`ctx.to_local(p)` converts a window-logical point into the handler's own space.
`pointer()` provides the current pointer identity and axes. Outside pointer,
scroll, gesture, or drag dispatch, the context uses fallback pointer information;
do not interpret that fallback as a new physical sample.

## Common operations

- Use activation handlers for buttons. A direct-pointer press can become a pan
  or be cancelled before activation.
- Use `pointer_kind()` to distinguish coarse input from mouse input.
- Use `owns_pointer()` when handling moves during a captured interaction.
- Handle `PointerCancel` as terminal: clear drag, pressed, and preview state.
- Use `coalesced()` before the current sample when every stylus position matters.
  These samples also use window-logical coordinates; convert each with
  `ctx.to_local`.
- Use `scroll_source()` and `scroll_phase()` to distinguish wheel input,
  touch panning, and trackpad momentum.
- Provide a press or keyboard route for controls otherwise exposed only on hover.

## Related guides

| Task | Guide |
| --- | --- |
| Implement a custom control | [Pointer contract](porting-widgets-to-the-pointer-model.md) |
| Attach handlers or recognizers | [Events and gestures](events-and-gestures.md) |
| Choose target sizes | [Density and targets](density-and-targets.md) |
| Implement scrolling | [Kinetic scrolling](kinetic-scrolling.md) |
| Edit text with a finger | [Touch text editing](text-touch-editing.md) |
| Draw with a stylus | [Ink](ink.md) |
| Support an on-screen keyboard | [Soft keyboard](soft-keyboard.md) |

## Constraints

Touch and pen capabilities vary by backend, device, and compositor. A finger
does not generate hover. Stylus pressure, tilt, proximity, and batched samples
must be treated as capabilities, not universal inputs.

The current Wayland integration cannot initiate native window movement with a
touch serial. Test custom window chrome on the target compositor.
Hardware verification is still required for touch, stylus, trackpad, and screen
reader combinations; headless tests do not establish device support.

## Reference

- [Pointer types](../crates/teksilo-core/src/pointer.rs)
- [Event context](../crates/teksilo-core/src/widget/event_context.rs)
- [Platform source](../crates/teksilo-platform/src/)


## Engineering reference

[Implementation details and review history](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/touch-and-pen.md)
are retained in the repository.
