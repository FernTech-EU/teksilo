<!-- SPDX-License-Identifier: LicenseRef-Teksilo-Trademark-Policy -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- This file is not part of the MPL-2.0 licensed source. See "About this document" below. -->

# Teksilo Trademark Policy

**Version 1.0. Last revised: 2026-08-28.**

"Teksilo"™ identifies this project, maintained by FernTech. This document says what you can do with the name without asking, and what you can't. It covers the Teksilo name and any Teksilo logo or wordmark.

## What's fine without asking

- Referring to the project by name: articles, talks, tutorials, comparisons, bug reports.
- Nominative statements: "built with Teksilo", "Teksilo-compatible widget", "a Teksilo plugin for X".
- Naming your own crate or package so that it describes compatibility or extension, as long as it doesn't read as the official project or as FernTech-published. The `teksilo-` prefix on crates.io is reserved for crates we publish; for your own, prefer a form that puts your name first (`foo-teksilo`, `foo-for-teksilo`) and say what it is in the crate description.
- Forking the source. The MPL-2.0 license already grants that; this policy doesn't take it away.

## Distribution and ecosystem packaging

Packagers for operating-system distributions and language ecosystems (Debian, Fedora, Arch, Nixpkgs, Homebrew, Guix, and the like) may keep the Teksilo name for packages that track upstream releases. That includes the changes packaging normally requires:

- backported security and bug fixes;
- adjusted dependency bounds, de-vendoring, unbundling;
- build-system, path, and packaging-metadata changes;
- patches carried while an upstream release is pending.

What needs a distinct name is a package that changes Teksilo's behaviour or public API, adds or removes features, or ships from a fork rather than from upstream releases.

If you maintain such a package and aren't sure which side of that line your patch set falls on, write to us rather than renaming preemptively: <trademarks@ferntech.eu>. We would rather answer the question than lose the package.

## What needs a distinct name

Distributing a modified or forked version of this project as "Teksilo", or under a name confusingly close to it, is not permitted outside the packaging allowance above. Give your fork its own name and its own branding. This is the Firefox/Iceweasel, Chromium/Chrome situation: the code is free to fork, but a modified build shipped under the original name lets its bugs and decisions get attributed to a project that didn't make them.

More generally, don't do anything with the name or FernTech's branding that would let a reasonable user believe your project, fork, or service is official, affiliated with, or endorsed by FernTech when it isn't.

## Trademark status

"Teksilo" is the subject of French trademark application No. 5292025, filed with the INPI by FernTech (classes 9 and 42). The application is pending; this document will be updated when it is registered.

## About this document

Unlike the source code, this policy is not licensed under the MPL-2.0. Forks and derivative works may copy the source; they should write their own trademark policy for their own name.

## Anything else

If your use isn't clearly covered above, ask first: <trademarks@ferntech.eu>. This policy may be revised as the project evolves; revisions are not retroactive, and the version in effect is the one stated at the top of this file.
