// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Make the crate react to the corpus it embeds.
//!
//! `lib.rs` embeds `corpus/index.json` with `include_str!`. Cargo does not
//! track what an embedding macro reads — nothing in the crate's dependency
//! graph names that file — so regenerating the corpus, or writing the vectors
//! into it, leaves cargo believing this crate is fresh.
//!
//! The symptom is a search that answers from the corpus as it was at the last
//! unrelated rebuild: `cargo teksilo build-vectors` writes 2540 embeddings,
//! the next query reports "lexical (BM25)" because the binary still carries a
//! vectorless index, and nothing anywhere says why.
//!
//! One `rerun-if-changed` on the file is the fix. It was a recursive walk when
//! `corpus/` was a whole tree behind `include_dir!`; the corpus is one file
//! now, so this is one line.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=corpus/index.json");
}
