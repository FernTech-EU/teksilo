---
name: extract-widget-api
description: Extract the public API and inline documentation of a fern-widgets widget. Use when the user wants to see a widget's public surface (struct, builder methods, enums, module doc) without opening the file, or asks things like "show me Button's API", "what are HStack's builder methods", "list fern-widgets", or "/extract-widget-api <Widget>". Also use when packing widget docs into context for a downstream task.
user_invocable: true
---

# extract-widget-api

Run [tools/extract_widget_api.py](../../../tools/extract_widget_api.py) to emit a widget's public API with `///` docs and its `//!` module header. Skips `impl Widget for Foo` trait plumbing and `pub(crate)` items.

## Usage

Run from the repository root via Bash:

```bash
python3 tools/extract_widget_api.py <Widget> [<Widget> ...]
```

Widget names accept type names (`Button`, `HStack`, `Dialog`) or module names (`button`, `hstack`), case-insensitive.

### Common invocations

```bash
python3 tools/extract_widget_api.py --list                        # List every widget file
python3 tools/extract_widget_api.py Button                        # One widget
python3 tools/extract_widget_api.py Button HStack Dialog          # Several at once
python3 tools/extract_widget_api.py --all                         # Every widget (large)
python3 tools/extract_widget_api.py Button -f json                # JSON for tooling
python3 tools/extract_widget_api.py Button -o /tmp/button.md      # Write to file
python3 tools/extract_widget_api.py Button -f text                # Plain text, no markdown
```

## Argument parsing

If the user invoked `/extract-widget-api` with arguments, pass them straight through:

- `/extract-widget-api Button` → `python3 tools/extract_widget_api.py Button`
- `/extract-widget-api HStack VStack` → `python3 tools/extract_widget_api.py HStack VStack`
- `/extract-widget-api --list` → `python3 tools/extract_widget_api.py --list`

If no arguments were given, ask the user which widget(s) they want, or suggest `--list`.

## Output handling

- For 1–2 widgets, print the full output directly to the user — it's already markdown-formatted.
- For `--all` or more than ~3 widgets, write to a file with `-o` and tell the user the path, since the dump is large.
- If the user asks for JSON (for tooling or downstream LLM context), use `-f json`.

## Errors

- Unknown widget names exit with code 2 and print `Did you mean: X, Y, Z?` suggestions. Relay the suggestions to the user.
- If the script reports "fern-widgets src not found", you're likely not at the repo root — `cd` into `/home/cyril/Devel/fern-ui` first (or use an absolute path to the script).
