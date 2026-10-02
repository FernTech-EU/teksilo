<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Examples and tooling

<!-- BEGIN README: Tooling: `cargo teksilo` -->
## Tooling: `cargo teksilo`

Four things Teksilo relies on do not travel with the crate: the guides under
`docs/`, the 56 worked examples (every one is `publish = false`), the agent
skill, and the harness for driving a running app. That leaves anyone building
*with* Teksilo, and any AI assistant helping them, working from less than the
framework actually documents.

One install fixes it, and everything it answers is matched to the Teksilo
version **your** `Cargo.lock` resolved:

```sh
cargo install cargo-teksilo
cargo teksilo setup             # run inside your app
```

```sh
cargo teksilo symbol Button                  # exact public API, for your version
cargo teksilo symbol --crate data ListModel  # 32 crates are queryable
cargo teksilo search "make a list scrollable"
cargo teksilo probe                          # automation harness -> scripts/
```

`setup` writes the probe harness into `scripts/teksilo_probe/` and installs the
skill (or a condensed brief, in that agent's own format) for every coding agent
already configured in the project; `--user` installs the skill into your home
directory instead (the harness, being project code, still goes into the
project). It never edits your own content (a shared file such as `AGENTS.md` gets
a marked region of its own), and it refuses outright rather than answering for a
version you do not have, a wrong answer about a framework reads exactly like a
right one.

If ONNX Runtime will not build on your platform, `cargo install cargo-teksilo
--no-default-features` gives the same tool with lexical search instead of hybrid;
everything else is unchanged.

Full reference: [agent-tooling.md](agent-tooling.md).
<!-- END README: Tooling: `cargo teksilo` -->

<!-- BEGIN README: Running the demos -->
## Running the demos

```sh
git clone https://github.com/ferntech-eu/teksilo
cd teksilo
cargo run -p simple-button      # the minimal app
cargo run -p widget-catalog     # browse every widget
cargo run -p file-dialogs       # native file dialogs
```

Inside a checkout, the extractor `cargo teksilo symbol` wraps is also directly
available:

```sh
python3 tools/extract_widget_api.py --list
python3 tools/extract_widget_api.py button calendar tree_view
```
<!-- END README: Running the demos -->

See [automation MCP](automation-mcp.md) for driving applications and
[the debug inspector](inspector.md) for inspecting widgets at runtime.
