// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The set of density-projected recipe fields that nothing reads, pinned.
//!
//! A `Recipe*Style` carries its dimensions in a data struct whose `for_tokens`
//! projects each one onto the active density, and a theme preset builds that
//! same struct with its own numbers. A field the widget never consults is
//! therefore written twice and rendered never: retuning it in a preset changes
//! nothing on screen, and the density ladder never reaches that dimension. Four
//! such fields were shipped configured by a preset before anyone noticed.
//!
//! [`docs/density-projection-gaps.md`] is the audit artifact — every unread
//! field, its raw-const use site, what it would become at Comfortable and
//! Touch, and a verdict. This re-derives the set from the source and holds the
//! document to it, so that:
//!
//! * a **new** offender — a projected field added with no reader — fails here
//!   instead of joining the pile silently;
//! * a row naming a field that is no longer projected fails, so a rename cannot
//!   leave a stale row behind;
//! * **wiring** one of the known ones does not fail, and needs no edit to this
//!   file. That is the whole point of the page, and it must not cost the person
//!   who does it a test edit.
//!
//! [`docs/density-projection-gaps.md`]: https://github.com/ferntech-eu/teksilo/blob/main/docs/density-projection-gaps.md

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The three density helpers. A struct-literal field whose initialiser mentions
/// one of them is a *projected* field.
const HELPERS: [&str; 3] = ["dp(", "spacing(", "density_min_size("];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("teksilo-widgets lives two levels below the workspace root")
        .to_path_buf()
}

/// Drop `//` comments. These files carry no `//` inside a string literal in a
/// struct literal, which is the only place that would matter.
fn strip_comments(src: &str) -> String {
    src.lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The type a `Self { .. }` literal at `pos` builds: the nearest preceding
/// `impl Name` or `fn f(..) -> Name`.
fn owner_at(src: &str, pos: usize) -> String {
    let head = &src[..pos];
    let mut best: Option<(usize, String)> = None;
    let mut consider = |at: usize, name: &str| {
        if best.as_ref().is_none_or(|(b, _)| at > *b) {
            best = Some((at, name.to_string()));
        }
    };
    for (at, rest) in match_indices_word(head, "impl ") {
        let tail = rest.trim_start();
        // `impl Trait for Type` names the type; `impl Type` names it directly.
        let name = match tail.split_once(" for ") {
            Some((_, after)) => first_ident(after),
            None => first_ident(tail),
        };
        if let Some(name) = name.filter(|n| n.starts_with(char::is_uppercase)) {
            consider(at, &name);
        }
    }
    for (at, rest) in match_indices_word(head, "-> ") {
        if let Some(name) = first_ident(rest)
            && name != "Self"
            && name.starts_with(char::is_uppercase)
        {
            consider(at, &name);
        }
    }
    best.map(|(_, n)| n).unwrap_or_else(|| "?".to_string())
}

fn match_indices_word<'a>(src: &'a str, needle: &str) -> Vec<(usize, &'a str)> {
    src.match_indices(needle)
        .map(|(i, _)| (i, &src[i + needle.len()..]))
        .collect()
}

/// The leading identifier of `src`, ignoring leading whitespace.
fn first_ident(src: &str) -> Option<String> {
    let src = src.trim_start();
    let end = src
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(src.len());
    (end > 0).then(|| src[..end].to_string())
}

/// One projected field: which recipe owns it, its name, and the file it is
/// initialised in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Projected {
    owner: String,
    field: String,
    file: String,
}

impl Projected {
    fn key(&self) -> String {
        format!("{}::{}", self.owner, self.field)
    }
}

/// Every `Name { field: expr, .. }` literal in `src` whose `expr` mentions a
/// density helper, with `Self` resolved to the enclosing type.
fn projected_fields(src: &str, file: &str) -> Vec<Projected> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    for (open, _) in src.match_indices('{') {
        // The literal's type name is the identifier immediately before `{`.
        let Some(name) = ident_ending_at(src, open) else {
            continue;
        };
        if !name.starts_with(char::is_uppercase) {
            continue;
        }
        let Some(close) = matching_close(bytes, open) else {
            continue;
        };
        let body = &src[open + 1..close];
        let owner = if name == "Self" {
            owner_at(src, open)
        } else {
            name
        };
        for part in split_top_level(body) {
            let Some((field, expr)) = part.split_once(':') else {
                continue;
            };
            let field = field.trim();
            if field.is_empty()
                || !field
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                continue;
            }
            let expr: String = expr.split_whitespace().collect::<Vec<_>>().join(" ");
            if HELPERS.iter().any(|h| expr.contains(h)) {
                out.push(Projected {
                    owner: owner.clone(),
                    field: field.to_string(),
                    file: file.to_string(),
                });
            }
        }
    }
    out
}

/// The identifier ending just before `pos` (skipping whitespace), if any.
fn ident_ending_at(src: &str, pos: usize) -> Option<String> {
    let head = src[..pos].trim_end();
    let end = head.len();
    let start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map(|(i, _)| i)?;
    (start < end).then(|| head[start..end].to_string())
}

/// The index of the `}` matching the `{` at `open`, counting every bracket kind
/// so a nested call or index cannot close the literal early.
fn matching_close(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split a struct-literal body on its top-level commas.
fn split_top_level(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in body.char_indices() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

/// Whether `src` consults `.<field>` on something named like a recipe.
///
/// The rule the audit page states, applied verbatim so the page and this test
/// cannot disagree about membership: `recipe.<field>`, optionally `self.`, or
/// with the binding named `rec` / `r` / `m` / `metrics`.
fn has_reader(src: &str, field: &str) -> bool {
    const BINDINGS: [&str; 5] = ["recipe", "rec", "r", "m", "metrics"];
    let needle = format!(".{field}");
    for (i, _) in src.match_indices(&needle) {
        // The character after the field name must not continue an identifier,
        // or `.slot_gap` would match `.slot_gap_extra`.
        if src[i + needle.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let Some(binding) = ident_ending_at(src, i) else {
            continue;
        };
        if BINDINGS.contains(&binding.as_str()) {
            return true;
        }
    }
    false
}

/// Every projected field in `crates/teksilo-widgets/src/styles/`, and whether
/// anything reads it.
fn derive() -> (Vec<Projected>, Vec<Projected>) {
    let root = repo_root();
    let dir = root.join("crates/teksilo-widgets/src/styles");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} is unreadable: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();

    let mut all: Vec<Projected> = Vec::new();
    let mut unread: Vec<Projected> = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .expect("under the root")
            .to_string_lossy()
            .into_owned();
        let src = strip_comments(&std::fs::read_to_string(path).expect("readable"));
        let mut seen = BTreeSet::new();
        for p in projected_fields(&src, &rel) {
            if !seen.insert(p.key()) {
                continue;
            }
            if !has_reader(&src, &p.field) {
                unread.push(p.clone());
            }
            all.push(p);
        }
    }
    (all, unread)
}

/// Every `` `Recipe` | `field` `` pair named in the audit page's tables.
fn documented() -> BTreeSet<String> {
    let path = repo_root().join("docs/density-projection-gaps.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is missing or unreadable: {e}", path.display()));
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("| `") {
            continue;
        }
        let cols: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().trim_matches('`'))
            .collect();
        if cols.len() < 2 {
            continue;
        }
        let (owner, field) = (cols[0], cols[1]);
        if owner.ends_with("Recipe")
            && !field.is_empty()
            && field
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            out.insert(format!("{owner}::{field}"));
        }
    }
    out
}

/// The derivation is not a stub: it finds projected fields, and it tells a read
/// one from an unread one.
///
/// Without this, a derivation that silently stopped matching anything would pass
/// every other test in this file — the failure mode a "the set has not grown"
/// guard is most exposed to.
#[test]
fn the_derivation_can_tell_a_read_field_from_an_unread_one() {
    let src = strip_comments(
        r#"
        impl FixtureRecipe {
            pub fn for_tokens(tokens: &InputTokens) -> Self {
                Self {
                    // Split across lines, as rustfmt leaves a long call.
                    consulted: dp(
                        SOME_CONST,
                        TargetRole::Target,
                        tokens,
                    ),
                    ignored: spacing(OTHER_CONST, tokens),
                    not_projected: PLAIN_CONST,
                }
            }
        }
        impl FooStyle for RecipeFixtureStyle {
            fn make_body(&self) -> f32 {
                self.recipe.consulted
            }
        }
        "#,
    );
    let fields = projected_fields(&src, "fixture.rs");
    let names: Vec<&str> = fields.iter().map(|p| p.field.as_str()).collect();
    assert_eq!(
        names,
        vec!["consulted", "ignored"],
        "the multi-line `dp` and the single-line `spacing` are projected; \
         the plain constant is not",
    );
    assert_eq!(
        fields[0].owner, "FixtureRecipe",
        "`Self` must resolve to the enclosing impl, not stay `Self`",
    );
    assert!(has_reader(&src, "consulted"), "`self.recipe.consulted`");
    assert!(!has_reader(&src, "ignored"), "nothing reads `ignored`");
    assert!(
        !has_reader(&src, "not_projected"),
        "a struct-literal key is not a read — it has no leading dot",
    );

    // And against the real tree: the sweep must find a substantial set, or the
    // subset check below is vacuously true.
    let (all, unread) = derive();
    assert!(
        all.len() > 80,
        "only {} projected fields found — the derivation stopped seeing the \
         recipes",
        all.len(),
    );
    assert!(
        !unread.is_empty(),
        "the unread set came back empty, which would make this file inert",
    );
}

/// A projected field with no reader must be documented.
///
/// This is the guard proper. It fails on a **new** offender and stays green
/// when someone wires a known one, because the claim is a subset and not an
/// equality.
#[test]
fn every_unread_projected_field_is_documented() {
    let (_, unread) = derive();
    let documented = documented();
    let missing: Vec<&Projected> = unread
        .iter()
        .filter(|p| !documented.contains(&p.key()))
        .collect();
    assert!(
        missing.is_empty(),
        "these density-projected recipe fields have no reader and are not in \
         docs/density-projection-gaps.md:\n{}\n\nEither route the widget \
         through the field — see the four worked examples on that page — or \
         add a row for it with its raw-const use site, its ladder and a \
         verdict. A projected field nothing reads is a number a preset writes \
         and no one ever sees.",
        missing
            .iter()
            .map(|p| format!("  {} ({})", p.key(), p.file))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// Every documented row must still name a projected field, so a rename or a
/// deletion cannot leave a stale row standing.
///
/// A row whose field has since been **wired** is not a failure: that is the
/// page's purpose, and making it cost a test edit would discourage the one
/// change this whole file exists to invite. Those rows are printed instead.
#[test]
fn every_documented_row_still_names_a_projected_field() {
    let (all, unread) = derive();
    let projected: BTreeSet<String> = all.iter().map(|p| p.key()).collect();
    let unread: BTreeSet<String> = unread.iter().map(|p| p.key()).collect();

    let mut stale = Vec::new();
    let mut now_read = Vec::new();
    for row in documented() {
        if !projected.contains(&row) {
            stale.push(row);
        } else if !unread.contains(&row) {
            now_read.push(row);
        }
    }
    for row in &now_read {
        println!(
            "NOTE {row} is documented as unread and now has a reader — the row \
             can go."
        );
    }
    assert!(
        stale.is_empty(),
        "docs/density-projection-gaps.md names fields that are no longer \
         density-projected recipe fields at all:\n{}\n\nThey were renamed, \
         moved or deleted; update the page.",
        stale
            .iter()
            .map(|r| format!("  {r}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// The page's **denominator** is the one the derivation produces.
///
/// Only the projected total is pinned, and deliberately. The unread count moves
/// every time someone wires one of these fields, which is the change this page
/// exists to invite — making it cost a test failure would discourage exactly the
/// right thing, so that number is a snapshot the page says is a snapshot, kept
/// honest by the NOTE above. The projected total moves only when a recipe gains
/// or loses a density-projected field, which is when this page's subject has
/// changed and someone should be reading it.
#[test]
fn the_documented_projected_total_matches_the_derivation() {
    let (all, _) = derive();
    let path = repo_root().join("docs/density-projection-gaps.md");
    let text = std::fs::read_to_string(&path).expect("readable");

    const LABEL: &str = "projected fields in scope";
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with(&format!("| {LABEL} |")))
        .unwrap_or_else(|| panic!("the counts table has no `{LABEL}` row"));
    let documented: usize = line
        .trim_matches('|')
        .split('|')
        .nth(1)
        .and_then(|c| c.trim().parse().ok())
        .unwrap_or_else(|| panic!("the `{LABEL}` row carries no number"));

    assert_eq!(
        documented,
        all.len(),
        "a recipe gained or lost a density-projected field: the page says \
         {documented} and the sweep finds {}. Update the denominator, and \
         check whether the new field belongs in one of the tables.",
        all.len(),
    );
}
