// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! LanguageSwitcher — a drop-in UI-language picker for settings screens.
//!
//! A thin [`ComboBox`] preset that lists the application's supported
//! locales and switches the active locale on selection. Each entry is
//! shown as its **endonym** — the language's own name — followed by the
//! BCP-47 tag, e.g. `français (fr-FR)`, `Deutsch (de-DE)`,
//! `العربية (ar-SA)`. Showing endonyms (not "French", "German", "Arabic")
//! means a speaker of each language can always find their own in the list.
//!
//! Zero-config: drop it into a settings panel and it
//!
//! - self-populates from the installed `I18nManager`
//!   (`bastyde_i18n::current_supported_locales()`),
//! - shows the active locale as the current selection
//!   (`bastyde_i18n::current_locale()`),
//! - switches the app locale on selection via `EventContext::set_locale`,
//!   which the window manager fans out to every window (re-translating
//!   text and flipping layout direction for RTL locales like Arabic),
//! - and keeps its selection in sync if the locale is changed elsewhere.
//!
//! ```ignore
//! // In a settings panel's build():
//! VStack::new()
//!     .child(TextWidget::new(tr!(ui_language())).style(TextStyleRole::BodyBold))
//!     .child(LanguageSwitcher::new())
//! ```
//!
//! Endonyms come from ICU4X CLDR data via
//! [`bastyde_i18n::language_endonym`]; an unknown tag falls back to the
//! raw BCP-47 tag. When no `I18nManager` is configured the switcher
//! renders an empty, placeholder ComboBox.

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{
    LanguageIdentifier, LocalizedString, current_locale, current_supported_locales,
    language_endonym, lit,
};

use crate::combo_box::{ComboBox, ComboBoxVariant};

/// One row in the switcher: the locale's BCP-47 `tag` (the value committed
/// to `set_locale`) and the user-facing `display` string
/// (`"<endonym> (<tag>)"`).
#[derive(Clone, PartialEq)]
struct LocaleChoice {
    tag: String,
    display: String,
}

/// A UI-language picker built on [`ComboBox`]. See the module docs.
pub struct LanguageSwitcher {
    /// Forwarded to the inner [`ComboBox`]. Defaults to `Outlined`.
    variant: ComboBoxVariant,
    /// Accessible / control label. Defaults to `lit!("Language")`; pass a
    /// `tr!(...)` to localize it.
    label: Option<LocalizedString>,
    /// Explicit locale list. When `None` (the default), the switcher reads
    /// the supported locales from the active `I18nManager`.
    locales_override: Option<Vec<LanguageIdentifier>>,
    /// The inner ComboBox's value signal. Owned here so the locale-sync
    /// effect can keep it aligned with the active locale.
    selected: Signal<Option<LocaleChoice>>,
    root_child_id: Option<WidgetId>,
}

impl Default for LanguageSwitcher {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LanguageSwitcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageSwitcher")
            .field("variant", &self.variant)
            .field("locales_override", &self.locales_override)
            .finish()
    }
}

impl LanguageSwitcher {
    /// Create a switcher that auto-discovers the supported locales from
    /// the active `I18nManager`.
    pub fn new() -> Self {
        Self {
            variant: ComboBoxVariant::default(),
            label: None,
            locales_override: None,
            selected: Signal::new(None),
            root_child_id: None,
        }
    }

    /// Pick the inner ComboBox's design-language variant.
    pub fn variant(mut self, variant: ComboBoxVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the accessible / control label (defaults to `"Language"`).
    /// Pass a `tr!(...)` to localize it.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Override the locale list instead of auto-discovering it from the
    /// active `I18nManager`. Useful in previews / tests, or to restrict
    /// the offered set.
    pub fn locales(mut self, locales: Vec<LanguageIdentifier>) -> Self {
        self.locales_override = Some(locales);
        self
    }

    /// Build the `"<endonym> (<tag>)"` choices for a locale list.
    fn choices_for(locales: &[LanguageIdentifier]) -> Vec<LocaleChoice> {
        locales
            .iter()
            .map(|l| {
                let tag = l.to_string();
                let endonym = language_endonym(l).unwrap_or_else(|| tag.clone());
                LocaleChoice {
                    display: format!("{endonym} ({tag})"),
                    tag,
                }
            })
            .collect()
    }
}

impl Widget for LanguageSwitcher {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let locales = self
            .locales_override
            .clone()
            .or_else(current_supported_locales)
            .unwrap_or_default();
        let choices = Self::choices_for(&locales);

        // Seed the selection from the active locale so the closed combo
        // shows the current language.
        let active_tag = current_locale().map(|s| s.get().to_string());
        let initial = active_tag
            .as_ref()
            .and_then(|t| choices.iter().find(|c| &c.tag == t).cloned());
        self.selected.set(initial);

        let label = self.label.clone().unwrap_or_else(|| lit!("Language"));

        let combo = ComboBox::from_items(
            choices.clone(),
            self.selected.clone(),
            |c: &LocaleChoice| LocalizedString::literal(c.display.clone()),
        )
        .variant(self.variant)
        .label(label)
        .placeholder(lit!("Language"))
        // The reason this widget needs `ComboBox::on_select` (not a plain
        // signal observer): `set_locale` lives on `EventContext`, so the
        // full window-manager fan-out (redraw-all + RTL layout direction)
        // only happens on this context-bearing path.
        .on_select(|c: &LocaleChoice, ctx| ctx.set_locale(c.tag.clone()));
        let combo_id = ctx.add(combo);
        self.root_child_id = Some(combo_id);

        // Keep the selection aligned if the locale is changed from
        // elsewhere (another switcher, a menu, the inspector). Endonym
        // strings are language-stable, so the choice list itself never
        // needs rebuilding on a locale change — only the selection.
        if let Some(locale_sig) = current_locale() {
            let selected = self.selected.clone();
            let choices = choices.clone();
            ctx.effect(&locale_sig, move |loc| {
                let tag = loc.to_string();
                let next = choices.iter().find(|c| c.tag == tag).cloned();
                if selected.get() != next {
                    selected.set(next);
                }
            });
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

    fn langs(tags: &[&str]) -> Vec<LanguageIdentifier> {
        tags.iter().map(|t| t.parse().unwrap()).collect()
    }

    #[test]
    fn choices_show_endonym_and_tag() {
        let choices = LanguageSwitcher::choices_for(&langs(&["fr-FR", "de-DE"]));
        assert_eq!(choices[0].tag, "fr-FR");
        assert_eq!(choices[0].display, "français (fr-FR)");
        assert_eq!(choices[1].display, "Deutsch (de-DE)");
    }

    #[test]
    fn unknown_tag_falls_back_to_raw_tag() {
        // A private-use tag has no CLDR endonym.
        let choices = LanguageSwitcher::choices_for(&langs(&["qaa"]));
        assert_eq!(choices[0].display, "qaa (qaa)");
    }

    #[test]
    fn builds_and_lays_out_with_explicit_locales() {
        let mut tree = light_tree();
        let id = tree.add(LanguageSwitcher::new().locales(langs(&["en-US", "fr-FR", "ar-SA"])));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        assert!(tree.bounds(id).width > 0.0);
    }

    #[test]
    fn empty_when_no_locales() {
        // No manager + no override → empty list, still builds without panic.
        let mut tree = light_tree();
        let id = tree.add(LanguageSwitcher::new());
        tree.layout(SizeProposal::exact(300.0, 50.0));
        assert!(tree.bounds(id).width >= 0.0);
    }
}
