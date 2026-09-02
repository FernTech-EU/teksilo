<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Switcher

Switcher — a container that shows exactly one child page at a time.

`Switcher` is the fundamental tab/wizard/step primitive: it owns N child
pages and exposes only the one whose index matches the `Signal<usize>` it
was constructed with. Switching is a signal write — the framework responds
with a relayout that shows the new page and dormantizes all others (excluded
from focus traversal, accessibility tree, hit-test, and paint).

**Lazy mount.** Pages added via `child` /
`children` / `child_boxed`
stay unconstructed until their index is selected for the first time. Once
mounted, the page's subtree persists for the `Switcher`'s lifetime — switching
away then back finds it in the exact state the user left it (focus, scroll
offsets, text-input contents, signal subscriptions). Pages added via
`child_id` are pre-mounted by the caller and treated
eagerly.

The `Switcher` itself reports the maximum natural size across every
currently-mounted page and stretches each placed page to its own bounds —
all pages share the same slot, so the container size never jumps on a switch.

```rust
# use teksilo_widgets::primitives::{Switcher, TextWidget};
# use teksilo_core::signal::Signal;
# use teksilo_i18n::lit;
let page = Signal::new(0_usize);
let _w = Switcher::new(page.clone())
    .child(TextWidget::new(lit!("Step 1")))   // built at startup (index 0 is default)
    .child(TextWidget::new(lit!("Step 2")))   // built on first page.set(1)
    .child(TextWidget::new(lit!("Step 3")));  // built on first page.set(2)
```

## Builder methods at a glance

`capture_child_ids_into`, `child`, `child_boxed`, `child_id`, `children`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/switcher/index.html)

## `pub struct Switcher`

A container that shows exactly one child at a time, driven by a
`Signal<usize>` index.

**Lazy mount.** A page added via `Self::child` / `Self::child_boxed`
/ `Self::children` stays unconstructed until its index is first
selected. Once mounted, the page's subtree persists for the
Switcher's lifetime — switching away then back finds it in the
state the user left it (focus, scroll, text-input contents, …).
Pages added via `Self::child_id` are pre-mounted by the caller
and treated eagerly: no lazy benefit, no semantic change.

The Switcher itself reports the maximum natural size across every
currently-mounted page and stretches each placed child to its own
bounds (top-leading, RTL-aware). Hidden pages keep their subtree
laid out but invisible via per-page `visible_when` bindings.

```rust
# use teksilo_widgets::primitives::{Switcher, TextWidget};
# use teksilo_core::signal::Signal;
# use teksilo_i18n::lit;
let page = Signal::new(0_usize);
let _w = Switcher::new(page.clone())
    .child(TextWidget::new(lit!("Page 0")))   // built at startup
    .child(TextWidget::new(lit!("Page 1")))   // built when page.set(1)
    .child(TextWidget::new(lit!("Page 2")));  // built when page.set(2)
```

```rust
pub struct Switcher { /* fields */ }
```

### Methods

#### `pub fn new(selected: Signal<usize>) -> Self`

Create a `Switcher` driven by `selected`. The initially selected index
is `selected.get()` at build time; page 0 is mounted immediately if that
is the starting value (the most common case).

#### `pub fn capture_child_ids_into(mut self, out: Rc<RefCell<Vec<WidgetId>>>) -> Self`

Capture each mounted page's `WidgetId` into an externally owned
buffer during `build()`. Use when the caller needs to reference
pages after they're added to the arena — e.g. for accessibility
relations like Tab → TabPanel.

The buffer reflects the **currently-mounted** set, not every
declared page. With lazy mount, a page added via `child(...)`
only appears in the buffer once it has been selected for the
first time. Callers that need every id up front should pass
pre-mounted ids via `Self::child_id` instead — those are
eagerly recorded.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Add a child page. The widget stays Boxed until its index is
selected for the first time, then is mounted into the arena
and kept alive across selection changes.

#### `pub fn child_boxed(mut self, widget: Box<dyn Widget>) -> Self`

Add a pre-boxed child page (lazy, same as `Self::child`).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Add a child page by its already-allocated `WidgetId`. Pre-mounted
pages are wired eagerly — the lazy path doesn't apply because
the caller has already paid the construction cost.

#### `pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self`

Add multiple child pages from an iterator (lazy, same as
`Self::child`).
