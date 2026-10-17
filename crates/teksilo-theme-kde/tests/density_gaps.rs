// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! No recipe this preset builds sets a field the widget does not render.
//!
//! `engineering/docs/density-projection-gaps.md` lists the density-projected
//! recipe fields that the widget composes from the recipe module's constant
//! instead of the style: a preset that writes one writes a number nobody
//! renders. Its sections 1 to 3 are the unrendered fields (section 4 lists
//! fields the page's text rule misses but a reader can see are read).
//!
//! Two checks hold this crate to that list, both driven by the page itself so
//! a row added there is covered here with no edit:
//!
//! - every struct literal of a recipe type in this crate's `src/`, the
//!   segmented control's among them, leaves out every field the page lists
//!   for that recipe;
//! - every factory that builds one of those recipes gives each listed field
//!   the stock recipe's value at every density.
//!
//! This file reads the page from the repository, so it is left out of the
//! published package (the `exclude` list in `Cargo.toml`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use teksilo_theme_kde::styles::recipes;
use teksilo_theme_kde::styles::standard_item::breeze_standard_item_recipe_for;
use teksilo_theme_kde::typography::breeze_typography;
use teksilo_tokens::{InputTokens, TargetDensity};
use teksilo_widgets::styles::{
    AvatarRecipe, BannerRecipe, CalendarRecipe, SearchFieldRecipe, SegmentedControlRecipe,
    StandardItemRecipe, ToastRecipe,
};

const GAPS_MD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../engineering/docs/density-projection-gaps.md"
));

const DENSITIES: [TargetDensity; 3] = [
    TargetDensity::Compact,
    TargetDensity::Comfortable,
    TargetDensity::Touch,
];

/// The text between two backticks of a table cell, if the cell is one.
fn ticked(cell: &str) -> Option<&str> {
    cell.trim().strip_prefix('`')?.strip_suffix('`')
}

/// The `(recipe, field)` rows of the page's sections 1 to 3, with the
/// section each came from.
fn unrendered() -> Vec<(String, String, String)> {
    let sections = GAPS_MD
        .split("\n## 4.")
        .next()
        .expect("the page has a section 4");
    let mut section = String::new();
    let mut rows = Vec::new();
    for line in sections.lines() {
        if let Some(h) = line.strip_prefix("## ").or(line.strip_prefix("### ")) {
            section = h.split_whitespace().next().unwrap_or("").to_string();
            continue;
        }
        let cells: Vec<&str> = line.split('|').collect();
        if cells.len() < 4 || !cells[0].trim().is_empty() {
            continue;
        }
        if let (Some(recipe), Some(field)) = (ticked(cells[1]), ticked(cells[2]))
            && recipe.ends_with("Recipe")
        {
            rows.push((section.clone(), recipe.to_string(), field.to_string()));
        }
    }
    rows
}

/// The page parses: every section 1 to 3 table yields rows, and the
/// segmented control's are among them.
#[test]
fn the_page_lists_unrendered_fields_in_every_section() {
    let rows = unrendered();
    let sections: BTreeSet<&str> = rows.iter().map(|(s, _, _)| s.as_str()).collect();
    for s in ["1.", "2.", "3a.", "3b."] {
        assert!(
            sections.contains(s),
            "section {s} yields no row; parsed {sections:?}"
        );
    }
    assert!(
        rows.iter()
            .any(|(_, r, f)| r == "SegmentedControlRecipe" && f == "height"),
        "the segmented control's height is listed"
    );
}

fn strip_comments(src: &str) -> String {
    src.lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The identifier ending just before `pos`, skipping whitespace, and the text
/// before it.
fn ident_before(src: &str, pos: usize) -> Option<(&str, &str)> {
    let head = src[..pos].trim_end();
    let start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map(|(i, _)| i)?;
    Some((&head[start..], &head[..start]))
}

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

fn split_top_level(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in body.char_indices() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

/// Every `FooRecipe { field: .. }` literal in `src`: the recipe and the
/// fields it initialises. A type named after `struct`, `impl`, `for`, `enum`
/// or a return arrow opens a declaration or a body, not a literal.
fn recipe_literals(src: &str) -> Vec<(String, Vec<String>)> {
    let src = strip_comments(src);
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    for (open, _) in src.match_indices('{') {
        let Some((name, before)) = ident_before(&src, open) else {
            continue;
        };
        if !name.ends_with("Recipe") || !name.starts_with(char::is_uppercase) {
            continue;
        }
        let prev = before.trim_end();
        if ["struct", "impl", "for", "enum", "->"]
            .iter()
            .any(|k| prev.ends_with(k))
        {
            continue;
        }
        let Some(close) = matching_close(bytes, open) else {
            continue;
        };
        let fields = split_top_level(&src[open + 1..close])
            .into_iter()
            .filter_map(|part| {
                let (field, _) = part.split_once(':')?;
                let field = field.trim();
                (!field.is_empty()
                    && field
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
                .then(|| field.to_string())
            })
            .collect();
        out.push((name.to_string(), fields));
    }
    out
}

fn sources() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("a readable source directory") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).expect("readable source");
                out.push((path, text));
            }
        }
    }
    out
}

/// No recipe literal in this crate initialises a field the page lists as
/// unrendered for that recipe.
#[test]
fn no_recipe_literal_sets_an_unrendered_field() {
    let rows = unrendered();
    let mut literals = 0;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut offenders = Vec::new();
    for (path, text) in sources() {
        for (recipe, fields) in recipe_literals(&text) {
            literals += 1;
            seen.insert(recipe.clone());
            for field in fields {
                if rows.iter().any(|(_, r, f)| *r == recipe && *f == field) {
                    offenders.push(format!("{}: {recipe}::{field}", path.display()));
                }
            }
        }
    }
    // The scan finds the literals it is meant to judge, the segmented
    // control's included.
    assert!(literals >= 10, "only {literals} recipe literals found");
    for recipe in [
        "SegmentedControlRecipe",
        "CalendarRecipe",
        "SearchFieldRecipe",
        "StandardItemRecipe",
    ] {
        assert!(seen.contains(recipe), "no {recipe} literal found: {seen:?}");
    }
    assert!(
        offenders.is_empty(),
        "unrendered fields set:\n{}",
        offenders.join("\n")
    );
}

/// The scan sees a field initialised in a literal, and none in a
/// declaration or a function body.
#[test]
fn the_scan_finds_fields_in_literals_only() {
    let src = "pub struct FooRecipe { pub height: f32 }\n\
               fn make() -> FooRecipe {\n\
                   let x: f32 = 1.0;\n\
                   FooRecipe {\n\
                       // a comment, with a comma: height\n\
                       height: dp(x, TargetRole::Target, t),\n\
                       width: 2.0,\n\
                       ..FooRecipe::for_tokens(t)\n\
                   }\n\
               }";
    assert_eq!(
        recipe_literals(src),
        vec![(
            "FooRecipe".to_string(),
            vec!["height".to_string(), "width".to_string()]
        )]
    );
}

/// One recipe's fields, by name.
macro_rules! fields {
    ($r:expr; $($f:ident),* $(,)?) => {
        vec![$((stringify!($f), $r.$f)),*]
    };
}

type Fields = Vec<(&'static str, f32)>;

/// Every factory of this crate that builds a recipe the page lists, with the
/// stock recipe's values at the same density: `(recipe, preset, stock)`.
fn factories(tokens: &InputTokens) -> Vec<(&'static str, Fields, Fields)> {
    let avatar = |r: AvatarRecipe| fields!(r; size_small, size_medium, size_large, size_x_large);
    let banner = |r: BannerRecipe| fields!(r; title_description_gap);
    let calendar = |r: CalendarRecipe| fields!(r; cell_size, header_height, outer_padding, section_gap, cell_gap);
    let search = |r: SearchFieldRecipe| fields!(r; row_height, input_panel_gap, panel_padding);
    let segmented =
        |r: SegmentedControlRecipe| fields!(r; height, padding_horizontal, padding_vertical);
    let toast = |r: ToastRecipe| fields!(r; title_body_gap, body_actions_gap);
    let item = |r: StandardItemRecipe| fields!(r; tree_indent_step, label_subtitle_gap, slot_gap, subtitle_slot_gap);
    vec![
        (
            "AvatarRecipe",
            avatar(recipes::breeze_avatar_style_for(tokens).recipe),
            avatar(AvatarRecipe::for_tokens(tokens)),
        ),
        (
            "BannerRecipe",
            banner(recipes::breeze_banner_style_for(tokens).recipe),
            banner(BannerRecipe::for_tokens(tokens)),
        ),
        (
            "CalendarRecipe",
            calendar(recipes::breeze_calendar_style_for(tokens).recipe),
            calendar(CalendarRecipe::for_tokens(tokens)),
        ),
        (
            "SearchFieldRecipe",
            search(recipes::breeze_search_field_style_for(tokens).recipe),
            search(SearchFieldRecipe::for_tokens(tokens)),
        ),
        (
            "SegmentedControlRecipe",
            segmented(recipes::breeze_segmented_control_style_for(tokens).recipe),
            segmented(SegmentedControlRecipe::for_tokens(tokens)),
        ),
        (
            "ToastRecipe",
            toast(recipes::breeze_toast_style_for(tokens).recipe),
            toast(ToastRecipe::for_tokens(tokens)),
        ),
        (
            "StandardItemRecipe",
            item(breeze_standard_item_recipe_for(
                tokens,
                &breeze_typography(),
            )),
            item(StandardItemRecipe::for_tokens(tokens)),
        ),
    ]
}

/// Every listed field of a recipe this crate builds is the stock recipe's,
/// at every density: the factory leaves the unrendered dimension to the
/// widget's own constant, as the page says it must.
#[test]
fn every_factory_leaves_the_unrendered_fields_at_the_stock_projection() {
    let rows = unrendered();
    let mut compared = 0;
    for density in DENSITIES {
        let tokens = InputTokens::for_density(density);
        for (recipe, preset, stock) in factories(&tokens) {
            let listed = rows
                .iter()
                .filter(|(_, r, _)| r == recipe)
                .map(|(_, _, f)| f.as_str());
            for field in listed {
                compared += 1;
                let value = |set: &Fields| {
                    set.iter()
                        .find(|(f, _)| *f == field)
                        .map(|(_, v)| *v)
                        .unwrap_or_else(|| panic!("{recipe}::{field} is not compared here"))
                };
                assert_eq!(
                    value(&preset),
                    value(&stock),
                    "{density:?}: {recipe}::{field} is set to a value the widget does not render"
                );
            }
        }
    }
    assert!(
        compared > 0,
        "no listed field of a recipe this crate builds"
    );
}
