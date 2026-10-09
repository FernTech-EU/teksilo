// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The programs the guides in `docs/` show in full, compiled as doctests.
//!
//! A guide's quick start is the code a reader copies first, and `docs/`
//! belongs to no crate, so nothing compiled it: `docs/title-bar.md` went on
//! building its root with `VStack::add_child` after that method was removed.
//!
//! To check a fence, put `<!-- compile-check -->` on the line directly above
//! its opening ```` ```rust ````. The marker is invisible in the rendered book.
//! The build script collects every marked fence, and `cargo test -p
//! teksilo-doc-guard --doc` compiles each one against the umbrella `teksilo`
//! crate with its default features, without running it (`no_run`), since a
//! quick start opens a window. Mark only complete programs that need nothing
//! a reader would not have: a fragment, or a program that needs a non-default
//! feature or a `.ftl` file, stays unmarked.

#[cfg(doctest)]
#[doc = include_str!(concat!(env!("OUT_DIR"), "/compile_checked.md"))]
pub struct CompileChecked;
