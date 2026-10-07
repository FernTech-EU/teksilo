<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Architecture

Teksilo retains a widget tree and runs event, layout, accessibility, and paint
passes over it. This page describes the boundaries relevant to custom widgets
and framework maintenance. Start with [your first application](first-application.md)
for application setup.

## Widget model

A `Widget` can compose children in `build`, draw in `paint`, or do both.
Layout uses a proposal-and-response model: parents propose sizes, children
respond, and parents place them. The widget arena owns mounted widgets and
provides stable identifiers for their lifetime.

Use [layout primitives](layout-primitives.md) for composition and
[events and gestures](events-and-gestures.md) for input handling.
[`Button`](../crates/teksilo-widgets/src/button.rs) is a concrete example of a
composed control with theme, focus, and accessibility behavior.

## State and updates

`Signal<T>` stores observable state. `map` derives a value; `Prop<T>` lets a
builder accept a static value or a signal. Bindings request repaint, layout, or
rebuild according to what changed. Keep observer handles alive while their
subscriptions are needed.

Handlers use `EventContext` to request structural changes. Deferred operations
run when the framework can safely mutate the tree. A rebuild can replace
widget IDs; do not retain an ID beyond the mounted widget's lifetime.

Use models for changing collections, `visible_when` for conditional visibility,
and the appropriate view for large collections. See [data models](data-models.md).

## Input and accessibility

Events follow preview and bubble dispatch. Pointer identity, capture, gesture
arbitration, and cancellation belong to the framework. Custom controls must
follow the [pointer contract](porting-widgets-to-the-pointer-model.md).

Widgets declare accessibility information beside their layout and paint methods.
The framework builds an AccessKit tree and routes its actions to widgets.
Applications can adjust labels, relationships, roles, and subtree behavior
through [accessibility overrides](accessibility-overrides.md).

Keyboard bindings resolve through the [shortcut, intent, and action
pipeline](shortcut-intent-action.md). Input handling and accessibility support
still require application-level testing.

## Rendering and text

Widgets paint through the canvas abstraction. The renderer uses wgpu; platform
integration owns windows and the event loop. Idle rendering is demand-driven.
Animations and asynchronous activity must participate in wake-up scheduling.
See [idle and animation](idle-and-animation.md).

The text stack combines `text-document` for document operations and
`text-typeset` for shaping and layout. `teksilo-text` connects that stack to the
canvas. Application code can access the document model directly.

## Crate boundaries

| Layer | Main crates |
| --- | --- |
| Values, colors, and theme tokens | `teksilo-tokens` |
| Drawing interface, live pictures, and waking a window from any thread | `teksilo-canvas` |
| Tree, state, layout, events, and styles | `teksilo-core` |
| Reactive collections | `teksilo-data` |
| Controls and specialized views | `teksilo-widgets`, `teksilo-charts`, `teksilo-scene` |
| Text integration | `teksilo-text` |
| GPU and operating-system integration | `teksilo-render`, `teksilo-platform` |
| Application lifecycle | `teksilo-app` |
| Application-facing exports and features | `teksilo` |

The core tree can be exercised without a window or GPU. Platform behavior and
rendering need separate integration checks.

## Common extension points

| Task | Reference |
| --- | --- |
| Change appearance | [Styling](styling-system.md), [reactive themes](reactive-theme.md) |
| Add translation | [Internationalization](i18n.md) |
| Add windows | [Multi-window](multi-window.md) |
| Persist state | [Settings](settings.md) |
| Run asynchronous work | [Async](async.md) |
| Inspect a running application | [Inspector](inspector.md), [automation](automation-mcp.md) |
| Use declarative syntax | [Macro reference](teksu-macro-reference.md) |

## Constraints

Keep widget state on the UI thread. Transfer background results through the
supported event or async interfaces. Treat painting, layout, and observers as
separate phases; avoid mutating data while holding a borrow that callbacks need.

The public API is pre-1.0. Verify extension code against the version used by the
application. See [status and limitations](status-and-limitations.md).

## Reference

- [Widget trait](../crates/teksilo-core/src/widget.rs)
- [Widget tree](../crates/teksilo-core/src/widget_tree.rs)
- [Event context](../crates/teksilo-core/src/widget/event_context.rs)


## Engineering reference

[Implementation details and review history](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/architecture.md)
are retained in the repository.
