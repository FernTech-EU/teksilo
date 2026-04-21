# Rich Clipboard & Default Context Menu — Plan (fern-ui only)

> **Status:** Shipped. Both the clipboard round-trip and the default
> context menu landed, but the context-menu implementation diverges
> from what this document originally proposed. The final shape is:
>
> - Framework's built-in `HandlerSet::context_menu(factory)` handles
>   right-click interception and overlay lifecycle (no manual
>   `context_menu_open` flag, no arena-parenting under the editor,
>   no custom `ContextMenuRoot` wrapper, no `Action`s registered on
>   the editor).
> - Default `MenuItem` closures call `rt_clipboard::*` **directly** —
>   the `Action`/`Intent` indirection proposed here turned out to be
>   incompatible with fern-core's ordering of overlay dismissal vs.
>   `drain_pending_intents`. Reserved `fern.rich_text.*` intent names
>   are still fired for observational purposes.
> - Slot-based replacement via `RichTextEditor::context_menu(factory)`
>   (inherent method shadowing the blanket
>   [`WidgetBuilder::context_menu`](../../crates/fern-core/src/widget_builder.rs)
>   trait method). Opt out with `.default_context_menu(false)`.
>
> The architectural analysis in this document is still accurate for
> the clipboard pieces (phases 1, 1a, 2a, 2b, 2c). The context-menu
> section (phase 3) describes a path that was tried, found to
> regress the read-only navigation tests via arena-parenting, and
> superseded by the design above. See
> [`docs/fern-ui-architecture.md §27.10.16`](../fern-ui-architecture.md)
> for the shipped design.

## Context

Today `RichTextEditor` has **plain-text clipboard only** ([crates/fern-widgets/src/rich_text/clipboard.rs](../../crates/fern-widgets/src/rich_text/clipboard.rs)). Rich format is preserved *within one editor instance* via `rich_clipboard_fragment` + plain-text equality check; paste from any other application lands as plain text. This is why `RichTextEditor` ships **no default context menu** — the widget exposes
[`context_target_at(point)`](../../crates/fern-widgets/src/rich_text.rs#L302) and leaves menu construction to the host. See
[docs/fern-ui-architecture.md §27.10.13](../fern-ui-architecture.md) and
[docs/fern-ui-milestones.md:229](../fern-ui-milestones.md).

Verified before writing this plan:

- **text-document already provides every API we need.**
  - `TextCursor::insert_html(&str) -> Result<()>` — cursor.rs:560, fragment-level HTML insert (wraps `DocumentFragment::from_html` + `insert_fragment`).
  - `TextCursor::insert_fragment(&DocumentFragment)` — cursor.rs:573, already used by today's paste.
  - `TextCursor::selection() -> DocumentFragment` — cursor.rs:623, already used by today's copy.
  - `DocumentFragment::to_html() -> String` — fragment.rs:127. Enables rich copy to external apps.
  - `DocumentFragment::from_html(&str)` — used internally by `insert_html`, available directly if needed.
  - No text-document changes required for the HTML round-trip.
- **arboard 3.6.1 (current lockfile) ships HTML clipboard on every backend we target.**
  - `Clipboard::set_html(html, alt_text)` — [arboard/src/lib.rs:110](https://github.com/1Password/arboard/blob/v3.6.1/src/lib.rs#L110). Writes HTML + plain-text alt in one transaction.
  - `Clipboard::get().html() -> Result<String>` — [arboard/src/lib.rs:200](https://github.com/1Password/arboard/blob/v3.6.1/src/lib.rs#L200). Implemented on X11, Wayland, macOS, Windows.
  - Linux dispatch: [platform/linux/mod.rs:176](...) routes `html()` to either `X11::get_html` or `WlDataControl::get_html`.
- **Linux clipboard subsystem in our build.** Our lockfile pulls `arboard` with `x11rb` but **without `wl-clipboard-rs`** — the `wayland-data-control` feature is off. X11 works natively; under Wayland this falls back to the XWayland clipboard bridge (works for the vast majority of apps, occasional quirks). A separate one-line feature enable in the workspace Cargo.toml is the only lift to get native Wayland support, and is tracked as phase 1a below.
- **RTF is still the long-tail rich format** (Pages, TextEdit, older Windows apps). `NSAttributedString` is a Cocoa *type* that serializes to RTF on the pasteboard — it is not a separate clipboard format. HTML covers 80% of real-world paste sources; RTF is a follow-up.

This plan covers **fern-ui work only**. Every prerequisite on text-document and arboard has been confirmed to exist in the installed versions — **no upstream work needs to land before this plan starts.**

---

## Goals

1. Round-trip real HTML rich content through the clipboard on Linux **now**, using `text/html` + `TextCursor::insert_html` / `DocumentFragment::to_html`.
2. Ship a default context menu on `RichTextEditor` covering **Copy / Cut / Paste / Paste Unformatted / Select All**, overridable by host apps.
3. Ship `EditCommandKind::PasteUnformatted` as an explicit command — independent of rich clipboard work. Users frequently want to strip formatting deliberately; a plain-text paste path should always exist regardless of what MIMEs the system clipboard carries.
4. Keep the door open for RTF follow-up without reshaping the API.

## Non-goals

- **RTF import/export.** Belongs in text-document; out of scope. The `ClipboardBackend` trait design leaves room for a `get_rtf` / `set_rtf` pair later, but no work is done here.
- **Platform clipboard rewrite.** We stay on arboard. No swap to `wl-clipboard-rs` / `x11-clipboard` / direct OS calls.
- **Reworking the `ClipboardBackend` error type.** Stays `Result<_, String>`.

---

## Current state, concretely

- [crates/fern-platform/src/clipboard.rs](../../crates/fern-platform/src/clipboard.rs) — `ClipboardBackend` trait exposes only `get_text` / `set_text` / `has_text`. `ArboardClipboard` wraps `arboard::Clipboard`. `MemoryClipboard` is the in-memory test backend.
- [crates/fern-widgets/src/rich_text/clipboard.rs](../../crates/fern-widgets/src/rich_text/clipboard.rs) — `copy` / `cut` / `paste` free functions driven by keyboard handler; `paste` does rich reuse via plain-text self-round-trip detection. No `paste_unformatted`.
- [crates/fern-widgets/src/rich_text/policy.rs](../../crates/fern-widgets/src/rich_text/policy.rs) — `EditCommandKind` enum has `Copy` / `Cut` / `Paste`. `ClipboardPolicy` gates which commands the filter accepts.
- [crates/fern-widgets/src/rich_text/hit_test.rs](../../crates/fern-widgets/src/rich_text/hit_test.rs) — `ContextTarget::{Selection, Word, Link, Image, Text}` classification already exists and is public.
- No `on_context_menu` handler or built-in menu construction anywhere on the widget.
- Workspace `arboard = "3.4"` at [Cargo.toml:28](../../Cargo.toml); resolves to 3.6.1 in `Cargo.lock`. No features declared — defaults only.

---

## Design

### Phase 1 — Typed-MIME clipboard in fern-platform

Extend `ClipboardBackend` and `ClipboardHandle` with one additional payload: HTML. Keep the API surface minimal — a generic `get(mime) / set(mime, bytes)` is overkill for what we ship now; a named `get_html` / `set_html` pair is clearer and trivially extensible to `get_rtf` later.

```rust
pub trait ClipboardBackend {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
    fn has_text(&mut self) -> bool { ... }

    // New. Default impls let existing implementors compile unchanged.
    fn get_html(&mut self) -> Result<String, String> { Err("unsupported".into()) }
    fn set_html(&mut self, html: &str, plain_fallback: &str) -> Result<(), String> {
        // Default: drop the HTML, write the plain fallback so other apps
        // still see something meaningful. Real backends override to write
        // both payloads in one transaction.
        self.set_text(plain_fallback)
    }
    fn has_html(&mut self) -> bool { false }
}
```

`set_html` takes a plain-text fallback because every real platform clipboard wants both payloads on the same "copy" transaction — if we write HTML alone, pasting into Notepad gets nothing. This matches arboard's `Clipboard::set_html(html, alt_text)` signature exactly and the equivalent requirements of `CF_HTML` + `CF_UNICODETEXT` on Windows and `NSPasteboardTypeHTML` + `NSPasteboardTypeString` on macOS.

**Backends:**

- `MemoryClipboard` — store both `text` and `html` fields; straightforward. `set_html` updates both fields so self-round-trip logic keeps working.
- `ArboardClipboard` — direct passthrough:

  ```rust
  fn get_html(&mut self) -> Result<String, String> {
      self.inner.get().html().map_err(|e| e.to_string())
  }
  fn set_html(&mut self, html: &str, plain: &str) -> Result<(), String> {
      self.inner.set_html(html, Some(plain.into())).map_err(|e| e.to_string())
  }
  fn has_html(&mut self) -> bool {
      self.inner.get().html().map(|s| !s.is_empty()).unwrap_or(false)
  }
  ```

  `has_html` via speculative read is expensive on X11 (round-trip to the selection owner); if we see it fire per-frame in menu-building we'll add caching. For now it only runs when the context menu opens.

**Tests** (colocated in [crates/fern-platform/src/clipboard.rs](../../crates/fern-platform/src/clipboard.rs)):

- `memory_backend_html_roundtrip` — `set_html("<p>a</p>", "a")` then `get_html()` → `"<p>a</p>"`, `get_text()` → `"a"`, `has_html()` → `true`.
- `memory_backend_set_html_updates_plain` — ensures the plain fallback overwrites whatever `set_text` previously put there (prevents stale plain text in self-round-trip detection).
- `handle_html_shared_state` — clone the handle, write HTML on one, read on the other.

### Phase 1a — Opt in to native Wayland clipboard

One-line workspace change: `arboard = { version = "3.4", features = ["wayland-data-control"] }` at [Cargo.toml:28](../../Cargo.toml). Pulls in `wl-clipboard-rs` so arboard uses the Wayland data-control protocol directly instead of going through XWayland. Pure win on GNOME/KDE/Sway sessions. Falls back to X11 when compiled for macOS or Windows (feature is a no-op there).

**Validation:** run [examples/rich_text_editor](../../examples/rich_text_editor/src/main.rs) on a Wayland session, copy from Firefox → paste into the editor, confirm rich formatting survives. If the feature introduces build issues on any target platform, document and drop — X11/XWayland fallback still works. **Not a blocker for shipping the rest of the plan.**

### Phase 2 — Rich paste path in `RichTextEditor`

Extend [rich_text/clipboard.rs](../../crates/fern-widgets/src/rich_text/clipboard.rs):

```rust
pub(crate) fn paste(state: &mut EditorState, ctx: &EventContext) {
    let Some(cb) = ctx.app_state::<ClipboardHandle>() else { return; };

    // 1. Self-round-trip on plain text (unchanged, cheapest path).
    if let Ok(system) = cb.get_text() {
        if state.rich_clipboard_plain.as_deref() == Some(system.as_str())
            && let Some(frag) = state.rich_clipboard_fragment.as_ref()
        {
            let _ = state.cursor.insert_fragment(&frag.clone());
            state.cursor.clear_selection();
            state.pending_text_changed = true;
            return;
        }
    }

    // 2. External HTML → DocumentFragment via TextCursor::insert_html.
    //    text-document handles the parse + insert in one call; no scratch doc.
    if cb.has_html() {
        if let Ok(html) = cb.get_html()
            && !html.is_empty()
            && state.cursor.insert_html(&html).is_ok()
        {
            state.cursor.clear_selection();
            state.pending_text_changed = true;
            return;
        }
    }

    // 3. Plain-text fallback (unchanged behaviour).
    if let Ok(text) = cb.get_text() && !text.is_empty() {
        let _ = state.cursor.insert_text(&text);
        state.cursor.clear_selection();
        state.pending_text_changed = true;
    }
}

pub(crate) fn paste_unformatted(state: &mut EditorState, ctx: &EventContext) {
    // Skip HTML entirely, skip self-round-trip rich reuse. Plain text only.
    let Some(cb) = ctx.app_state::<ClipboardHandle>() else { return; };
    let Ok(text) = cb.get_text() else { return; };
    if text.is_empty() { return; }
    let _ = state.cursor.insert_text(&text);
    state.cursor.clear_selection();
    state.pending_text_changed = true;
}
```

`copy` / `cut` extend symmetrically:

```rust
pub(crate) fn copy(state: &mut EditorState, ctx: &EventContext) {
    if !state.cursor.has_selection() { return; }
    let fragment = state.cursor.selection();
    let plain = fragment.to_plain_text().to_string();
    let html = fragment.to_html();
    if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
        // Write both in one transaction; backends without HTML support
        // fall back to set_text(plain) via the default impl.
        let _ = cb.set_html(&html, &plain);
    }
    state.rich_clipboard_fragment = Some(fragment);
    state.rich_clipboard_plain = Some(plain);
}
```

`DocumentFragment::to_html()` returns `String` (not `Result`), so no error handling needed. The default `set_html` implementation already falls back to `set_text(plain)`, which preserves existing behaviour on backends that haven't opted into HTML — the `MemoryClipboard` + `ArboardClipboard` + any future backend just need their native `set_html` overrides.

**Also add `EditCommandKind::PasteUnformatted`** to [policy.rs](../../crates/fern-widgets/src/rich_text/policy.rs#L12), wire it into the keyboard handler (default binding: Ctrl+Shift+V / ⌘⇧V), and extend `ClipboardPolicy` to gate it (allowed under `Full`, rejected under read-only presets).

**Tests** (colocated in [rich_text/tests.rs](../../crates/fern-widgets/src/rich_text/tests.rs)):

- `paste_html_from_external_source` — seed `MemoryClipboard` with HTML + plain (non-matching stored fragment); paste; assert fragment-level structure (headings, bold spans) ends up in the document.
- `paste_prefers_self_round_trip_over_html` — seed both the stored fragment *and* HTML; paste must use the richer stored fragment.
- `paste_unformatted_ignores_html` — seed HTML with bold spans; `paste_unformatted` inserts plain text only, no formatting.
- `paste_falls_back_to_plain_when_html_unsupported` — backend `has_html()` returns `false`; paste still works with the existing plain-text path.
- `copy_writes_both_text_and_html` — after `copy`, the backend has both payloads and the HTML round-trips through `DocumentFragment::to_html` → `insert_html` losslessly.
- `copy_gracefully_falls_back_on_plain_only_backend` — default trait impl means a backend without `set_html` override gets plain text only, no panic.

### Phase 3 — Default context menu on `RichTextEditor`

New module: [crates/fern-widgets/src/rich_text/context_menu.rs](../../crates/fern-widgets/src/rich_text/context_menu.rs).

```rust
pub struct DefaultContextMenu;

impl DefaultContextMenu {
    /// Build a MenuContext from a ContextTarget + PolicyBundle + clipboard state.
    /// Items are intent-driven so apps can swap bindings or add items via the
    /// standard menu API.
    pub fn for_target(
        target: ContextTarget,
        policy: &PolicyBundle,
        clipboard: Option<&ClipboardHandle>,
        has_selection: bool,
    ) -> MenuContext { ... }
}
```

Items (intents fired via `ctx.send_intent(...)`, following the shortcut/intent/action pattern):

| Item | Enabled when | Intent |
| --- | --- | --- |
| Cut | has selection AND policy allows Cut | `RichTextIntent::Cut` |
| Copy | has selection AND policy allows Copy | `RichTextIntent::Copy` |
| Paste | clipboard has text or html AND policy allows Paste | `RichTextIntent::Paste` |
| Paste Unformatted | clipboard has text AND policy allows PasteUnformatted | `RichTextIntent::PasteUnformatted` |
| — | separator | — |
| Select All | document non-empty AND policy allows SelectAll | `RichTextIntent::SelectAll` |

`RichTextIntent` is a new `#[derive(IntentKind)]` enum owned by the widget module. Matching `Action`s are pre-registered by the widget during `build()` so the default menu works out of the box; apps can override by registering the same intent names at a higher scope.

**Wire-up on the widget:**

```rust
impl RichTextEditor {
    /// Enable (default) or disable the built-in right-click menu.
    /// When disabled, `context_target_at` remains public and apps can
    /// build their own menu.
    pub fn default_context_menu(self, enabled: bool) -> Self { ... }
}
```

Default: **enabled** for both presets, with items filtered by `PolicyBundle`:

- `editor()` preset → all five items visible, greying based on selection / clipboard state.
- `read_only()` preset → Copy + Select All only (matches the policy bundle's clipboard permissions).

The widget installs an `on_pointer_event` handler that, on right-click, calls `context_target_at(point)` and opens the built menu via the existing `MenuContext` infrastructure. Apps opting out (`default_context_menu(false)`) get today's behaviour: no menu, `context_target_at` still public.

**Tests:**

- `default_menu_items_for_selection` — target is `Selection`, editor preset → menu contains Cut/Copy/Paste/PasteUnformatted/SelectAll with expected enabled/disabled states.
- `default_menu_read_only_preset` — read-only preset → Copy + Select All only; Cut/Paste absent.
- `default_menu_respects_opt_out` — `default_context_menu(false)` → right-click emits no menu; `context_target_at` still returns the classification.
- `default_menu_paste_disabled_on_empty_clipboard` — `has_text()` and `has_html()` both false → Paste and PasteUnformatted items are visible but disabled.

### Phase 4 — Example & docs

- Extend [examples/rich_text_editor](../../examples/rich_text_editor/src/main.rs) preamble: remove the "menu bars and context menus — not here yet" sentence, add a one-line "right-click for Cut/Copy/Paste/Paste Unformatted/Select All."
- Update [docs/fern-ui-milestones.md:229](../fern-ui-milestones.md#L229): move "Rich-format clipboard" from "Remaining polish" to "Delivered: HTML round-trip (import via `TextCursor::insert_html`, export via `DocumentFragment::to_html`). RTF deferred."
- Update [docs/fern-ui-architecture.md §27.10.13](../fern-ui-architecture.md): note that HTML round-trips inter-application via arboard, RTF remains deferred, `NSAttributedString` is not a separate format. Mention `ClipboardPolicy::PasteUnformatted`.
- Add a short section to [.claude/CLAUDE.md](../../.claude/CLAUDE.md) under "Rich Text" pointing at `default_context_menu` and `RichTextIntent`.

---

## Work breakdown

| Phase | What | Files | Estimate |
| --- | --- | --- | --- |
| 1 | Typed-MIME clipboard | [fern-platform/clipboard.rs](../../crates/fern-platform/src/clipboard.rs) | 0.5 day |
| 1a | Native Wayland arboard feature | [Cargo.toml](../../Cargo.toml) | 0.25 day (validation time) |
| 2a | `PasteUnformatted` command + keyboard binding | [policy.rs](../../crates/fern-widgets/src/rich_text/policy.rs), [keyboard.rs](../../crates/fern-widgets/src/rich_text/keyboard.rs), [clipboard.rs](../../crates/fern-widgets/src/rich_text/clipboard.rs) | 0.5 day |
| 2b | HTML paste path (direct `cursor.insert_html`) | [clipboard.rs](../../crates/fern-widgets/src/rich_text/clipboard.rs) | 0.5 day |
| 2c | HTML copy path (`DocumentFragment::to_html`) | [clipboard.rs](../../crates/fern-widgets/src/rich_text/clipboard.rs) | 0.5 day |
| 3 | Default context menu + opt-out builder + intents | new `rich_text/context_menu.rs` + [rich_text.rs](../../crates/fern-widgets/src/rich_text.rs) | 1.5 days |
| 4 | Example, milestones doc, architecture doc | `docs/`, `examples/rich_text_editor/` | 0.5 day |

**Total: ~3.5–4 days.** Both earlier uncertainties (text-document fragment-level HTML import; arboard HTML API) resolved favourably, so phases 2b and 2c drop from 1–2 days each to half a day.

**Sequencing:** 1 → 1a (parallel) → 2a → 2b → 2c → 3 → 4. Phase 3 technically only depends on phase 1, but shipping it without 2a/2b/2c gives a menu with broken Paste — not useful. Ship the whole sequence.

## Ship order recommendation

Single coherent slice. With text-document and arboard confirming the full stack, there is no reason to split this plan. The 3.5-day total is low enough to land as one PR.

If splitting is still preferred (review size, risk isolation):

1. **PR 1 (phase 1 + 1a):** clipboard typed-MIME plumbing. No user-visible change, pure infrastructure. Safe to land first.
2. **PR 2 (phase 2 + 3 + 4):** widget behaviour, default menu, docs. The full user-facing slice.

---

## Remaining unknowns

1. **Arboard HTML latency on X11.** Speculative `has_html()` probes hit the selection owner over the X protocol. A single probe per context-menu-open is negligible; if we later surface a permanent "Paste enabled" indicator we'll want cached state. **Mitigation:** the menu builder calls `has_html` once per open. No per-frame probing.
2. **macOS HTML wrapper quirk.** The arboard test at [lib.rs:346-358](arboard/src/lib.rs#L346-L358) notes that macOS wraps pasted HTML in `<html><body>...</body></html>`. `DocumentFragment::from_html` must tolerate this — text-document uses `scraper` / HTML5 semantics, so wrappers parse fine. **Validation:** add a copy-from-Safari manual test to the phase 4 checklist.
3. **Ctrl+Shift+V collision.** De-facto standard for Paste Unformatted (Firefox, VS Code, JetBrains, LibreOffice). Before landing phase 2a, grep [crates/fern-core/src/shortcut.rs](../../crates/fern-core/src/shortcut.rs) and [crates/fern-widgets/src/rich_text/keyboard.rs](../../crates/fern-widgets/src/rich_text/keyboard.rs) for any existing binding.

## Non-risks

- **Backwards compatibility.** Existing `ClipboardBackend` implementors get default method bodies — no breakage. `paste` keeps its existing signature and falls back to plain text when HTML is absent.
- **Headless tests.** `MemoryClipboard` gains HTML support, so existing widget tests are unaffected. New tests use the same infrastructure.
- **No new crate dependencies.** arboard and text-document are already in the tree at the required versions.
- **No text-document changes.** `insert_html`, `to_html`, `from_html` are all on the existing public API surface.
