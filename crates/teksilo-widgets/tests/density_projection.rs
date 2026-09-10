// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The density sweep's contract, checked three ways.
//!
//! 1. **Compact is the identity.** Every `Recipe*Style::for_tokens` at
//!    `TargetDensity::Compact` equals its `Default`, and every dimension
//!    resolver returns the constant it was named after. This is what makes the
//!    whole density layer inert until an app opts in, and it is asserted per
//!    recipe rather than per widget so a new field cannot slip past it.
//! 2. **The ladder actually moves.** The same recipes at `Comfortable` and
//!    `Touch` report the tokened values, and a laid-out `MenuItem` grows when
//!    the tree's density is switched at runtime — which is the only thing that
//!    proves `WidgetTree::set_input_density`'s `Rebuild` binding re-bakes a
//!    dimension decided in `build()`, rather than merely relayouting.
//! 3. **The inventory is true.** `docs/density-inventory.md` is the audit
//!    artifact; this parses it and holds it to its own rules, so a row that
//!    claims a treatment the code does not implement fails here rather than in
//!    a reader's head.
//!
//! Reference: `docs/density-inventory.md`, and the touch design's A11.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use teksilo_canvas::{Size, SizeProposal};
use teksilo_core::styles::density::{density_min_size, dp, spacing};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::{InputTokens, TargetAxes, TargetDensity, TargetRole};
use teksilo_widgets::styles::*;

fn compact() -> InputTokens {
    InputTokens::for_density(TargetDensity::Compact)
}
fn comfortable() -> InputTokens {
    InputTokens::for_density(TargetDensity::Comfortable)
}
fn touch() -> InputTokens {
    InputTokens::for_density(TargetDensity::Touch)
}

// ───────────────────────────────────────────────────────────────────────────
// 1. Compact is the identity, for every recipe that carries dimensions
// ───────────────────────────────────────────────────────────────────────────

/// Assert `R::default() == R::for_tokens(&InputTokens::default())` for each
/// named recipe type.
///
/// This is the load-bearing invariant of the whole package: `Default` is
/// documented as the Compact projection, every widget's lazy fallback now calls
/// `for_tokens`, and any drift between the two would silently change what a
/// Compact tree renders.
macro_rules! compact_is_default {
    ($($recipe:ident),* $(,)?) => {
        #[test]
        fn every_recipe_default_equals_its_compact_projection() {
            let t = compact();
            $(
                assert_eq!(
                    $recipe::default(),
                    $recipe::for_tokens(&t),
                    concat!(
                        stringify!($recipe),
                        "::default() must equal for_tokens(Compact) — see the type's own doc",
                    ),
                );
            )*
        }
    };
}

compact_is_default!(
    AvatarRecipe,
    BadgeRecipe,
    BannerRecipe,
    CalendarRecipe,
    CardRecipe,
    CheckboxRecipe,
    ColorPickerRecipe,
    ComboBoxRecipe,
    DateEditRecipe,
    DialogRecipe,
    DropTargetRecipe,
    DropZoneRecipe,
    IconButtonRecipe,
    LinkRecipe,
    MenuItemRecipe,
    PanelRecipe,
    PopoverRecipe,
    ProgressBarRecipe,
    RadioRecipe,
    RadioTileRecipe,
    ScrollBarRecipe,
    SearchFieldRecipe,
    SegmentedControlRecipe,
    SliderRecipe,
    SnackbarRecipe,
    SplitterRecipe,
    StandardItemRecipe,
    TabRecipe,
    TableRecipe,
    TextInputRecipe,
    ToastRecipe,
    ToggleRecipe,
    TooltipRecipe,
);

/// `RecipeButtonStyle` has no separate recipe struct — its variants carry
/// their own `ButtonRecipe`s — so its Compact identity is checked on the
/// per-variant dimensions instead.
#[test]
fn the_button_style_compact_projection_matches_its_default() {
    let by_default = RecipeButtonStyle::default();
    let by_tokens = RecipeButtonStyle::for_tokens(&compact());
    assert_eq!(by_default.recipes.len(), by_tokens.recipes.len());
    for (variant, recipe) in &by_default.recipes {
        let other = by_tokens
            .recipes
            .get(variant)
            .expect("for_tokens must define the same variants as Default");
        assert_eq!(
            recipe.min_size, other.min_size,
            "{variant:?} min_size drifted between Default and for_tokens(Compact)"
        );
        assert_eq!(
            recipe.padding, other.padding,
            "{variant:?} padding drifted between Default and for_tokens(Compact)"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// 2. The ladder moves above Compact
// ───────────────────────────────────────────────────────────────────────────

/// The three helper shapes, spot-checked on the dimensions the design's
/// Constants table names by number.
#[test]
fn the_shipped_recipes_walk_the_ladder() {
    // A menu row: 24 dp at Compact (the AA floor), then the ladder.
    assert_eq!(MenuItemRecipe::for_tokens(&compact()).item_height, 24.0);
    assert_eq!(MenuItemRecipe::for_tokens(&comfortable()).item_height, 32.0);
    assert_eq!(MenuItemRecipe::for_tokens(&touch()).item_height, 44.0);

    // A text field: 28 dp at Compact, already above the floor, so Comfortable
    // raises it only to 32.
    assert_eq!(TextInputRecipe::for_tokens(&compact()).height, 28.0);
    assert_eq!(TextInputRecipe::for_tokens(&comfortable()).height, 32.0);
    assert_eq!(TextInputRecipe::for_tokens(&touch()).height, 44.0);

    // A checkbox's hit area, which is what a finger aims at; the 19 dp painted
    // box is a decoration and does not move.
    assert_eq!(CheckboxRecipe::for_tokens(&touch()).box_hit_area, 44.0);
    assert_eq!(
        CheckboxRecipe::for_tokens(&touch()).box_visual_size,
        CheckboxRecipe::for_tokens(&compact()).box_visual_size,
    );

    // Padding scales rather than clamping: 1.00 / 1.15 / 1.30.
    let pad_compact = MenuItemRecipe::for_tokens(&compact()).padding_horizontal;
    let pad_touch = MenuItemRecipe::for_tokens(&touch()).padding_horizontal;
    assert!((pad_touch - pad_compact * 1.30).abs() < 1e-4, "{pad_touch}");

    // The scroll-bar thumb's minimum length reaches the touch target while the
    // lane's thickness — a grab affordance whose hit area grows instead — does
    // not move.
    assert_eq!(
        ScrollBarRecipe::for_tokens(&compact()).min_thumb_length,
        24.0
    );
    assert_eq!(ScrollBarRecipe::for_tokens(&touch()).min_thumb_length, 44.0);
    assert_eq!(
        ScrollBarRecipe::for_tokens(&touch()).thickness_hover,
        ScrollBarRecipe::for_tokens(&compact()).thickness_hover,
    );
}

/// A corner radius, a hairline and a glyph metric are decorations: they read
/// the same at every density, which is what keeps a Touch build looking like
/// the same design language rather than a zoomed one.
#[test]
fn decorations_never_move() {
    for t in [compact(), comfortable(), touch()] {
        assert_eq!(CardRecipe::for_tokens(&t).corner_radius, 8.0);
        assert_eq!(CardRecipe::for_tokens(&t).border_width, 1.0);
        assert_eq!(BannerRecipe::for_tokens(&t).glyph_size, 16.0);
        assert_eq!(ToggleRecipe::for_tokens(&t).track_width, 28.0);
        assert_eq!(ToggleRecipe::for_tokens(&t).track_height, 16.0);
    }
}

/// The helper contract itself, on the exact numbers the design's Constants
/// table publishes.
#[test]
fn the_helpers_report_the_published_ladder() {
    assert_eq!(dp(24.0, TargetRole::Target, &compact()), 24.0);
    assert_eq!(dp(24.0, TargetRole::Target, &comfortable()), 32.0);
    assert_eq!(dp(24.0, TargetRole::Target, &touch()), 44.0);

    assert_eq!(dp(6.0, TargetRole::Grab, &compact()), 6.0);
    assert_eq!(dp(6.0, TargetRole::Grab, &comfortable()), 10.0);
    assert_eq!(dp(6.0, TargetRole::Grab, &touch()), 16.0);

    assert_eq!(spacing(10.0, &compact()), 10.0);
    assert!((spacing(10.0, &comfortable()) - 11.5).abs() < 1e-4);
    assert!((spacing(10.0, &touch()) - 13.0).abs() < 1e-4);

    let base = Size::new(0.0, 22.0);
    assert_eq!(
        density_min_size(base, TargetAxes::HEIGHT, &compact()),
        Size::new(0.0, 24.0),
        "a sub-floor MinSize is raised to the 24 dp conformance floor even at Compact",
    );
}

// ───────────────────────────────────────────────────────────────────────────
// 3. A runtime density switch actually re-bakes
// ───────────────────────────────────────────────────────────────────────────

/// The whole reason `set_input_density` marks `Rebuild` rather than reusing
/// `set_theme`'s `mark_all_dirty()`: a `MenuItem`'s height is baked in
/// `build()`, so a layout+paint mark cannot move it.
///
/// A failure here means the density projection is reaching the tokens but not
/// the built tree — the exact bug A11's revision-3 correction exists to
/// prevent.
#[test]
fn a_density_switch_regrows_a_laid_out_menu_item() {
    // A menu row inside a vertical stack reports its own intrinsic height
    // rather than filling the window, which is the number the density moves.
    fn proposal() -> SizeProposal {
        SizeProposal {
            width: Some(300.0),
            height: None,
        }
    }

    fn height_at(density: TargetDensity) -> f32 {
        let mut tree = WidgetTree::new();
        let item = tree.add(teksilo_widgets::MenuItem::new(teksilo_i18n::lit!("Open")));
        tree.layout(proposal());
        if density == TargetDensity::Compact {
            return tree.bounds(item).height;
        }
        tree.set_input_density(density);
        tree.layout(proposal());
        tree.bounds(item).height
    }

    let at_compact = height_at(TargetDensity::Compact);
    let at_comfortable = height_at(TargetDensity::Comfortable);
    let at_touch = height_at(TargetDensity::Touch);

    // The row's laid-out height carries a fixed inset of its own (the focus
    // envelope), so the contract is the *delta*: the row must grow by exactly
    // the ladder's step, 24 → 32 → 44.
    assert!(
        (at_comfortable - at_compact - 8.0).abs() < 0.01,
        "Comfortable must add 32 - 24 = 8 dp, got {at_comfortable} vs {at_compact}"
    );
    assert!(
        (at_touch - at_compact - 20.0).abs() < 0.01,
        "Touch must add 44 - 24 = 20 dp, got {at_touch} vs {at_compact}"
    );
}

/// Switching back returns the exact Compact geometry — the projection is a
/// pure function of the density, not an accumulating transform.
#[test]
fn switching_back_to_compact_restores_the_original_geometry() {
    let proposal = SizeProposal {
        width: Some(300.0),
        height: None,
    };
    let mut tree = WidgetTree::new();
    let item = tree.add(teksilo_widgets::MenuItem::new(teksilo_i18n::lit!("Open")));
    tree.layout(proposal);
    let before = tree.bounds(item);

    tree.set_input_density(TargetDensity::Touch);
    tree.layout(proposal);
    tree.set_input_density(TargetDensity::Compact);
    tree.layout(proposal);

    assert_eq!(tree.bounds(item).height, before.height);
    assert_eq!(tree.bounds(item).width, before.width);
}

/// An app-installed Tier-3 slot is `Some(..)` and rides a density switch
/// verbatim: a hand-written style owns its own metrics, and the framework does
/// not rewrite them. Documented behaviour, asserted so it stays deliberate.
#[test]
fn an_app_installed_style_slot_survives_a_density_switch() {
    #[derive(Debug)]
    struct AppButtonStyle;
    impl teksilo_core::styles::ButtonStyle for AppButtonStyle {
        fn make_body(
            &self,
            cfg: &teksilo_core::styles::ButtonStyleConfig,
            ctx: &mut teksilo_core::BuildContext,
        ) -> teksilo_core::WidgetId {
            ctx.add(teksilo_widgets::primitives::MinSize::new(50.0, 50.0).child_id(cfg.label))
        }
    }

    let mut theme = teksilo_core::presets::intui::light();
    let installed: Rc<dyn teksilo_core::styles::ButtonStyle> = Rc::new(AppButtonStyle);
    theme.style_slots.button = Some(installed.clone());

    let projected = theme.with_density(TargetDensity::Touch);
    let survivor = projected
        .style_slots
        .button
        .clone()
        .expect("the installed slot must survive the projection");
    assert!(
        Rc::ptr_eq(&installed, &survivor),
        "the very same Rc must ride across, not a rebuilt one"
    );
    assert_eq!(projected.input.density, TargetDensity::Touch);
}

// ───────────────────────────────────────────────────────────────────────────
// 4. The inventory holds itself to its own rules
// ───────────────────────────────────────────────────────────────────────────

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("teksilo-widgets lives two levels below the workspace root")
        .to_path_buf()
}

/// One parsed inventory row: `(class, compact value, treatment)`.
struct Row {
    file: String,
    ident: String,
    value: Option<f32>,
    class: String,
    treatment: String,
}

fn inventory_rows() -> Vec<Row> {
    let path = repo_root().join("docs/density-inventory.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is missing or unreadable: {e}", path.display()));
    let mut rows = Vec::new();
    for line in text.lines() {
        if !line.starts_with("| `crates/") {
            continue;
        }
        let cols: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cols.len() != 6 {
            panic!(
                "inventory row has {} columns, expected 6: {line}",
                cols.len()
            );
        }
        let raw = cols[3].trim_matches('`');
        rows.push(Row {
            file: cols[0].trim_matches('`').to_string(),
            ident: cols[2].trim_matches('`').to_string(),
            value: raw.parse::<f32>().ok(),
            class: cols[4].to_string(),
            treatment: cols[5].to_string(),
        });
    }
    assert!(
        rows.len() > 500,
        "only {} inventory rows parsed — the table format changed",
        rows.len()
    );
    rows
}

/// Every row must carry one of the four classes. A fifth would mean the
/// vocabulary drifted and the sweep no longer covers what the document claims.
#[test]
fn every_inventory_row_carries_a_known_class() {
    for row in inventory_rows() {
        assert!(
            matches!(
                row.class.as_str(),
                "Target" | "Grab" | "Spacing" | "Decoration" | "— not a dimension"
            ),
            "{} / {} has an unknown class {:?}",
            row.file,
            row.ident,
            row.class,
        );
    }
}

/// The rule that governs this whole package, and the one correction it made to
/// the P02 inventory:
///
/// `dp(.., TargetRole::Target, ..)` is a **floor**, so routing a dimension
/// whose Compact value is below 24 dp through it would raise it *at Compact* —
/// which the programme's invariant forbids. A row may therefore say "scales
/// with `target_size`" only if its Compact value already meets the floor.
/// Everything below it keeps its painted value at every density and takes its
/// conformance from the hit mechanisms, which is what A10 designed them for.
///
/// The named exceptions are the `MinSize` rows: a `MinSize` *is* a hit box, so
/// `density_min_size` enforces the floor there, and the files listed below
/// change at Compact by design. The count is deliberately not written here —
/// `docs/density-inventory.md` §0 enumerates the Compact-visible exceptions and
/// is the only place it lives, because a count repeated in a second place is a
/// count that drifts. It drifted here: this sentence said "three" while the
/// list under it named four.
#[test]
fn no_sub_floor_dimension_claims_to_scale_with_target_size() {
    const DECLARED_EXCEPTIONS: &[&str] = &[
        "crates/teksilo-preview-ui/src/navigator.rs",
        "crates/teksilo-theme-macos/src/styles/button.rs",
        "crates/teksilo-theme-macos/src/styles/text_input.rs",
    ];

    for row in inventory_rows() {
        if !row.treatment.contains("scales with `target_size`") {
            continue;
        }
        let Some(value) = row.value else { continue };
        if value >= 24.0 {
            continue;
        }
        assert!(
            DECLARED_EXCEPTIONS.contains(&row.file.as_str()) && row.ident.contains("MinSize::new"),
            "{} / {} is {value} dp — below the 24 dp floor — yet claims to scale with \
             `target_size`. Either it is one of the declared `MinSize` exceptions, or its \
             treatment must say the paint is fixed and name the hit mechanism that carries it.",
            row.file,
            row.ident,
        );
    }
}

/// A `Spacing` row multiplies rather than clamps, so it is safe at any value —
/// but it must actually say so, or a reader auditing conformance cannot tell a
/// swept dimension from a missed one.
#[test]
fn every_spacing_row_names_the_spacing_factor() {
    for row in inventory_rows() {
        if row.class != "Spacing" {
            continue;
        }
        assert!(
            row.treatment.contains("spacing_factor") || row.treatment.starts_with("fixed"),
            "{} / {} is classed Spacing but its treatment neither names `spacing_factor` nor \
             says why it is fixed: {:?}",
            row.file,
            row.ident,
            row.treatment,
        );
    }
}

/// A `Decoration` or non-dimension row must say it is fixed. This is what makes
/// the document readable as an audit: every row either names a density route or
/// says, in the same column, why it has none.
#[test]
fn every_fixed_row_says_it_is_fixed() {
    for row in inventory_rows() {
        if row.class != "Decoration" && row.class != "— not a dimension" {
            continue;
        }
        assert!(
            row.treatment.starts_with("fixed") || row.treatment.starts_with("**deleted**"),
            "{} / {} is classed {:?} but its treatment neither begins with \"fixed\" nor \
             records the constant's removal: {:?}",
            row.file,
            row.ident,
            row.class,
            row.treatment,
        );
    }
}
