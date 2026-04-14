# FernUI i18n showcase

Phase H demo from the §12 internationalization implementation. Run it:

```bash
cargo run -p internationalization
```

The window opens with English text. Click **Français** or **العربية** to
switch locales — the UI updates live. Arabic flips layout direction: the
**Leading / Trailing** row at the bottom visibly reverses its children,
and the direction note at the top updates accordingly.

## What it demonstrates

- `tr!(...)` calls validated at **compile time** against
  [`locales/en-US.ftl`](locales/en-US.ftl)
- Zero-arg and arg-bearing messages (`tr!(greeting(name = name))`)
- Three compiled-in locales (`en-US`, `fr-FR`, `ar-SA`)
- RTL layout flip via `rtl_from_locale` + `HAlignment::resolve(rtl)`
- `CommandContext::set_locale(...)` broadcasting to every tree
- `fern_widgets::framework_locales()` registration so a11y strings
  like *Dialog* and *Status* are available in fr-FR (Phase E / Step 4)

## Translator hot-reload

The translator workflow from architecture §12.6 lets someone edit a
`.ftl` file and see the running application update within ~100ms, no
rebuild or restart. Each `--translation-dev LOCALE=PATH` flag registers
a runtime override — the compiled-in bundle for that locale is
replaced by the file on disk, and a `notify`-backed file watcher
observes the path for modifications.

### 1. Start the application with an override

```bash
# Copy the compiled-in French to a writable location first
cp examples/internationalization/locales/fr-FR.ftl /tmp/fr.ftl

# Run the demo with that path registered as an override
cargo run -p internationalization -- --translation-dev fr-FR=/tmp/fr.ftl
```

The app starts normally. Click **Français** to see the current
translations.

### 2. Edit the file while the app is running

In another terminal:

```bash
sed -i 's/Bonjour, { $name } !/Salut { $name } !/' /tmp/fr.ftl
```

Switch focus back to the running app — the greeting updates from
*Bonjour, Alice !* to *Salut Alice !*. No restart, no rebuild, no
composite rebuild: only the i18n version signal is bumped and every
`LocalizedString::to_signal()` observer re-resolves.

### 3. Iterate as much as you want

Every save fires the watcher; every reload is free (the bundle parse
is a few milliseconds for a typical `.ftl`). Save a syntactically
invalid file by mistake and the framework logs the parse error and
keeps the previous bundle intact — the running UI stays on the last
good translation.

### 4. Multiple locales at once

Pass `--translation-dev` once per locale:

```bash
cargo run -p internationalization -- \
    --translation-dev fr-FR=/tmp/fr.ftl \
    --translation-dev ar-SA=/tmp/ar.ftl
```

Editing either file reloads that locale's bundle independently.

## Constraints

- **Same-locale reloads only.** The watcher never changes the active
  locale or layout direction. Switching from French to Arabic still
  goes through the language selector (which emits a `LangCmd` →
  `CommandContext::set_locale` → composite rebuild). Hot-reload is
  purely for content changes within a locale.
- **File must exist at startup.** If a path is missing, the watcher
  logs and skips that entry — the compile-in bundle stays in place
  for that locale.
- **Editor-save patterns.** The watcher watches the *parent directory*
  non-recursively and filters by file path, so atomic
  write-then-rename saves (vim, most IDEs) work alongside direct
  in-place writes.

## Files

| File | Purpose |
|---|---|
| [`src/main.rs`](src/main.rs) | Root widget, `LangCmd` enum, `parse_translation_dev_flags`, `FernAppBuilder` wiring |
| [`locales/en-US.ftl`](locales/en-US.ftl) | Source language — validated by `tr!` at compile time |
| [`locales/fr-FR.ftl`](locales/fr-FR.ftl) | French runtime translation |
| [`locales/ar-SA.ftl`](locales/ar-SA.ftl) | Arabic runtime translation (triggers RTL) |

## Reference

- Architecture §12 — full i18n design
- Architecture §12.6 — `runtime_override` + translator workflow
- Architecture §12.7 — why hot-reload must not trigger a composite rebuild
- Architecture §12.13 — framework bundle design, `tr_widget!`
