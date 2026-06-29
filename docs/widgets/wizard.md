<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Wizard

`Wizard` — a thin modal launcher around `Stepper`.

Renders as a button (or a custom `.trigger(...)` widget) that opens a modal
containing a `Stepper` built from the same `Step`s. The modal's Cancel and
a wrapped Finish both dismiss it.

## Builder methods at a glance

`step`, `steps`, `variant`, `enabled`, `non_linear`, `presentation`, `close_behavior`, `size`, `back_label`, `next_label`, `finish_label`, `skip_label`, `cancel_label`, `on_finish`, `trigger`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/stepper/index.html)

## `pub struct Wizard`

A button (or custom trigger) that opens a modal `Stepper`.

`Wizard::new(label)` renders as a `Filled` `Button` whose tap opens a
full-screen modal containing a `Stepper` built from the same `Step`s.
The modal's auto-injected Cancel button and the wrapped Finish both dismiss
it. Override the trigger with `trigger` to use any widget
instead of the default button.

```rust
pub struct Wizard { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Create a wizard trigger button with the given label. The label is also
used as the modal title.

#### `pub fn step(mut self, step: Step) -> Self`

Append a single `Step` to the wizard.

#### `pub fn steps(mut self, steps: impl IntoIterator<Item = Step>) -> Self`

Append multiple `Step`s from an iterator.

#### `pub fn variant(mut self, variant: ButtonVariant) -> Self`

Set the visual variant of the trigger button (default `Filled`).

#### `pub fn enabled(mut self, enabled: bool) -> Self`

Enable or disable the trigger button. When `false`, tapping or
pressing the trigger is a no-op.

#### `pub fn non_linear(mut self, non_linear: bool) -> Self`

Allow jumping between steps by clicking their indicators (the
markers become `Role::Tab`). Default: linear.

#### `pub fn presentation(mut self, presentation: ModalPresentation) -> Self`

Control how the modal is presented (auto, sheet, full-screen, …).

#### `pub fn close_behavior(mut self, close_behavior: ModalCloseBehavior) -> Self`

Control how the modal is dismissed (manual, click-outside, …).

#### `pub fn size(mut self, width: u32, height: u32) -> Self`

Set the preferred modal size in logical pixels. Default 640 × 460.

#### `pub fn back_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Back" button label inside the modal. Default: "Back".

#### `pub fn next_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Next" button label inside the modal. Default: "Next".

#### `pub fn finish_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Finish" button label inside the modal. Default: "Finish".

#### `pub fn skip_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Skip" button label inside the modal. Default: "Skip".

#### `pub fn cancel_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Cancel" button label inside the modal. Default: "Cancel".

#### `pub fn on_finish( mut self, action: impl Fn(&mut EventContext, &StepperController) + 'static, ) -> Self`

#### `pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self`
