// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Typed `Rc<dyn FooStyle>` slot bag — the theme-wide override channel
//! for the four-tier styling system.
//!
//! Each themable widget reads its slot like:
//!
//! ```ignore
//! let style = self.style_override
//!     .clone()
//!     .or_else(|| ctx.theme().style_slots.button.clone())
//!     .unwrap_or_else(|| Rc::new(RecipeButtonStyle::default()));
//! ```
//!
//! Per-call `.style(...)` overrides win first; theme-wide
//! `style_slots.button = Some(...)` wins second; the widget's local
//! `Recipe*Style` default is the fallback.
//!
//! `Option` per slot rather than a populated default because the
//! `Recipe*Style` types live in `teksilo-widgets` and can't be imported by
//! `teksilo-core` (cycle). Apps that want theme-wide custom styles
//! install them explicitly:
//!
//! ```ignore
//! let mut theme = intui::light();
//! theme.style_slots.button = Some(Rc::new(MyGlassButton));
//! ```

use crate::styles::{
    SharedAvatarStyle, SharedBadgeStyle, SharedBannerStyle, SharedButtonStyle, SharedCalendarStyle,
    SharedCardStyle, SharedChartStyle, SharedCheckboxStyle, SharedColorPickerStyle,
    SharedComboBoxStyle, SharedDateEditStyle, SharedDialogStyle, SharedDropTargetStyle,
    SharedDropZoneStyle, SharedGridViewStyle, SharedIconButtonStyle, SharedLinkStyle,
    SharedListContainerStyle, SharedMenuItemStyle, SharedPanelStyle, SharedPopoverStyle,
    SharedProgressBarStyle, SharedRadioStyle, SharedRadioTileStyle, SharedRichTextEditorStyle,
    SharedScrollBarStyle, SharedSearchFieldStyle, SharedSegmentedControlStyle, SharedSliderStyle,
    SharedSnackbarStyle, SharedSpinBoxStyle, SharedSplitButtonStyle, SharedSplitterStyle,
    SharedStandardItemStyle, SharedTabStyle, SharedTableStyle, SharedTextInputStyle,
    SharedTextSelectionStyle, SharedToastStyle, SharedToggleStyle, SharedTooltipStyle,
    SharedWebViewStyle,
};

/// Typed slot bag living on [`crate::styles::Theme`]. One slot per
/// themable widget. `None` means "use the widget's local default
/// `Recipe*Style`"; `Some(rc)` installs the override theme-wide.
#[derive(Default, Clone)]
pub struct ComponentStyleSlots {
    pub button: Option<SharedButtonStyle>,
    pub split_button: Option<SharedSplitButtonStyle>,
    pub splitter: Option<SharedSplitterStyle>,
    pub icon_button: Option<SharedIconButtonStyle>,
    pub toggle: Option<SharedToggleStyle>,
    pub checkbox: Option<SharedCheckboxStyle>,
    pub radio: Option<SharedRadioStyle>,
    pub radio_tile: Option<SharedRadioTileStyle>,
    pub slider: Option<SharedSliderStyle>,
    pub text_input: Option<SharedTextInputStyle>,
    pub combo_box: Option<SharedComboBoxStyle>,
    pub menu_item: Option<SharedMenuItemStyle>,
    pub panel: Option<SharedPanelStyle>,
    pub card: Option<SharedCardStyle>,
    pub chart: Option<SharedChartStyle>,
    pub popover: Option<SharedPopoverStyle>,
    pub tooltip: Option<SharedTooltipStyle>,
    pub scroll_bar: Option<SharedScrollBarStyle>,
    pub standard_item: Option<SharedStandardItemStyle>,
    pub tab: Option<SharedTabStyle>,
    pub dialog: Option<SharedDialogStyle>,
    pub snackbar: Option<SharedSnackbarStyle>,
    pub toast: Option<SharedToastStyle>,
    pub banner: Option<SharedBannerStyle>,
    pub badge: Option<SharedBadgeStyle>,
    pub progress_bar: Option<SharedProgressBarStyle>,
    pub link: Option<SharedLinkStyle>,
    pub segmented_control: Option<SharedSegmentedControlStyle>,
    pub avatar: Option<SharedAvatarStyle>,
    pub calendar: Option<SharedCalendarStyle>,
    pub color_picker: Option<SharedColorPickerStyle>,
    pub spin_box: Option<SharedSpinBoxStyle>,
    pub date_edit: Option<SharedDateEditStyle>,
    pub search_field: Option<SharedSearchFieldStyle>,
    pub rich_text_editor: Option<SharedRichTextEditorStyle>,
    pub table: Option<SharedTableStyle>,
    pub list_container: Option<SharedListContainerStyle>,
    pub drop_zone: Option<SharedDropZoneStyle>,
    pub drop_target: Option<SharedDropTargetStyle>,
    pub grid_view: Option<SharedGridViewStyle>,
    pub web_view: Option<SharedWebViewStyle>,
    /// Touch text-selection chrome — the selection handles and the magnifier.
    /// See [`TextSelectionStyle`](crate::styles::TextSelectionStyle).
    pub text_selection: Option<SharedTextSelectionStyle>,
}

/// Every slot's name, written **once**, expanded through by each probe below.
///
/// [`ComponentStyleSlots::NAMES`], [`ComponentStyleSlots::installed`],
/// [`ComponentStyleSlots::unchanged_against`] and the `Debug` and `PartialEq`
/// impls all expand this one roster, so none of them can be the copy that fell
/// behind: a hand-written list per asker is how one of them loses a slot, which
/// for `installed` means an override that is silently never reported. Every
/// probe except `NAMES` also destructures `Self` without a `..` rest pattern, so
/// a slot added to the struct and not to this roster is a compile error.
macro_rules! for_every_slot {
    ($probe:ident) => {
        $probe!(
            button,
            split_button,
            splitter,
            icon_button,
            toggle,
            checkbox,
            radio,
            radio_tile,
            slider,
            text_input,
            combo_box,
            menu_item,
            panel,
            card,
            chart,
            popover,
            tooltip,
            scroll_bar,
            standard_item,
            tab,
            dialog,
            snackbar,
            toast,
            banner,
            badge,
            progress_bar,
            link,
            segmented_control,
            avatar,
            calendar,
            color_picker,
            spin_box,
            date_edit,
            search_field,
            rich_text_editor,
            table,
            list_container,
            drop_zone,
            drop_target,
            grid_view,
            web_view,
            text_selection
        )
    };
}

impl ComponentStyleSlots {
    /// Every slot's name, in declaration order: the roster that
    /// [`installed`](Self::installed) and
    /// [`unchanged_against`](Self::unchanged_against) answer from.
    ///
    /// For a test that wants to say which slots a preset fills and which it
    /// deliberately leaves to the widget's default, as set equality against
    /// this list rather than against a hand-copied one.
    pub const NAMES: &'static [&'static str] = {
        macro_rules! probe {
            ($($name:ident),* $(,)?) => {
                &[$(stringify!($name)),*]
            };
        }
        for_every_slot!(probe)
    };

    /// The names of the slots that carry an override, in declaration order.
    ///
    /// The direct "what is installed here" question, kept for diagnostics and
    /// tests. Its original consumer — the target-conformance audit's
    /// [`unprojected_style_slots`](crate::accessibility::target_audit::unprojected_style_slots)
    /// — now asks [`unchanged_against`](Self::unchanged_against) instead,
    /// comparing the theme derived at two densities.
    ///
    /// The body destructures `Self` **without** a `..` rest pattern, so adding a
    /// slot to the struct and forgetting it in `for_every_slot!` is a compile
    /// error rather than a silently unreported override.
    pub fn installed(&self) -> Vec<&'static str> {
        macro_rules! probe {
            ($($name:ident),* $(,)?) => {{
                let Self { $($name),* } = self;
                let mut out = Vec::new();
                $(if $name.is_some() {
                    out.push(stringify!($name));
                })*
                out
            }};
        }
        for_every_slot!(probe)
    }

    /// The slots installed here that are **the same object** in `other` — the
    /// ones a re-derivation left alone.
    ///
    /// One consumer, and it is the same one [`installed`](Self::installed)
    /// has: the target-conformance audit's
    /// [`unprojected_style_slots`](crate::accessibility::target_audit::unprojected_style_slots),
    /// which asks a theme for the styles no
    /// [`DensityProjection`](crate::styles::DensityProjection) rebuilds by
    /// deriving it at two densities and comparing.
    ///
    /// `Rc::ptr_eq`, not any comparison of contents: a Tier-3 style is a trait
    /// object with no equality of its own, and identity is the right question.
    /// A projection that re-derives a slot builds a fresh `Rc` for it, so a slot
    /// still pointing at the same allocation at two densities is one the ladder
    /// did not reach — whoever installed it.
    ///
    /// The body destructures `self` without a `..` rest pattern, for the reason
    /// [`installed`](Self::installed) does: adding a slot to the struct and
    /// forgetting it in `for_every_slot!` is a compile error rather than a
    /// silent omission. Both probes expand through that one roster, so neither
    /// can be the copy that fell behind.
    pub fn unchanged_against(&self, other: &Self) -> Vec<&'static str> {
        fn same<T: ?Sized>(a: &Option<std::rc::Rc<T>>, b: &Option<std::rc::Rc<T>>) -> bool {
            matches!((a, b), (Some(x), Some(y)) if std::rc::Rc::ptr_eq(x, y))
        }
        macro_rules! probe {
            ($($name:ident),* $(,)?) => {{
                // The destructure is what makes this exhaustive: a slot added
                // to the struct and not listed below fails to compile here.
                // `other` is then read field by field under the same names.
                let Self { $($name),* } = self;
                let mut out = Vec::new();
                $(if same($name, &other.$name) {
                    out.push(stringify!($name));
                })*
                out
            }};
        }
        for_every_slot!(probe)
    }

    /// Whether no slot carries an override — the state every shipped preset
    /// that ships raw tokens alone is in.
    pub fn is_empty(&self) -> bool {
        self.installed().is_empty()
    }
}

impl std::fmt::Debug for ComponentStyleSlots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-rolled because `Rc<dyn FooStyle>` doesn't impl Debug.
        // Show which slots are populated (Some/None): the actual
        // chrome behaviour isn't introspectable.
        macro_rules! probe {
            ($($name:ident),* $(,)?) => {{
                // No `..`: a slot missing from the roster fails to compile.
                let Self { $($name),* } = self;
                f.debug_struct("ComponentStyleSlots")
                    $(.field(stringify!($name), &$name.is_some()))*
                    .finish()
            }};
        }
        for_every_slot!(probe)
    }
}

impl PartialEq for ComponentStyleSlots {
    fn eq(&self, other: &Self) -> bool {
        // Rc trait-object pointer-equality is the only meaningful "are
        // these the same style" check. Used for theme-equality (mostly
        // tests + cache keys).
        fn rc_eq<T: ?Sized>(a: &Option<std::rc::Rc<T>>, b: &Option<std::rc::Rc<T>>) -> bool {
            match (a, b) {
                (None, None) => true,
                (Some(x), Some(y)) => std::rc::Rc::ptr_eq(x, y),
                _ => false,
            }
        }
        macro_rules! probe {
            ($($name:ident),* $(,)?) => {{
                // No `..`: a slot missing from the roster fails to compile.
                let Self { $($name),* } = self;
                true $(&& rc_eq($name, &other.$name))*
            }};
        }
        for_every_slot!(probe)
    }
}

// `styles.rs` declares this module with `#[path]`, so a child module resolves
// from `styles/`, not from a directory named after this file.
#[cfg(test)]
#[path = "component_style_slots/tests.rs"]
mod tests;
