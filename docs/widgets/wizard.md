<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Wizard

`Wizard` — a thin modal launcher around `Stepper`.

## Public functions

### `Wizard`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(label: impl Into<LocalizedString>)`](#wizard-new) |
| | **Builder methods** |
| `Self` | [`step(step: Step)`](#wizard-step) |
| `Self` | [`steps(steps: impl IntoIterator<Item = Step>)`](#wizard-steps) |
| `Self` | [`variant(variant: ButtonVariant)`](#wizard-variant) |
| `Self` | [`enabled(enabled: impl Into<Prop<bool>>)`](#wizard-enabled) |
| `Self` | [`non_linear(non_linear: bool)`](#wizard-non_linear) |
| `Self` | [`presentation(presentation: ModalPresentation)`](#wizard-presentation) |
| `Self` | [`close_behavior(close_behavior: ModalCloseBehavior)`](#wizard-close_behavior) |
| `Self` | [`size(width: u32, height: u32)`](#wizard-size) |
| `Self` | [`back_label(label: impl Into<LocalizedString>)`](#wizard-back_label) |
| `Self` | [`next_label(label: impl Into<LocalizedString>)`](#wizard-next_label) |
| `Self` | [`finish_label(label: impl Into<LocalizedString>)`](#wizard-finish_label) |
| `Self` | [`skip_label(label: impl Into<LocalizedString>)`](#wizard-skip_label) |
| `Self` | [`cancel_label(label: impl Into<LocalizedString>)`](#wizard-cancel_label) |
| `Self` | [`on_finish<R: IntoFinishOutcome>(action: impl Fn(&mut EventContext, &StepperController) -> R + 'static)`](#wizard-on_finish) |
| `Self` | [`trigger(trigger: impl Widget + 'static)`](#wizard-trigger) |

## Detailed description

Renders as a button (or a custom `.trigger(...)` widget) that opens a modal
containing a `Stepper` built from the same `Step`s. The modal's Cancel and
a wrapped Finish both dismiss it.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/stepper/index.html)

<a id="wizard"></a>

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

<a id="wizard-new"></a>

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Create a wizard trigger button with the given label. The label is also
used as the modal title.

<a id="wizard-step"></a>

#### `pub fn step(mut self, step: Step) -> Self`

Append a single `Step` to the wizard.

<a id="wizard-steps"></a>

#### `pub fn steps(mut self, steps: impl IntoIterator<Item = Step>) -> Self`

Append multiple `Step`s from an iterator.

<a id="wizard-variant"></a>

#### `pub fn variant(mut self, variant: ButtonVariant) -> Self`

Set the visual variant of the trigger button (default `Filled`).

<a id="wizard-enabled"></a>

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Enable or disable the trigger button, statically or reactively.
When disabled, tapping or pressing the trigger is a no-op.

<a id="wizard-non_linear"></a>

#### `pub fn non_linear(mut self, non_linear: bool) -> Self`

Allow jumping between steps by clicking their indicators (the
markers become `Role::Tab`). Default: linear.

<a id="wizard-presentation"></a>

#### `pub fn presentation(mut self, presentation: ModalPresentation) -> Self`

Control how the modal is presented (auto, sheet, full-screen, …).

<a id="wizard-close_behavior"></a>

#### `pub fn close_behavior(mut self, close_behavior: ModalCloseBehavior) -> Self`

Control how the modal is dismissed (manual, click-outside, …).

<a id="wizard-size"></a>

#### `pub fn size(mut self, width: u32, height: u32) -> Self`

Set the preferred modal size in logical pixels. Default 640 × 460.

<a id="wizard-back_label"></a>

#### `pub fn back_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Back" button label inside the modal. Default: "Back".

<a id="wizard-next_label"></a>

#### `pub fn next_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Next" button label inside the modal. Default: "Next".

<a id="wizard-finish_label"></a>

#### `pub fn finish_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Finish" button label inside the modal. Default: "Finish".

<a id="wizard-skip_label"></a>

#### `pub fn skip_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Skip" button label inside the modal. Default: "Skip".

<a id="wizard-cancel_label"></a>

#### `pub fn cancel_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Cancel" button label inside the modal. Default: "Cancel".

<a id="wizard-on_finish"></a>

#### `pub fn on_finish<R: IntoFinishOutcome>( mut self, action: impl Fn(&mut EventContext, &StepperController) -> R + 'static, ) -> Self`

Called when Finish is activated on the last step.

The callback may refuse: its return value goes through
`IntoFinishOutcome` (`()` always succeeds; `false` / `Err(_)` /
`FinishOutcome::Rejected` do not). A rejected finish leaves the modal
**open** on the last step and marks it
`StepStatus::Error` — the right response to
a commit that failed.

<a id="wizard-trigger"></a>

#### `pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self`
