<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Stepper

![Stepper preview](img/stepper.png)

`Stepper` — a modern, embeddable step-flow widget (Material/Ant/Flutter
"stepper"), and `Wizard`, a thin modal launcher built on it.

## Public types

| Kind | Name |
| ---: | :--- |
| `enum` | [`StepperOrientation`](#stepperorientation) — Indicator-strip orientation for a `Stepper` |
| `enum` | [`ChromePosition`](#chromeposition) — Where the optional chrome slot (banner / sidebar) sits relative to the stepper body |
| `struct` | [`Stepper`](#stepper) — An embeddable multi-step flow widget |
| `struct` | [`StepperController`](#steppercontroller) — Shared handle controlling a `Stepper` |
| `enum` | [`StepStatus`](#stepstatus) — Lifecycle state of a single step, surfaced in the indicator strip and (for the active step) as `aria-current="step"` |
| `struct` | [`Step`](#step) — One page in a `Stepper` |

## Public functions

### `Stepper`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new()`](#stepper-new) |
| | **Builder methods** |
| `Self` | [`step(step: Step)`](#stepper-step) |
| `Self` | [`steps(steps: impl IntoIterator<Item = Step>)`](#stepper-steps) |
| `Self` | [`controller(controller: StepperController)`](#stepper-controller) |
| `Self` | [`orientation(orientation: StepperOrientation)`](#stepper-orientation) |
| `Self` | [`vertical()`](#stepper-vertical) |
| `Self` | [`non_linear(non_linear: bool)`](#stepper-non_linear) |
| `Self` | [`circle_size(size: f32)`](#stepper-circle_size) |
| `Self` | [`chrome(chrome: impl Widget + 'static)`](#stepper-chrome) |
| `Self` | [`chrome_position(position: ChromePosition)`](#stepper-chrome_position) |
| `Self` | [`back_label(label: impl Into<LocalizedString>)`](#stepper-back_label) |
| `Self` | [`next_label(label: impl Into<LocalizedString>)`](#stepper-next_label) |
| `Self` | [`finish_label(label: impl Into<LocalizedString>)`](#stepper-finish_label) |
| `Self` | [`skip_label(label: impl Into<LocalizedString>)`](#stepper-skip_label) |
| `Self` | [`help(label: impl Into<LocalizedString>, action: impl Fn(&mut EventContext, &StepperController) + 'static)`](#stepper-help) |
| `Self` | [`cancel(label: impl Into<LocalizedString>, action: impl Fn(&mut EventContext, &StepperController) + 'static)`](#stepper-cancel) |
| `Self` | [`on_finish<R: IntoFinishOutcome>(action: impl Fn(&mut EventContext, &StepperController) -> R + 'static)`](#stepper-on_finish) |
| `Self` | [`enter_advances(enter_advances: bool)`](#stepper-enter_advances) |
| `Self` | [`tooltip(text: impl Into<LocalizedString>)`](#stepper-tooltip) |
| `Self` | [`rich_tooltip(key: impl Into<String>)`](#stepper-rich_tooltip) |
| `Self` | [`rich_tooltip_content(content: crate::tooltip::TooltipContent)`](#stepper-rich_tooltip_content) |
| `Self` | [`composite_tooltip(content: impl Widget + 'static)`](#stepper-composite_tooltip) |

### `StepperController`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(step_count: usize)`](#steppercontroller-new) |
| | **Methods** |
|  | [`next()`](#steppercontroller-next) |
|  | [`skip()`](#steppercontroller-skip) |
|  | [`back()`](#steppercontroller-back) |
|  | [`go_to(idx: usize)`](#steppercontroller-go_to) |
|  | [`reset()`](#steppercontroller-reset) |
|  | [`set_status(idx: usize, status: StepStatus)`](#steppercontroller-set_status) |
|  | [`set_visible(idx: usize, visible: bool)`](#steppercontroller-set_visible) |
| `usize` | [`current()`](#steppercontroller-current) |
| `StepStatus` | [`status(idx: usize)`](#steppercontroller-status) |
| `bool` | [`visited(idx: usize)`](#steppercontroller-visited) |
| `bool` | [`skipped(idx: usize)`](#steppercontroller-skipped) |
| `bool` | [`is_visible(idx: usize)`](#steppercontroller-is_visible) |
| `bool` | [`is_reachable(idx: usize)`](#steppercontroller-is_reachable) |
| `Option<usize>` | [`next_reachable(from: usize)`](#steppercontroller-next_reachable) |
| `bool` | [`has_next()`](#steppercontroller-has_next) |
| `usize` | [`step_count()`](#steppercontroller-step_count) |
| `bool` | [`can_back()`](#steppercontroller-can_back) |
| `Signal<usize>` | [`current_step_signal()`](#steppercontroller-current_step_signal) |
| `Signal<u64>` | [`version_signal()`](#steppercontroller-version_signal) |

### `StepStatus`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `bool` | [`is_optional()`](#stepstatus-is_optional) |

### `Step`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(title: impl Into<LocalizedString>)`](#step-new) |
| | **Builder methods** |
| `Self` | [`content<W, F>(factory: F)`](#step-content) |
| `Self` | [`content_boxed(factory: impl Fn() -> Box<dyn Widget> + 'static)`](#step-content_boxed) |
| `Self` | [`supporting_text(text: impl Into<LocalizedString>)`](#step-supporting_text) |
| `Self` | [`status(status: StepStatus)`](#step-status) |
| `Self` | [`optional(optional: bool)`](#step-optional) |
| `Self` | [`complete_when(signal: impl Into<teksilo_core::signal::Prop<bool>>)`](#step-complete_when) |
| `Self` | [`validate_on_next(f: impl Fn() -> bool + 'static)`](#step-validate_on_next) |
| `Self` | [`visible_when(visible: impl Into<teksilo_core::signal::Prop<bool>>)`](#step-visible_when) |

## Detailed description

A stepper shows a **visible step-indicator strip** above (or beside) a
content area driven by a `Switcher`, with a
footer of Back / Skip / Help / Next / Finish controls. It supports linear
and **non-linear** (clickable) navigation, optional + skippable steps, per
step validation gating, a generic chrome slot, and a
`StepperController` handle for programmatic reset / jump / introspection.

### Data flow

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

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![Stepper at Touch density](img/stepper-touch.png)

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/stepper/index.html)

<a id="stepperorientation"></a>

## `pub enum StepperOrientation`

Indicator-strip orientation for a `Stepper`.

```rust
pub enum StepperOrientation { /* variants */ }
```

### Variants

- **`Horizontal`** — Markers in a row, content below (default).
- **`Vertical`** — Markers in a column on the leading side, content beside.

<a id="chromeposition"></a>

## `pub enum ChromePosition`

Where the optional chrome slot (banner / sidebar) sits relative to the
stepper body.

```rust
pub enum ChromePosition { /* variants */ }
```

### Variants

- **`Leading`** — Leading column (left in LTR). Forced to `Top` in vertical orientation.
- **`Top`** — Banner above the stepper body.

<a id="stepper"></a>

## `pub struct Stepper`

An embeddable multi-step flow widget. See the `module docs` for the
data-flow pattern and a usage example.

```rust
pub struct Stepper { /* fields */ }
```

### Methods

<a id="stepper-new"></a>

#### `pub fn new() -> Self`

Create an empty `Stepper`. Append steps with `step` or
`steps` and provide a finish callback with
`on_finish`.

<a id="stepper-step"></a>

#### `pub fn step(mut self, step: Step) -> Self`

Append a single `Step` definition.

<a id="stepper-steps"></a>

#### `pub fn steps(mut self, steps: impl IntoIterator<Item = Step>) -> Self`

Append multiple `Step` definitions from an iterator.

<a id="stepper-controller"></a>

#### `pub fn controller(mut self, controller: StepperController) -> Self`

Drive the stepper with an externally-held controller (for programmatic
reset / jump / introspection). If omitted, the stepper creates its own.

<a id="stepper-orientation"></a>

#### `pub fn orientation(mut self, orientation: StepperOrientation) -> Self`

Set the indicator-strip orientation (horizontal or vertical).

<a id="stepper-vertical"></a>

#### `pub fn vertical(mut self) -> Self`

Shorthand for `.orientation(StepperOrientation::Vertical)`.

<a id="stepper-non_linear"></a>

#### `pub fn non_linear(mut self, non_linear: bool) -> Self`

Allow jumping between steps by clicking their indicators (the markers
become `Role::Tab`). Linear (default) markers are `Role::ListItem`.

<a id="stepper-circle_size"></a>

#### `pub fn circle_size(mut self, size: f32) -> Self`

Override the marker circle diameter (logical px).

<a id="stepper-chrome"></a>

#### `pub fn chrome(mut self, chrome: impl Widget + 'static) -> Self`

A generic chrome widget (banner / sidebar) — the modern replacement for
QWizard's watermark pixmap.

**It lands in the leading column by default**
(`ChromePosition::Leading`, QWizard's watermark slot), i.e. a full
height sidebar. For a *title banner* pair it with
`.chrome_position(ChromePosition::Top)`, or the chrome renders as a
wide sidebar holding a few words.

<a id="stepper-chrome_position"></a>

#### `pub fn chrome_position(mut self, position: ChromePosition) -> Self`

Choose where the optional chrome widget sits relative to the stepper
body. Forced to `ChromePosition::Top` when
`orientation` is `Vertical`.

<a id="stepper-back_label"></a>

#### `pub fn back_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Back" button label. Default: "Back".

<a id="stepper-next_label"></a>

#### `pub fn next_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Next" button label. Default: "Next".

<a id="stepper-finish_label"></a>

#### `pub fn finish_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Finish" button label. Default: "Finish".

<a id="stepper-skip_label"></a>

#### `pub fn skip_label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the "Skip" button label. Default: "Skip".

<a id="stepper-help"></a>

#### `pub fn help( mut self, label: impl Into<LocalizedString>, action: impl Fn(&mut EventContext, &StepperController) + 'static, ) -> Self`

Add a Help button + callback to the footer.

<a id="stepper-cancel"></a>

#### `pub fn cancel( mut self, label: impl Into<LocalizedString>, action: impl Fn(&mut EventContext, &StepperController) + 'static, ) -> Self`

Add a Cancel button + callback to the footer.

<a id="stepper-on_finish"></a>

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

<a id="stepper-enter_advances"></a>

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

<a id="stepper-tooltip"></a>

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip to this stepper. Clears any
previously set rich or composite tooltip.

<a id="stepper-rich_tooltip"></a>

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip identified by a registry key. Clears any
previously set plain or composite tooltip.

<a id="stepper-rich_tooltip_content"></a>

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip with inline content. Clears any previously set
plain or composite tooltip.

<a id="stepper-composite_tooltip"></a>

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip (arbitrary widget body). Clears any
previously set plain or rich tooltip.

<a id="steppercontroller"></a>

## `pub struct StepperController`

Shared handle controlling a `Stepper`.

```rust
pub struct StepperController { /* fields */ }
```

### Methods

<a id="steppercontroller-new"></a>

#### `pub fn new(step_count: usize) -> Self`

A controller for a stepper with `step_count` steps, starting at step 0.

<a id="steppercontroller-next"></a>

#### `pub fn next(&self)`

Advance to the next **reachable** step, recording the current one on
the back-stack. Invisible (`Step::visible_when`)
and `StepStatus::Disabled` steps are stepped over; a no-op when none
remains.

<a id="steppercontroller-skip"></a>

#### `pub fn skip(&self)`

Mark the current (optional) step skipped, then advance like
`next`.

<a id="steppercontroller-back"></a>

#### `pub fn back(&self)`

Return to the most recently visited **reachable** step (the back-stack
top). Entries that became unreachable meanwhile are popped and skipped.
No-op on an empty stack.

<a id="steppercontroller-go_to"></a>

#### `pub fn go_to(&self, idx: usize)`

Jump to step `idx` (non-linear), recording the current step on the
back-stack so `back` returns here. A no-op when `idx` is
out of range or not `reachable`.

<a id="steppercontroller-reset"></a>

#### `pub fn reset(&self)`

Reset to the first reachable step: clears the back-stack, restores the
statuses the stepper was declared with (so a `Disabled` / `Optional`
step keeps its character), and clears visited/skipped flags. Per-step
visibility is app-owned and left untouched.

<a id="steppercontroller-set_status"></a>

#### `pub fn set_status(&self, idx: usize, status: StepStatus)`

Override a step's `StepStatus` (e.g. mark it `Error` after async
validation). Setting `StepStatus::Disabled` takes the step out of the
flow — `next` / `go_to` skip it — but does
**not** move off it if it is the active step.

<a id="steppercontroller-set_visible"></a>

#### `pub fn set_visible(&self, idx: usize, visible: bool)`

Show or hide step `idx`. A hidden step is skipped by
`next` / `back` / `go_to`
and drops out of the indicator strip — the branching-wizard shape
("this step only if you chose X") without maintaining two step lists.

Usually driven declaratively by
`Step::visible_when`; this is the
imperative twin. Hiding the *active* step does not navigate away from
it — hide steps the user has not reached yet.

<a id="steppercontroller-current"></a>

#### `pub fn current(&self) -> usize`

<a id="steppercontroller-status"></a>

#### `pub fn status(&self, idx: usize) -> StepStatus`

<a id="steppercontroller-visited"></a>

#### `pub fn visited(&self, idx: usize) -> bool`

`true` if step `idx` has ever been the active step.

<a id="steppercontroller-skipped"></a>

#### `pub fn skipped(&self, idx: usize) -> bool`

`true` if step `idx` was skipped via `skip`.

<a id="steppercontroller-is_visible"></a>

#### `pub fn is_visible(&self, idx: usize) -> bool`

`true` if step `idx` is visible (see `set_visible`).

<a id="steppercontroller-is_reachable"></a>

#### `pub fn is_reachable(&self, idx: usize) -> bool`

`true` if step `idx` participates in the flow — visible **and** not
`StepStatus::Disabled`.

<a id="steppercontroller-next_reachable"></a>

#### `pub fn next_reachable(&self, from: usize) -> Option<usize>`

The next reachable step after `from`, if any.

<a id="steppercontroller-has_next"></a>

#### `pub fn has_next(&self) -> bool`

`true` if `next` would move — i.e. the active step is not
the last reachable one. The footer shows Next when this holds and
Finish when it does not.

<a id="steppercontroller-step_count"></a>

#### `pub fn step_count(&self) -> usize`

<a id="steppercontroller-can_back"></a>

#### `pub fn can_back(&self) -> bool`

`true` if there is a previously-visited, still-reachable step to
return to.

<a id="steppercontroller-current_step_signal"></a>

#### `pub fn current_step_signal(&self) -> Signal<usize>`

The active-step signal — the stepper's `Switcher` and indicators bind
to it.

<a id="steppercontroller-version_signal"></a>

#### `pub fn version_signal(&self) -> Signal<u64>`

Bumped on every structural mutation; bind at `BindingLevel::Rebuild`.

<a id="stepstatus"></a>

## `pub enum StepStatus`

Lifecycle state of a single step, surfaced in the indicator strip and
(for the active step) as `aria-current="step"`.

Mirrors the modern stepper status model (Ant `wait/process/finish/error`,
Flutter `StepState`): `Upcoming` = not yet reached, `Active` = currently
shown, `Complete` = validated, `Error` = failed validation, `Disabled` =
unreachable, `Optional` = reachable but skippable, `Skipped` = an optional
step the user bypassed.

```rust
pub enum StepStatus { /* variants */ }
```

### Variants

- **`Upcoming`**
- **`Active`**
- **`Complete`**
- **`Error`**
- **`Disabled`**
- **`Optional`**
- **`Skipped`**

### Methods

<a id="stepstatus-is_optional"></a>

#### `pub fn is_optional(self) -> bool`

`true` for `Optional` — the only status that surfaces a Skip button.

<a id="step"></a>

## `pub struct Step`

One page in a `Stepper`.

A step carries a localized `title`, optional `supporting_text`, a content
factory (the body shown when the step is active), and an optional
completion gate. The recommended data-flow pattern: the application owns
its form state as `Signal`s, the content factory binds widgets to those
signals (write side), and `complete_when` derives
the Next gate from the same signals.

```rust
pub struct Step { /* fields */ }
```

### Methods

<a id="step-new"></a>

#### `pub fn new(title: impl Into<LocalizedString>) -> Self`

<a id="step-content"></a>

#### `pub fn content<W, F>(mut self, factory: F) -> Self where W: Widget + 'static, F: Fn() -> W + 'static,`

The body shown while this step is active. The factory may capture
clones of the application's form `Signal`s to read/write step input.

<a id="step-content_boxed"></a>

#### `pub fn content_boxed(mut self, factory: impl Fn() -> Box<dyn Widget> + 'static) -> Self`

The body shown while this step is active, as a **boxed** widget — the
escape hatch for a body whose concrete type varies at runtime.

`content` is generic over one `W: Widget`, and
`Box<dyn Widget>` does not itself implement `Widget`, so a step whose
body branches on app state cannot be expressed as a single `content`
factory. Box each branch instead of duplicating the surrounding
builder:

```ignore
Step::new(lit!("Details")).content_boxed({
    let purpose = purpose.clone();
    move || -> Box<dyn Widget> {
        match purpose.get() {
            Purpose::Novel => Box::new(novel_form()),
            Purpose::Import => Box::new(import_form()),
        }
    }
})
```

<a id="step-supporting_text"></a>

#### `pub fn supporting_text(mut self, text: impl Into<LocalizedString>) -> Self`

Secondary line under the title in the header / indicator.

<a id="step-status"></a>

#### `pub fn status(mut self, status: StepStatus) -> Self`

Set the step's initial `StepStatus`.

<a id="step-optional"></a>

#### `pub fn optional(mut self, optional: bool) -> Self`

Mark the step optional (reachable but skippable — surfaces a Skip
button while active). Equivalent to `.status(StepStatus::Optional)`.

<a id="step-complete_when"></a>

#### `pub fn complete_when(mut self, signal: impl Into<teksilo_core::signal::Prop<bool>>) -> Self`

Reactive Next gate: while this step is active, Next is enabled iff
`signal` is `true`. Derive it from the same form signals the step's
content writes — e.g. `name.map(|n| !n.is_empty())`.

<a id="step-validate_on_next"></a>

#### `pub fn validate_on_next(mut self, f: impl Fn() -> bool + 'static) -> Self`

Imperative validation fallback: checked on the Next click. Returning
`false` blocks navigation. Prefer `complete_when`
where a reactive signal is available.

<a id="step-visible_when"></a>

#### `pub fn visible_when(mut self, visible: impl Into<teksilo_core::signal::Prop<bool>>) -> Self`

Reactive visibility: while `visible` is `false` this step drops out of
the flow — Next / Back / indicator clicks skip it, and its marker is
hidden from the indicator strip (and from AT).

This is how a **branching** wizard is expressed: declare every step
once and gate the conditional ones on the choice that selects them,
instead of maintaining one step list per branch.

```ignore
let purpose = Signal::new(Purpose::Novel);
Stepper::new()
    .step(Step::new(lit!("Purpose")).content(|| purpose_picker()))
    .step(Step::new(lit!("Import source"))
        .visible_when(purpose.map(|p| *p == Purpose::Import))
        .content(|| import_form()))
```

Hiding the step the user is *currently on* does not navigate away from
it — gate steps ahead of the choice, not the one making it.
