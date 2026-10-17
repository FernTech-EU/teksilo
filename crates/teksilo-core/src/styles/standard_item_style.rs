// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `StandardListItem` / `StandardTreeItem`.
//! See `docs/styling-system.md`.

use std::rc::Rc;

use teksilo_tokens::{InputTokens, TargetRole, TextRole};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::styles::density::dp;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct StandardItemStyleConfig {
    /// Pre-built row content — typically an HStack of `[checkbox?]
    /// [leading?] [center?] [label / VStack { label, subtitle row }]
    /// [Spacer] [trailing?]` composed by the host
    /// `StandardListItem` / `StandardTreeItem`. The style is
    /// responsible for the chrome (selection background, corner
    /// radius, padding) but not for row-internal layout — the
    /// per-slot composition is StandardItem-specific (subtitle has
    /// its own sub-row, the checkbox carries `labelled_externally` AT
    /// metadata, etc.) and would force every custom style to
    /// reimplement it.
    pub content: WidgetId,
    pub is_selected: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_pressed: Signal<bool>,
    pub is_focused: Signal<bool>,
    /// Input-modality "focus-visible": `true` after keyboard input. A focus ring
    /// should render only when this and `is_focused` are both true, so a mouse
    /// click selects without a ring while keyboard navigation reveals one.
    pub is_focus_visible: Signal<bool>,
    pub is_disabled: Signal<bool>,
    /// Whether the host window is currently active (`focused AND not occluded`).
    /// Composed with `is_focused` so a selected row shows the vivid `Selected`
    /// surface only while the view holds keyboard focus **and** the window is
    /// active; otherwise it falls back to the muted `SelectedInactive` — the
    /// same desaturation a view-focus loss produces (macOS "unemphasized"
    /// selection serves both states). Populated from
    /// `BuildContext::window_active_signal`.
    ///
    /// Which of the two signals a style composes is its
    /// [`StandardItemStyle::selection_emphasis`]; build the condition with
    /// [`SelectionEmphasis::emphasised`] rather than by hand, so the chrome
    /// and the row's label (which the row composes from the same hook) agree.
    pub is_window_active: Signal<bool>,
}

/// When a selected row is drawn **emphasised**: the vivid `Selected`
/// surface (and [`StandardItemStyle::selected_label_role`] on its
/// foregrounds) rather than the muted `SelectedInactive` one.
///
/// Design languages disagree on what mutes a selection. macOS, IntUI and
/// Fluent mute it whenever the view loses keyboard focus, as well as when
/// the window does. Qt picks a palette colour group per *window*: every
/// view in an active window paints its selection with the active
/// `Highlight`, whichever of them holds focus, so KDE's Breeze mutes a
/// selection only when the window goes inactive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum SelectionEmphasis {
    /// Emphasised while the row's view holds keyboard focus **and** its
    /// window is active. The default, and the rule every shipped style
    /// followed before the choice existed.
    #[default]
    ViewFocusedAndWindowActive,
    /// Emphasised while the window is active, whether or not the view holds
    /// keyboard focus: Qt's per-window colour groups.
    WindowActive,
}

impl SelectionEmphasis {
    /// The emphasis condition as a signal, from the view-focus signal (a
    /// row's [`StandardItemStyleConfig::is_focused`]) and the window-active
    /// signal ([`StandardItemStyleConfig::is_window_active`]).
    ///
    /// A selected row is emphasised while this reads `true`. Selection
    /// itself is not part of it: the caller combines that, since the chrome
    /// and the label each already hold it.
    pub fn emphasised(
        self,
        view_focused: &Signal<bool>,
        window_active: &Signal<bool>,
    ) -> Signal<bool> {
        match self {
            Self::ViewFocusedAndWindowActive => view_focused.and(window_active),
            Self::WindowActive => window_active.clone(),
        }
    }
}

pub trait StandardItemStyle: 'static {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId;

    /// The text role a row's label takes while the row is **emphasised**
    /// — selected, with its view focused and its window active. `None`
    /// (the default) keeps the row's own mapping, `TextRole::Primary`.
    ///
    /// The row builds its label before a style ever sees it, so a style
    /// that fills the selection with a saturated colour cannot recolour
    /// the text on top of it. Design languages whose selected row is a
    /// solid accent fill with a light label — macOS's
    /// `alternateSelectedControlTextColor` is the canonical case — return
    /// [`TextRole::OnAccent`] here; ones whose selection is a pale wash
    /// (IntUI, Fluent) leave it `None` and the label keeps its contrast
    /// against the wash.
    ///
    /// Same shape as
    /// [`ButtonStyle::label_text_role`](crate::styles::ButtonStyle::label_text_role),
    /// and defaulted for the same reason: an existing style needs no
    /// change.
    fn selected_label_role(&self) -> Option<TextRole> {
        None
    }

    /// When a selected row is drawn emphasised; see [`SelectionEmphasis`].
    ///
    /// The row reads this for its foregrounds (the label, the subtitle and
    /// the tree chevron take [`selected_label_role`](Self::selected_label_role)
    /// only while the row is emphasised), and a style reads it for its own
    /// chrome through [`SelectionEmphasis::emphasised`], so a solid
    /// selection fill and the text on it stay in step.
    ///
    /// **Defaulted** to [`SelectionEmphasis::ViewFocusedAndWindowActive`],
    /// the rule every style followed before this existed, so an existing
    /// style needs no change. The shipped `RecipeStandardItemStyle` consults
    /// its own value, which is always the default: a style that returns
    /// another value and delegates its chrome to the recipe gets the
    /// recipe's muting, and has to paint its selection itself to follow its
    /// own rule.
    fn selection_emphasis(&self) -> SelectionEmphasis {
        SelectionEmphasis::ViewFocusedAndWindowActive
    }

    /// The height floor of a row with no subtitle, in logical pixels, at the
    /// density `tokens` describe.
    ///
    /// A metrics accessor rather than a `make_body` concern: the row measures
    /// itself in `layout_response`, long after the chrome is built, and holds
    /// an `Rc<dyn StandardItemStyle>` that cannot reach a recipe field.
    /// Without it a preset's row height is written and never rendered.
    ///
    /// **Defaulted**, so a `StandardItemStyle` implemented outside this
    /// workspace keeps compiling and keeps the ladder it had. The default is
    /// `teksilo_widgets::styles::recipe_standard_item_style::STANDARD_ITEM_MIN_HEIGHT_SINGLE_LINE`
    /// put through [`dp`] as a target, restated as a literal because
    /// `teksilo-core` cannot name a `teksilo-widgets` constant, and pinned
    /// equal to it by
    /// `the_standard_item_trait_defaults_restate_the_module_constants`.
    fn min_height_single_line(&self, tokens: &InputTokens) -> f32 {
        dp(28.0, TargetRole::Target, tokens)
    }

    /// The height floor of a row carrying a subtitle, in logical pixels.
    /// Defaulted on the same terms as
    /// [`min_height_single_line`](Self::min_height_single_line), from
    /// `STANDARD_ITEM_MIN_HEIGHT_TWO_LINE`.
    fn min_height_two_line(&self, tokens: &InputTokens) -> f32 {
        dp(44.0, TargetRole::Target, tokens)
    }
}

pub type SharedStandardItemStyle = Rc<dyn StandardItemStyle>;
