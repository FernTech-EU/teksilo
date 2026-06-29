// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ThemeSwitcher — a drop-in app-theme picker for settings screens & toolbars.
//!
//! A thin [`ComboBox`] preset that switches the application theme. By default
//! it offers three entries — **Light**, **Dark**, and **System** — where
//! *System* follows the native OS theme live: it adopts the OS's actual colours
//! (GNOME / KDE / Cinnamon on Linux) and tracks OS light/dark changes at
//! runtime, falling back to the built-in light/dark presets on platforms
//! without OS-colour support.
//!
//! Zero-config: drop `ThemeSwitcher::new()` into a settings panel or toolbar and
//! it
//!
//! - shows the active theme as the current selection (matched by the theme's
//!   stable [`ThemeId`]),
//! - switches the app theme on selection via `EventContext::set_theme` (fixed
//!   themes) or `EventContext::follow_system_theme` (System),
//! - and stays in sync if the theme changes elsewhere (a menu, the inspector,
//!   or an OS light/dark toggle).
//!
//! ```ignore
//! // In a settings panel or toolbar:
//! Toolbar::new().child(HStack::new().child(Spacer::new()).child(ThemeSwitcher::new()))
//! ```
//!
//! Labels are **translated** via the framework Fluent bundle (`tr_widget!`),
//! with an English literal fallback so a host app that hasn't installed an
//! `I18nManager` still reads "Light / Dark / System" rather than raw keys.
//!
//! Custom themes: `.themes([(label, theme), …])` replaces Light/Dark with an
//! app-supplied set (e.g. the `bastyde-theme-{fluent,macos,material3}` presets);
//! `.system(false)` drops the System entry.

use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{Theme, ThemeId};
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{LocalizedString, tr_widget};

use crate::combo_box::{ComboBox, ComboBoxVariant};

/// One row in the switcher: the theme's stable [`ThemeId`] (used to match the
/// active theme and to look up the action) and the user-facing `display`
/// string. `ThemeId` `"system"` denotes the follow-OS entry.
#[derive(Clone, PartialEq)]
struct ThemeChoice {
    id: ThemeId,
    display: String,
}

/// What selecting an entry does: pin a fixed theme, or follow the OS.
/// The fixed theme is boxed because `Theme` is large relative to the
/// zero-size `FollowSystem` variant.
#[derive(Clone)]
enum ThemeAction {
    Set(Box<Theme>),
    FollowSystem,
}

/// A drop-in app-theme picker built on [`ComboBox`]. See the module docs.
pub struct ThemeSwitcher {
    /// Forwarded to the inner [`ComboBox`]. Defaults to `Outlined`.
    variant: ComboBoxVariant,
    /// Accessible / control label. Defaults to the translated "Theme".
    label: Option<LocalizedString>,
    /// Explicit fixed-theme list `(label, theme)`. When `None` (the default),
    /// the switcher offers Light + Dark.
    themes_override: Option<Vec<(LocalizedString, Theme)>>,
    /// Whether to append a "System" (follow-OS) entry. Default `true`.
    include_system: bool,
    /// The inner ComboBox's value signal. Owned here so the theme-sync effect
    /// can keep it aligned with the active theme.
    selected: Signal<Option<ThemeChoice>>,
    /// Optional plain single-line tooltip. Mutually exclusive with the rich /
    /// composite slots (the last tooltip setter called wins).
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source — registry key or inline content.
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body — a CK3-style arbitrary widget tree.
    composite_tooltip_content: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
}

impl Default for ThemeSwitcher {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ThemeSwitcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemeSwitcher")
            .field("variant", &self.variant)
            .field("include_system", &self.include_system)
            .finish()
    }
}

/// The translated label for a default entry, by theme id. Returns a reactive
/// [`LocalizedString`] (from `tr_widget!`) that re-resolves on locale change;
/// without an `I18nManager` the macro itself falls back to the English literal.
fn default_label(id: &str) -> Option<LocalizedString> {
    match id {
        "intui.light" => Some(tr_widget!(theme_switcher_light())),
        "intui.dark" => Some(tr_widget!(theme_switcher_dark())),
        "system" => Some(tr_widget!(theme_switcher_system())),
        _ => None,
    }
}

impl ThemeSwitcher {
    /// Create a switcher offering Light / Dark / System (the System entry
    /// follows the OS theme live).
    pub fn new() -> Self {
        Self {
            variant: ComboBoxVariant::default(),
            label: None,
            themes_override: None,
            include_system: true,
            selected: Signal::new(None),
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            root_child_id: None,
        }
    }

    /// Pick the inner ComboBox's design-language variant.
    pub fn variant(mut self, variant: ComboBoxVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the accessible / control label (defaults to the translated "Theme").
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Replace the default Light/Dark fixed-theme list with an app-supplied set
    /// of `(label, theme)` pairs — e.g. the `bastyde-theme-*` presets. The
    /// System (follow-OS) entry is still appended unless [`system`](Self::system)
    /// is `false`.
    pub fn themes(
        mut self,
        themes: impl IntoIterator<Item = (impl Into<LocalizedString>, Theme)>,
    ) -> Self {
        self.themes_override = Some(
            themes
                .into_iter()
                .map(|(label, theme)| (label.into(), theme))
                .collect(),
        );
        self
    }

    /// Whether to offer the "System" (follow-OS) entry. Default `true`.
    pub fn system(mut self, include: bool) -> Self {
        self.include_system = include;
        self
    }

    /// Attach a plain single-line tooltip shown after a hover delay.
    ///
    /// The three tooltip setters are mutually exclusive — `tooltip` /
    /// [`rich_tooltip`](Self::rich_tooltip) /
    /// [`rich_tooltip_content`](Self::rich_tooltip_content) /
    /// [`composite_tooltip`](Self::composite_tooltip) — and the last one
    /// called wins (each clears the others).
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip registry
    /// (inline markup, a shortcut chip, a "more" disclosure). See
    /// [`Button::rich_tooltip`](crate::button::Button::rich_tooltip).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by an inline
    /// [`TooltipContent`](crate::tooltip::TooltipContent) rather than a
    /// registry key.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — the third tier, hosting an arbitrary
    /// widget tree (headings, formatted paragraphs, controls). Handy for
    /// explaining a non-obvious behaviour: e.g. that switching theme
    /// *family* at runtime only re-tints colours, while widget chrome
    /// (Material 3's pill buttons, switch, card radii) is chosen when the UI
    /// is built. See
    /// [`Button::composite_tooltip`](crate::button::Button::composite_tooltip).
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Build the `(display, id, action)` entries for the current configuration.
    fn entries(&self) -> Vec<(String, ThemeId, ThemeAction)> {
        let mut out: Vec<(String, ThemeId, ThemeAction)> = Vec::new();
        match &self.themes_override {
            Some(custom) => {
                for (label, theme) in custom {
                    out.push((
                        label.resolve_now(),
                        theme.id.clone(),
                        ThemeAction::Set(Box::new(theme.clone())),
                    ));
                }
            }
            None => {
                let light = bastyde_core::presets::intui::light();
                let dark = bastyde_core::presets::intui::dark();
                // `tr_widget!` already falls back to the English literal when no
                // manager is installed, so `resolve_now()` yields "Light"/"Dark"
                // without a separate fallback. (The visible item labels are
                // re-derived reactively in `build()` via `default_label`.)
                out.push((
                    tr_widget!(theme_switcher_light()).resolve_now(),
                    light.id.clone(),
                    ThemeAction::Set(Box::new(light)),
                ));
                out.push((
                    tr_widget!(theme_switcher_dark()).resolve_now(),
                    dark.id.clone(),
                    ThemeAction::Set(Box::new(dark)),
                ));
            }
        }
        if self.include_system {
            out.push((
                tr_widget!(theme_switcher_system()).resolve_now(),
                ThemeId::new("system"),
                ThemeAction::FollowSystem,
            ));
        }
        out
    }
}

impl Widget for ThemeSwitcher {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let entries = self.entries();
        let choices: Vec<ThemeChoice> = entries
            .iter()
            .map(|(display, id, _)| ThemeChoice {
                id: id.clone(),
                display: display.clone(),
            })
            .collect();
        // id → action lookup for on_select (a handful of entries; linear scan).
        let actions: Rc<Vec<(ThemeId, ThemeAction)>> = Rc::new(
            entries
                .into_iter()
                .map(|(_, id, action)| (id, action))
                .collect(),
        );

        // Seed the selection from the active theme's id so the closed combo
        // shows the current theme.
        let current_id = ctx.theme().id.clone();
        let initial = choices.iter().find(|c| c.id == current_id).cloned();
        self.selected.set(initial);

        // Pass the reactive `LocalizedString` straight through (don't pre-resolve
        // with `lit!`, which would freeze the control label at the build-time
        // locale). The AT tree re-walks on a locale change and re-resolves it.
        let label = self
            .label
            .clone()
            .unwrap_or_else(|| tr_widget!(theme_switcher_label()));

        let on_select_actions = actions.clone();
        let combo =
            ComboBox::from_items(choices.clone(), self.selected.clone(), |c: &ThemeChoice| {
                // Default entries re-derive their label from the id so the
                // visible item text follows a locale change; custom themes use
                // the app-supplied (already-resolved) label.
                default_label(c.id.as_str())
                    .unwrap_or_else(|| LocalizedString::literal(c.display.clone()))
            })
            .variant(self.variant)
            .label(label)
            // The reason this widget needs `ComboBox::on_select` (not a plain signal
            // observer): both `set_theme` and `follow_system_theme` live on
            // `EventContext`, which `ctx.effect` can't provide.
            .on_select(move |c: &ThemeChoice, ctx| {
                if let Some((_, action)) = on_select_actions.iter().find(|(id, _)| *id == c.id) {
                    match action {
                        ThemeAction::Set(theme) => ctx.set_theme((**theme).clone()),
                        ThemeAction::FollowSystem => ctx.follow_system_theme(),
                    }
                }
            });
        let combo_id = ctx.add(combo);
        self.root_child_id = Some(combo_id);

        // Keep the selection aligned if the theme changes elsewhere (a menu,
        // the inspector, an OS light/dark toggle). Matched by stable id.
        {
            let selected = self.selected.clone();
            let choices = choices.clone();
            ctx.effect(&ctx.theme_signal(), move |theme| {
                let next = choices.iter().find(|c| c.id == theme.id).cloned();
                if selected.get() != next {
                    selected.set(next);
                }
            });
        }

        // Tooltip — three mutually-exclusive setters; setters clear the others
        // so at most one branch runs. Anchored on the inner ComboBox (the
        // visible control) so it shows on hover, independent of the dropdown.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, combo_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, combo_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(combo_id, tooltip_id, delay);
        }

        vec![combo_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // The inner ComboBox carries the control role + label.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    fn light_tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
    }

    #[test]
    fn default_entries_are_light_dark_system() {
        let sw = ThemeSwitcher::new();
        let entries = sw.entries();
        let ids: Vec<&str> = entries.iter().map(|(_, id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["intui.light", "intui.dark", "system"]);
        // No I18nManager installed in tests → English literal fallback.
        let labels: Vec<&str> = entries.iter().map(|(d, _, _)| d.as_str()).collect();
        assert_eq!(labels, vec!["Light", "Dark", "System"]);
    }

    #[test]
    fn system_can_be_disabled() {
        let entries = ThemeSwitcher::new().system(false).entries();
        let ids: Vec<&str> = entries.iter().map(|(_, id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["intui.light", "intui.dark"]);
    }

    #[test]
    fn builds_and_lays_out() {
        let mut tree = light_tree();
        let id = tree.add(ThemeSwitcher::new());
        tree.layout(SizeProposal::exact(240.0, 40.0));
        assert!(tree.bounds(id).width > 0.0);
    }

    // The key handler lives on the inner ComboBox; focus it directly.
    fn inner_combo(tree: &WidgetTree, id: WidgetId) -> WidgetId {
        tree.children(id)
            .first()
            .copied()
            .expect("ThemeSwitcher should wrap one ComboBox child")
    }

    #[test]
    fn selecting_dark_row_queues_theme_change() {
        let mut tree = light_tree();
        let id = tree.add(ThemeSwitcher::new());
        tree.layout(SizeProposal::exact(240.0, 240.0));

        let combo = inner_combo(&tree, id);
        tree.focus(combo);
        // Seeded at Light (entry 0); ArrowDown commits Dark (entry 1), firing
        // on_select → set_theme, which parks a pending theme request.
        tree.press_key(
            bastyde_core::event::Key::ArrowDown,
            bastyde_core::event::Modifiers::NONE,
        );
        let pending = tree.take_pending_theme_request();
        assert!(
            pending.is_some(),
            "selecting Dark must queue a theme switch"
        );
        assert_eq!(pending.unwrap().id.as_str(), "intui.dark");
    }

    #[test]
    fn tooltip_appears_after_hover_delay() {
        use bastyde_canvas::MockTextBackend;
        use std::cell::RefCell;
        use std::time::Duration;

        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        let id = tree.add(ThemeSwitcher::new().tooltip(bastyde_i18n::lit!("Pick the app theme")));
        tree.layout(SizeProposal::exact(240.0, 40.0));

        assert!(tree.active_overlays().is_empty());
        tree.pointer_move(tree.bounds(id).center());
        // Not instant — the tooltip waits for the hover delay.
        assert!(tree.active_overlays().is_empty());
        tree.advance_time(Duration::from_secs(2));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "ThemeSwitcher tooltip should appear after the hover delay"
        );
    }

    #[test]
    fn selecting_system_row_requests_follow_os() {
        let mut tree = light_tree();
        let id = tree.add(ThemeSwitcher::new());
        tree.layout(SizeProposal::exact(240.0, 240.0));

        let combo = inner_combo(&tree, id);
        tree.focus(combo);
        // Light → Dark → System: two ArrowDowns land on System.
        tree.press_key(
            bastyde_core::event::Key::ArrowDown,
            bastyde_core::event::Modifiers::NONE,
        );
        let _ = tree.take_pending_theme_request(); // clear the Dark request
        tree.press_key(
            bastyde_core::event::Key::ArrowDown,
            bastyde_core::event::Modifiers::NONE,
        );
        assert!(
            tree.take_pending_follow_system_request(),
            "selecting System must request follow-OS mode"
        );
    }
}
