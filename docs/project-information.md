<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Contributing and project information

<!-- BEGIN README: Development environment -->
## Development environment

Working on Teksilo itself needs nothing beyond a stable Rust toolchain and the
system libraries winit, wgpu and arboard link against. `text-document` and
`text-typeset` are ordinary crates.io dependencies, so a clone builds and tests
straight away:

```sh
git clone https://github.com/ferntech-eu/teksilo
cd teksilo
cargo test --workspace
```

On Debian or Ubuntu the system libraries are:

```sh
sudo apt-get install --no-install-recommends \
  build-essential pkg-config \
  libglib2.0-dev libgtk-3-dev libsoup-3.0-dev \
  libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \
  libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev libxcb1-dev libx11-dev
```

### Building against local text-document / text-typeset

The two siblings are developed alongside Teksilo, and a change in one is
usually made together with the change in the other. To build against local
checkouts rather than the published versions, check them out beside this
repository and create `.cargo/config.toml`:

```toml
# Local development overrides: NOT committed (see .gitignore).
#
# Cargo.toml declares `text-document` and `text-typeset` as ordinary crates.io
# dependencies, so a fresh clone and every CI job build without preparation.
# This file redirects them at the sibling checkouts so that edits there are
# picked up by the next `cargo build` here.
#
# Requires ../text-document and ../text-typeset beside this repository.
# Delete this file to build against the published versions instead.
[patch.crates-io]
text-document = { path = "../text-document/crates/public_api" }
text-typeset = { path = "../text-typeset" }
```

The layout it expects:

```
parent/
├── teksilo/          # this repository
├── text-document/
└── text-typeset/
```

That file is gitignored: it is per-machine, and keeping it out of the manifest
is what lets CI and a fresh clone resolve the siblings from crates.io with no
preparation step. When a sibling publishes a new version, bump the version in
`[workspace.dependencies]` in `Cargo.toml`, the patch carries no version of
its own, so a local build will not tell you that you are behind.

The committed `Cargo.lock` must resolve from crates.io without local patches.
Building with the overrides above can add local package entries and remove
registry checksums. Before committing a dependency change, run:

```sh
bash tools/relock-crates-io.sh
```

The script restores registry sources while preserving the tested versions. It
fails if those versions cannot be resolved without the local checkouts. CI
checks the committed lockfile with `cargo metadata --locked`.

`teksilo-analytics-native` is the exception to all of this. It is excluded from
the workspace (it builds protobuf from source, which needs `cmake` and a C++
toolchain) and depends on `teksilo-collector-proto`, which is not published;
that one keeps a plain path dependency on a sibling checkout. A default
`cargo build` never reaches it.
<!-- END README: Development environment -->

<!-- BEGIN README: Architecture stack -->
## Architecture stack

Teksilo is part of a small stack:

- [text-document](https://github.com/ferntech-eu/text-document), the document model. **Required dependency.**
- [text-typeset](https://github.com/ferntech-eu/text-typeset), the typesetting engine. **Required dependency.**
- [Qleany](https://github.com/ferntech-eu/qleany), an architecture materializer that generates Clean Architecture (Vertical Slice variant) in Rust or C++/Qt from a YAML manifest. Independent and optional; pairs naturally with Teksilo for application backends.
<!-- END README: Architecture stack -->

<!-- BEGIN README: Contributing -->
## Contributing

Bug reports and patches are welcome. Please open an issue before sending a non-trivial pull request so we can discuss whether the change fits.

- The framework was built to support FernTech's application portfolio; roadmap priorities are weighted by what those applications need.
- Architectural changes need a design discussion first. Surface-level changes (new builder methods, bug fixes, new examples) are easier.
- Tests are required for new code. The suite runs headlessly, with no GPU or display server.
- The `teksu!` macro and the builder API both need to keep working. New widgets should be usable from both.

No CLA. A DCO sign-off (`git commit -s`) on each commit is enough.
<!-- END README: Contributing -->

<!-- BEGIN README: Authorship and review -->
## Authorship and review

The rules under which Teksilo is built:

1. Direct human communication is written by humans. PR messages, issues, posts, replies: no AI drafting, no AI polish. Common decency.

2. Documentation may be drafted by AI; every line is reviewed by a human. API examples must compile against the current API. Claims are checked, not skimmed.

3. Code, including tests, may be written by AI; every line is reviewed by a human. "Reviewed" means the reviewer understands the change well enough to defend it without the AI in the room. Blind vibe coding is forbidden. Plausible-looking code is not reviewed code.

4. Architecture and public API are human. AI implements within them; it does not design them. The load-bearing surface is specified by a human: the `Widget` trait, `Signal`/`Prop`, the event model, anything downstream apps depend on.

5. Authors and reviewers, both human, are the voluntary bottleneck. Final responsibility rests with them, not the AI. They may use any tool to help, AI included; what is missed lands on them regardless. They take their time; high-speed AI output is not a reason for high-speed work.

6. The human who signs the work owns it, AI or not. Provenance is not disclosed in commits or PR text.

7. No AI has ever been condemned by judges. Only humans and companies have. Stay sharp.
<!-- END README: Authorship and review -->

<!-- BEGIN README: License -->
## License

Mozilla Public License 2.0. See [LICENSE](https://github.com/ferntech-eu/teksilo/blob/main/LICENSE). Teksilo can be used in commercial and closed-source software without restriction; modifications to the Teksilo files themselves must be shared under MPL2 if distributed; application code that merely uses Teksilo is under its own license.
<!-- END README: License -->

<!-- BEGIN README: Commercial support -->
## Commercial support

For priority bug fixes, written support, or an indemnification agreement, contact <support@ferntech.eu>. For everyone else, the issue tracker is the right place.
<!-- END README: Commercial support -->

<!-- BEGIN README: Trademark -->
## Trademark

"Teksilo"™ is a trademark of FernTech, a French company, the subject of French trademark application No. 5292025 (INPI, classes 9 and 42; pending).

The MPL-2.0 source license does not grant trademark rights. Forks and derivative works may use the source code under MPL-2.0 but must adopt a distinct name and distinct branding when distributed (compare Firefox / Iceweasel, Chromium / Chrome).

Nominative use ("built with Teksilo", "Teksilo-compatible widget", articles describing Teksilo) is fine. Distribution packagers may keep the Teksilo name for packages that track upstream releases, including backported fixes, dependency adjustments, and build-system changes; see [TRADEMARKS.md](https://github.com/ferntech-eu/teksilo/blob/main/TRADEMARKS.md) for where that line falls.

See [TRADEMARKS.md](https://github.com/ferntech-eu/teksilo/blob/main/TRADEMARKS.md) for the full policy; for anything it doesn't cover, contact trademarks@ferntech.eu.
<!-- END README: Trademark -->

<!-- BEGIN README: Acknowledgments -->
## Acknowledgments

Teksilo builds on the work of others: AccessKit; winit and wgpu; HarfBuzz (via harfrust), swash, fontdb, etagere, and ICU4X; unicode-bidi; Fluent and the Mozilla l10n team; the published design notes of the Druid, Masonry, and Xilem projects; and SwiftUI's layout protocol. Anthropic and Mistral provided the language models whose code generation contributed substantially under human review.

The theme presets follow design languages published by others: JetBrains' Int UI, which the default light and dark themes are drawn from; Microsoft's Fluent and the WinUI theme resources; Google's Material 3; and Apple's macOS Human Interface Guidelines. Each preset is an independent implementation, and none of those vendors is affiliated with or endorses Teksilo. See [TRADEMARKS.md](https://github.com/ferntech-eu/teksilo/blob/main/TRADEMARKS.md) and [NOTICE](https://github.com/ferntech-eu/teksilo/blob/main/NOTICE).
<!-- END README: Acknowledgments -->

## Maintaining these pages

Shared sections come from the repository README. Edit `README.md`, then run:

```sh
python3 tools/sync_readme_docs.py
python3 tools/sync_readme_docs.py --check
```

Text outside the generated markers is maintained in the documentation pages.
The check runs for documentation pull requests and before site builds.
