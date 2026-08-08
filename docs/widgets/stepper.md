<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Stepper

`Stepper` — a modern, embeddable step-flow widget (Material/Ant/Flutter
"stepper"), and `Wizard`, a thin modal launcher built on it.

A stepper shows a **visible step-indicator strip** above (or beside) a
content area driven by a `Switcher`, with a
footer of Back / Skip / Help / Next / Finish controls. It supports linear
and **non-linear** (clickable) navigation, optional + skippable steps, per
step validation gating, a generic chrome slot, and a
`StepperController` handle for programmatic reset / jump / introspection.

# Data flow

The application owns its form state as `Signal`s. A step's content factory
captures clones of those signals (write side); `Step::complete_when`
derives the Next gate from the same signals; and
`Stepper::on_finish` reads them back — plus the `StepperController` for
per-step introspection (`visited` / `skipped`) — to branch on the choices
made. There is no `QVariant` field registry: plain shared signals are the
cross-step channel.

```ignore
#[derive(Clone)]
struct Form { name: Signal<String>, plan: Signal<Plan> }
let form = Form { name: Signal::new(String::new()), plan: Signal::new(Plan::Free) };

Stepper::new()
    .step(Step::new(lit!("Account"))
        .content({ let f = form.clone(); move || TextInput::new().text(f.name.clone()) })
        .complete_when(form.name.map(|n| !n.is_empty())))
    .step(Step::new(lit!("Plan"))
        .content({ let f = form.clone(); move || plan_picker(f.plan.clone()) }))
    .on_finish({ let f = form.clone(); move |_ctx, ctrl| {
        match f.plan.get() { Plan::Free => {/* … */} Plan::Pro => {/* … */} }
        let _ = ctrl.skipped(1);
    }});
```

## Builder methods at a glance

`step`, `steps`, `controller`, `orientation`, `vertical`, `non_linear`, `circle_size`, `chrome`, `chrome_position`, `back_label`, `next_label`, `finish_label`, `skip_label`, `help`, `cancel`, `on_finish`, `enter_advances`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/stepper/index.html)

## `pub enum StepperOrientation`

Indicator-strip orientation for a `Stepper`.

```rust
pub enum StepperOrientation { /* variants */ }
```

### Variants

- **`Horizontal`** — Markers in a row, content below (default).
- **`Vertical`** — Markers in a column on the leading side, content beside.

## `pub enum ChromePosition`

Where the optional chrome slot (banner / sidebar) sits relative to the
stepper body.

```rust
pub enum ChromePosition { /* variants */ }
```

### Variants

- **`Leading`** — Leading column (left in LTR). Forced to `Top` in vertical orientation.
- **`Top`** — Banner above the stepper body.

## `pub struct Stepper`

An embeddable multi-step flow widget. See the `module docs` for the
data-flow pattern and a usage example.

```rust
pub struct Stepper { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty `Stepper`. Append steps with `step` or
`steps` and provide a finish callback with
`on_finish`.

#### `pub fn step(mut self, step: Step) -> Self`

Append a single `Step` definition.

#### `pub fn steps(mut self, steps: impl IntoIterator<Item = Step>) -> Self`

Append multiple `Step` definitions from an iterator.

#### `pub fn controller(mut self, controller: StepperController) -> Self`

Drive the stepper with an externally-held controller (for programmatic
reset / jump / introspection). If omitted, the stepper creates its own.

#### `pub fn orientation(mut self, orientation: StepperOrientation) -> Self`

Set the indicator-strip orientation (horizontal or vertical).

#### `pub fn vertical(mut self) -> Self`

Shorthand for `.orientation(StepperOrientation::Vertical)`.

#### `pub fn non_linear(mut self, non_linear: bool) -> Self`

Allow jumping between steps by clicking their indicators (the markers
become `Role::Tab`). Linear (default) markers are `Role::ListItem`.

#### `pub fn circle_size(mut self, size: f32) -> Self`

Override the marker circle diameter (logical px).

#### `pub fn chrome(mut self, chrome: impl Widget + 'static) -> Self`

A generic chrome widget (banner / sidebar) — the modern replacement for
QWizard's watermark pixmap.

**It lands in the leading column by default**
(`ChromePosition::Leading`, QWizard's watermark slot), i.e. a full
height sidebar. For a *title banner* pair it with
`.chrome_position(ChromePosition::Top)`, or the chrome renders as a
wide sidebar holding a few words.

#### `pub fn chrome_position(mut self, position: ChromePosition) -> Self`

Choose where the optional chrome widget sits relative to the stepper
body. Forced to `ChromePosition::Top` when
`orientation` is `Vertical`.

#### `pub fn back_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Back" button label. Default: "Back".

#### `pub fn next_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Next" button label. Default: "Next".

#### `pub fn finish_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Finish" button label. Default: "Finish".

#### `pub fn skip_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Skip" button label. Default: "Skip".

#### `pub fn help( mut self, label: impl Into<LocalizedString>, action: impl Fn(&mut EventContext, &StepperController) + 'static, ) -> Self`

Add a Help button + callback to the footer.

#### `pub fn cancel( mut self, label: impl Into<LocalizedString>, action: impl Fn(&mut EventContext, &StepperController) + 'static, ) -> Self`

Add a Cancel button + callback to the footer.

#### `pub fn on_finish<R: IntoFinishOutcome>( mut self, action: impl Fn(&mut EventContext, &StepperController) -> R + 'static, ) -> Self`

Called when Finish is activated on the last step. Receives the event
context and the controller (for `skipped` / `visited` introspection);
read collected values from the form signals your steps wrote.

**The callback may refuse.** Its return value goes through the
`IntoFinishOutcome` bridge — `()` always succeeds, while `false`,
`Err(_)`, or `FinishOutcome::Rejected` keep the stepper on the last
step and mark it `StepStatus::Error` (a `Wizard` modal stays
open). This is the
Finish counterpart of `Step::validate_on_next` — for the case where
the commit itself can fail (disk full, name taken, server refused):

```ignore
.on_finish(move |ctx, _ctrl| match create_project(&name.get()) {
    Ok(()) => true,
    Err(e) => { status.set(e.to_string()); false }
})
```

#### `pub fn enter_advances(mut self, enter_advances: bool) -> Self`

Whether pressing <kbd>Enter</kbd> activates the footer's primary button
(Next, or Finish on the last step). Default: `true`.

The key is handled on the **bubble** pass at the stepper root, so a
focused control that wants Enter for itself — a Button, a multi-line
editor, a list row — consumes it first and the stepper never sees it.
A single-line form field lets it through, which is where the "Enter
means Next" contract is expected. Gates apply exactly as they do to a
click: a blocked `complete_when` / `validate_on_next` refuses the same
way.

Turn it off for a step whose body treats Enter as content in a way the
framework cannot see.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip to this stepper. Clears any
previously set rich or composite tooltip.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip identified by a registry key. Clears any
previously set plain or composite tooltip.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip with inline content. Clears any previously set
plain or composite tooltip.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip (arbitrary widget body). Clears any
previously set plain or rich tooltip.
