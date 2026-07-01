// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! FontPicker — a drop-in font-family selector.
//!
//! A [`ComboBox`] preset that lists every installed font family and lets
//! the user pick one, in the tradition of Qt's `QFontComboBox`, GTK's
//! `FontChooser`, and UIKit's `UIFontPickerViewController`. It
//!
//! - **self-populates** from the app's shared typesetter
//!   (`ctx.app_state::<SharedTypesetter>()` → `families()`), so no font
//!   list is passed in;
//! - **previews each font**: every row shows the family name in a legible
//!   system font next to a tiny sample rendered *in that font*
//!   ([`FontPreviewMode::NameThenSample`], the default), and the closed
//!   trigger shows the selected family in its own typeface;
//! - is **searchable** (type to filter hundreds of fonts) and
//!   **filterable** by spacing ([`FontSpacingFilter`]) and by writing
//!   system ([`WritingSystem`]);
//! - binds the choice to a `Signal<Option<String>>` (the family name), which
//!   plugs straight into `TextStyle.family` / `RichTextEditor::set_font_family`.
//!
//! ```ignore
//! let family: Signal<Option<String>> = Signal::new(None);
//! VStack::new()
//!     .child(TextWidget::new(tr!(font())).style(TextStyleRole::BodyBold))
//!     .child(FontPicker::new(family.clone())
//!         .on_select(|name, _ctx| editor.set_font_family(name)));
//! ```
//!
//! # Writing-system detection is off-thread
//!
//! Classifying which scripts a font covers parses its OS/2 table, i.e.
//! reads the font file — hundreds of reads for a full system. The picker
//! therefore builds the coverage index on a background thread the first
//! time it mounts and polls readiness on the frame tick; until the index is
//! ready the writing-system filter shows the unfiltered list and samples
//! fall back to a Latin default. Spacing (monospaced / proportional)
//! filtering is instant (it uses only font metadata, no bytes).
//!
//! Only family selection is offered, matching Qt's `QFontComboBox`. Face /
//! weight / size selection belongs to a larger font *dialog* and is out of
//! scope.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::frame_tick_scheduler::FrameTickSubscription;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::styles::{ComboBoxStyle, ComboBoxStyleConfig, SharedComboBoxStyle};
use bastyde_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{LocalizedString, lit, tr_widget};
use bastyde_text::{FontFamilyInfo, SharedTypesetter, WritingSystem, WritingSystemSet};
use bastyde_tokens::{TextStyle, TextStyleRole};

use crate::combo_box::{ComboBox, ComboBoxVariant};
use crate::primitives::{HStack, Spacer, TextWidget};

/// Spacing filter, mirroring the monospaced / proportional axis of Qt's
/// `QFontComboBox::FontFilters`. Cheap — it reads only font metadata.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FontSpacingFilter {
    /// Show all fonts (default).
    #[default]
    Any,
    /// Only monospaced fonts.
    Monospaced,
    /// Only proportional (non-monospaced) fonts.
    Proportional,
}

/// How each row — and the closed trigger — previews a font.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FontPreviewMode {
    /// Family name in a legible system font, then a tiny sample rendered in
    /// the font itself (the default). The sample text is chosen for the
    /// font's writing system.
    #[default]
    NameThenSample,
    /// Family name rendered in its own typeface (the Qt / UIKit default).
    NameInOwnFont,
    /// Family name in the system font, no in-font sample (UIKit
    /// `displayUsingSystemFont`). Maximum legibility.
    NameInSystemFont,
}

/// Per-family metadata for headless testing / restricted font sets via
/// [`FontPicker::families_with_meta`]. In a real app this data comes from
/// the shared typesetter instead.
#[derive(Clone, Debug, Default)]
pub struct FontMeta {
    /// Whether the family is monospaced (drives [`FontSpacingFilter`]).
    pub monospaced: bool,
    /// The scripts the family covers (drives the writing-system filter and
    /// the per-row sample text).
    pub writing_systems: WritingSystemSet,
}

/// A font-family selector built on [`ComboBox`]. See the module docs.
pub struct FontPicker {
    /// The bound family name — the source of truth. Passes straight through
    /// to the inner ComboBox.
    selected: Signal<Option<String>>,
    /// Explicit family list. `None` ⇒ enumerate from the shared typesetter.
    families_override: Option<Vec<FontFamilyInfo>>,
    /// Explicit writing-system coverage (from `families_with_meta`). `None`
    /// ⇒ built on a background thread from the typesetter.
    meta_override: Option<HashMap<String, WritingSystemSet>>,

    spacing_filter: Prop<FontSpacingFilter>,
    writing_system: Prop<Option<WritingSystem>>,
    preview_mode: FontPreviewMode,
    sample_global: Option<String>,
    sample_by_ws: HashMap<WritingSystem, String>,
    sample_by_family: HashMap<String, String>,
    show_selected_in_own_font: bool,

    placeholder: Option<LocalizedString>,
    label: Option<LocalizedString>,
    initial_enabled: bool,
    variant: ComboBoxVariant,
    style_override: Option<SharedComboBoxStyle>,
    max_visible_items: Option<usize>,
    searchable: bool,
    search_query: Option<Signal<String>>,
    on_select: Option<Rc<dyn Fn(&str, &mut EventContext)>>,

    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn Widget>>,

    // ── Runtime state (persists across rebuilds) ──
    /// The master family list, refreshed each build.
    all: Rc<RefCell<Vec<FontFamilyInfo>>>,
    /// Writing-system coverage map — empty until the index is ready.
    meta: Rc<RefCell<HashMap<String, WritingSystemSet>>>,
    /// Whether `meta` is authoritative (override present, or index built).
    meta_ready: Rc<Cell<bool>>,
    /// The filtered item source handed to the ComboBox. Mutated by
    /// `replace_all` on every filter change (reactive — no combo rebuild).
    model: bastyde_data::ListModel<String>,
    /// The names last pushed into `model`, so an unrelated rebuild (locale /
    /// theme / ancestor) that recomputes an identical list doesn't churn the
    /// (possibly-open) dropdown with a redundant `replace_all`.
    last_names: Rc<RefCell<Vec<String>>>,
    /// In-flight background index: a readiness flag + the result slot.
    index_handle: Option<(
        Arc<AtomicBool>,
        Arc<Mutex<Option<HashMap<String, WritingSystemSet>>>>,
    )>,
    index_started: bool,
    /// Bumped once when the background index completes, to force a single
    /// rebuild that re-applies the filter and stops the readiness poll.
    rev: Signal<u64>,
    frame_tick_sub: Option<FrameTickSubscription>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for FontPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontPicker")
            .field("preview_mode", &self.preview_mode)
            .field("searchable", &self.searchable)
            .finish_non_exhaustive()
    }
}

impl FontPicker {
    /// Create a picker bound to `selected` (the chosen family name). The
    /// list is enumerated from the app's shared typesetter at build time.
    pub fn new(selected: Signal<Option<String>>) -> Self {
        Self {
            selected,
            families_override: None,
            meta_override: None,
            spacing_filter: Prop::Static(FontSpacingFilter::Any),
            writing_system: Prop::Static(None),
            preview_mode: FontPreviewMode::default(),
            sample_global: None,
            sample_by_ws: HashMap::new(),
            sample_by_family: HashMap::new(),
            show_selected_in_own_font: true,
            placeholder: None,
            label: None,
            initial_enabled: true,
            variant: ComboBoxVariant::default(),
            style_override: None,
            max_visible_items: None,
            searchable: true,
            search_query: None,
            on_select: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            all: Rc::new(RefCell::new(Vec::new())),
            meta: Rc::new(RefCell::new(HashMap::new())),
            meta_ready: Rc::new(Cell::new(false)),
            model: bastyde_data::ListModel::new(),
            last_names: Rc::new(RefCell::new(Vec::new())),
            index_handle: None,
            index_started: false,
            rev: Signal::new(0),
            frame_tick_sub: None,
            root_child_id: None,
        }
    }

    /// Override the family list instead of enumerating from the typesetter.
    /// Family names only — spacing is treated as proportional and
    /// writing-system coverage is unknown (the writing-system filter shows
    /// all). For deterministic filter tests, prefer
    /// [`families_with_meta`](Self::families_with_meta).
    pub fn families(mut self, families: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut list: Vec<FontFamilyInfo> = families
            .into_iter()
            .map(|n| FontFamilyInfo {
                name: n.into(),
                monospaced: false,
            })
            .collect();
        // Present alphabetized, like the typesetter-backed path.
        list.sort_by_key(|f| f.name.to_lowercase());
        self.families_override = Some(list);
        self.meta_override = None;
        self
    }

    /// Override the family list *and* its metadata (monospaced + writing
    /// systems). Enables headless testing of the spacing / writing-system
    /// filters and the script-aware sample without a font backend.
    pub fn families_with_meta(mut self, families: Vec<(String, FontMeta)>) -> Self {
        let mut list = Vec::with_capacity(families.len());
        let mut meta = HashMap::with_capacity(families.len());
        for (name, m) in families {
            // Coverage is keyed by the lowercased name — the same convention
            // the typesetter-backed index uses — so lookups agree regardless
            // of the display casing (see `passes` / `sample_for`).
            meta.insert(name.to_lowercase(), m.writing_systems);
            list.push(FontFamilyInfo {
                name,
                monospaced: m.monospaced,
            });
        }
        list.sort_by_key(|f| f.name.to_lowercase());
        self.families_override = Some(list);
        self.meta_override = Some(meta);
        self
    }

    /// Restrict the list by spacing (monospaced / proportional). Accepts a
    /// static value or a `Signal` for a reactive filter toolbar.
    pub fn spacing_filter(mut self, filter: impl Into<Prop<FontSpacingFilter>>) -> Self {
        self.spacing_filter = filter.into();
        self
    }

    /// Restrict the list to fonts covering a writing system. `None` shows
    /// all. Accepts a static value or a `Signal`. The first time a
    /// non-`None` value is applied, the coverage index is built off-thread;
    /// until it is ready the list is unfiltered.
    pub fn writing_system(mut self, ws: impl Into<Prop<Option<WritingSystem>>>) -> Self {
        self.writing_system = ws.into();
        self
    }

    /// Choose how rows (and the trigger) preview each font. Default
    /// [`FontPreviewMode::NameThenSample`].
    pub fn preview_mode(mut self, mode: FontPreviewMode) -> Self {
        self.preview_mode = mode;
        self
    }

    /// Convenience: `true` keeps the default preview; `false` switches to
    /// [`FontPreviewMode::NameInSystemFont`] (UIKit `displayUsingSystemFont`).
    pub fn preview_in_own_font(mut self, on: bool) -> Self {
        if !on {
            self.preview_mode = FontPreviewMode::NameInSystemFont;
        }
        self
    }

    /// Global sample text override (used when the font's writing system has
    /// no more specific sample). Mirrors GTK's preview text.
    pub fn sample_text(mut self, text: impl Into<String>) -> Self {
        self.sample_global = Some(text.into());
        self
    }

    /// Per-writing-system sample override (Qt `setSampleTextForSystem`).
    pub fn sample_text_for(mut self, ws: WritingSystem, text: impl Into<String>) -> Self {
        self.sample_by_ws.insert(ws, text.into());
        self
    }

    /// Per-family sample override (Qt `setSampleTextForFont`) — for fonts
    /// whose script the generic sample doesn't suit (icon fonts, etc.).
    pub fn sample_text_for_family(
        mut self,
        family: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        // Keyed lowercase to match the family-name lookup in `sample_for`.
        self.sample_by_family
            .insert(family.into().to_lowercase(), text.into());
        self
    }

    /// Whether the closed trigger renders the selected family in its own
    /// typeface (default `true`; Qt behaviour). No effect in
    /// [`FontPreviewMode::NameInSystemFont`].
    pub fn show_selected_in_own_font(mut self, on: bool) -> Self {
        self.show_selected_in_own_font = on;
        self
    }

    /// Placeholder shown when nothing is selected. Defaults to a localized
    /// "Select a font…".
    pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// Accessible / control label. Defaults to a localized "Font".
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Enable / disable the control.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Design-language variant, forwarded to the inner [`ComboBox`].
    pub fn variant(mut self, variant: ComboBoxVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Per-call [`ComboBoxStyle`] override, forwarded to the inner combo.
    pub fn style(mut self, style: impl ComboBoxStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Maximum rows shown before the dropdown scrolls (default 8).
    pub fn max_visible_items(mut self, n: usize) -> Self {
        self.max_visible_items = Some(n);
        self
    }

    /// Enable / disable the in-dropdown search field (default `true`).
    pub fn searchable(mut self, on: bool) -> Self {
        self.searchable = on;
        self
    }

    /// Drive the search field from an external query signal (implies
    /// `searchable`).
    pub fn search_query(mut self, query: Signal<String>) -> Self {
        self.search_query = Some(query);
        self.searchable = true;
        self
    }

    /// React to a commit with a live [`EventContext`] — the place to apply
    /// the chosen font (e.g. `editor.set_font_family(name)`).
    pub fn on_select(mut self, f: impl Fn(&str, &mut EventContext) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Attach a plain tooltip, forwarded to the inner [`ComboBox`].
    /// Mutually exclusive with the rich / composite variants — last-call-wins.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a registry-keyed rich tooltip, forwarded to the inner combo.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach an inline rich tooltip, forwarded to the inner combo.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip hosting an arbitrary widget tree,
    /// forwarded to the inner combo.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Kick off the background writing-system coverage index (once), when a
    /// typesetter is available and no explicit meta was supplied.
    fn maybe_start_index(&mut self, ctx: &BuildContext) {
        if self.index_started || self.meta_override.is_some() {
            return;
        }
        let Some(ts) = ctx.app_state::<SharedTypesetter>() else {
            return;
        };
        let builder = ts.bridge().borrow().writing_system_index_builder();
        let ready = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let ready_t = ready.clone();
        let result_t = result.clone();
        std::thread::spawn(move || {
            let map = builder.build();
            if let Ok(mut slot) = result_t.lock() {
                *slot = Some(map);
            }
            ready_t.store(true, Ordering::Release);
        });
        self.index_handle = Some((ready, result));
        self.index_started = true;
    }
}

impl Widget for FontPicker {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Rebuild once when the background index reports ready (via `rev`).
        self.rev
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Refresh the master family list + coverage from overrides or the
        // shared typesetter.
        let families = self
            .families_override
            .clone()
            .or_else(|| {
                ctx.app_state::<SharedTypesetter>()
                    .map(|ts| ts.bridge().borrow().families())
            })
            .unwrap_or_default();
        *self.all.borrow_mut() = families;

        if let Some(meta) = &self.meta_override {
            *self.meta.borrow_mut() = meta.clone();
            self.meta_ready.set(true);
        } else {
            self.maybe_start_index(ctx);
        }

        // The reactive filter: recompute the visible names and push them
        // into the model. `replace_all` bumps the model version, so an open
        // dropdown re-filters live (no ComboBox rebuild).
        let recompute: Rc<dyn Fn()> = {
            let all = self.all.clone();
            let meta = self.meta.clone();
            let meta_ready = self.meta_ready.clone();
            let spacing = self.spacing_filter.clone();
            let ws = self.writing_system.clone();
            let selected = self.selected.clone();
            let model = self.model.clone();
            let last_names = self.last_names.clone();
            Rc::new(move || {
                let all = all.borrow();
                let meta = meta.borrow();
                let ready = meta_ready.get();
                let sp = spacing.get();
                let w = ws.get();
                let mut names: Vec<String> = all
                    .iter()
                    .filter(|info| passes(info, sp, w, ready, &meta))
                    .map(|info| info.name.clone())
                    .collect();
                // Keep the current selection visible even if the filter
                // would exclude it, so a filter change never silently clears
                // the user's choice.
                if let Some(sel) = selected.get()
                    && !names.iter().any(|n| n == &sel)
                {
                    names.push(sel);
                    names.sort_by_key(|n| n.to_lowercase());
                }
                // Only touch the model when the list actually changed, so an
                // unrelated rebuild doesn't churn an open dropdown.
                if *last_names.borrow() != names {
                    *last_names.borrow_mut() = names.clone();
                    model.replace_all(names);
                }
            })
        };
        recompute();

        // Re-filter when a bound spacing / writing-system signal changes.
        if let Prop::Bound(s) = &self.spacing_filter {
            let rc = recompute.clone();
            ctx.effect(s, move |_| rc());
        }
        if let Prop::Bound(s) = &self.writing_system {
            let rc = recompute.clone();
            ctx.effect(s, move |_| rc());
        }

        // Poll the background index on the frame tick; on ready, populate
        // `meta`, re-filter, and bump `rev` to rebuild once (which drops the
        // subscription and stops the poll).
        let pending = self.index_handle.is_some() && !self.meta_ready.get();
        if pending {
            let handle = self.index_handle.clone();
            let meta = self.meta.clone();
            let meta_ready = self.meta_ready.clone();
            let rev = self.rev.clone();
            let rc = recompute.clone();
            ctx.effect(&ctx.frame_tick(), move |_| {
                if meta_ready.get() {
                    return;
                }
                let Some((ready, result)) = &handle else {
                    return;
                };
                if !ready.load(Ordering::Acquire) {
                    return;
                }
                if let Ok(mut slot) = result.lock()
                    && let Some(map) = slot.take()
                {
                    *meta.borrow_mut() = map;
                    meta_ready.set(true);
                    rc();
                    rev.set(rev.get().wrapping_add(1));
                }
            });
            self.frame_tick_sub = Some(ctx.subscribe_frame_tick());
        } else {
            // Index ready (or none): stop polling.
            self.frame_tick_sub = None;
        }

        // Build the inner ComboBox over the reactive model.
        let base_style = ctx.theme().typography.body.clone();
        let mut combo =
            ComboBox::from_model(self.model.clone(), self.selected.clone(), |s: &String| {
                LocalizedString::literal(s.clone())
            })
            .variant(self.variant)
            .searchable(self.searchable)
            .enabled(self.initial_enabled)
            .label(
                self.label
                    .clone()
                    .unwrap_or_else(|| tr_widget!(font_picker_label())),
            )
            .placeholder(
                self.placeholder
                    .clone()
                    .unwrap_or_else(|| tr_widget!(font_picker_placeholder())),
            );

        // Per-row preview.
        {
            let meta = self.meta.clone();
            let meta_ready = self.meta_ready.clone();
            let mode = self.preview_mode;
            let global = self.sample_global.clone();
            let by_ws = self.sample_by_ws.clone();
            let by_family = self.sample_by_family.clone();
            let base = base_style.clone();
            combo = combo.render_item(move |name: &String, _selected: bool| {
                build_font_row(
                    name,
                    &meta,
                    meta_ready.get(),
                    mode,
                    &global,
                    &by_ws,
                    &by_family,
                    &base,
                )
            });
        }

        // Trigger-in-own-font (Qt behaviour), unless system-font mode.
        if self.show_selected_in_own_font && self.preview_mode != FontPreviewMode::NameInSystemFont
        {
            let base = base_style.clone();
            combo = combo.render_selected(move |name: &String| {
                Box::new(
                    TextWidget::new(lit!(name.clone()))
                        .style(TextStyle {
                            family: name.clone(),
                            ..base.clone()
                        })
                        .single_line(),
                )
            });
        }

        if let Some(n) = self.max_visible_items {
            combo = combo.max_visible_items(n);
        }
        if let Some(q) = &self.search_query {
            combo = combo.search_query(q.clone());
        }
        if let Some(style) = &self.style_override {
            combo = combo.style(SharedStyleAdapter(style.clone()));
        }
        if let Some(cb) = &self.on_select {
            let cb = cb.clone();
            combo = combo.on_select(move |s: &String, ctx| cb(s.as_str(), ctx));
        }

        // Forward exactly one configured tooltip (last-call-wins upstream).
        if let Some(content) = self.composite_tooltip_content.take() {
            combo = combo.composite_tooltip_boxed(content);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            combo = match source {
                crate::tooltip::RichTooltipSource::Key(k) => combo.rich_tooltip(k),
                crate::tooltip::RichTooltipSource::Content(c) => combo.rich_tooltip_content(c),
            };
        } else if let Some(text) = self.tooltip_text.clone() {
            combo = combo.tooltip(text);
        }

        let combo_id = ctx.add(combo);
        self.root_child_id = Some(combo_id);
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
        // The inner ComboBox carries the control role, label, value, and the
        // listbox / option a11y for the dropdown.
    }
}

/// Filter predicate: does this family pass the spacing + writing-system
/// filters? While the coverage index is not ready, a writing-system filter
/// is inert (shows all) rather than hiding fonts we can't yet classify.
fn passes(
    info: &FontFamilyInfo,
    spacing: FontSpacingFilter,
    ws: Option<WritingSystem>,
    meta_ready: bool,
    meta: &HashMap<String, WritingSystemSet>,
) -> bool {
    let spacing_ok = match spacing {
        FontSpacingFilter::Any => true,
        FontSpacingFilter::Monospaced => info.monospaced,
        FontSpacingFilter::Proportional => !info.monospaced,
    };
    if !spacing_ok {
        return false;
    }
    match ws {
        None => true,
        Some(ws) if !meta_ready => {
            let _ = ws;
            true
        }
        // Coverage map is keyed by lowercased family name (see
        // `families_with_meta` / the typesetter index builder).
        Some(ws) => meta
            .get(&info.name.to_lowercase())
            .is_some_and(|set| set.contains(ws)),
    }
}

/// Pick the most illustrative writing system for a font's sample: prefer a
/// non-Latin, non-Symbol script (more distinctive), else Latin, else
/// whatever is present.
fn representative_ws(set: WritingSystemSet) -> Option<WritingSystem> {
    let mut has_latin = false;
    for ws in set.iter() {
        match ws {
            WritingSystem::Latin => has_latin = true,
            WritingSystem::Symbol => {}
            other => return Some(other),
        }
    }
    if has_latin {
        Some(WritingSystem::Latin)
    } else {
        set.iter().next()
    }
}

/// The sample string to render *in* a font for its row, or `None` for
/// name-only. Order: per-family override → per-writing-system override /
/// script default → global override → Latin default.
fn sample_for(
    name: &str,
    meta: &Rc<RefCell<HashMap<String, WritingSystemSet>>>,
    meta_ready: bool,
    global: &Option<String>,
    by_ws: &HashMap<WritingSystem, String>,
    by_family: &HashMap<String, String>,
) -> Option<String> {
    if let Some(s) = by_family.get(&name.to_lowercase()) {
        return Some(s.clone());
    }
    if meta_ready
        && let Some(set) = meta.borrow().get(&name.to_lowercase()).copied()
        && let Some(ws) = representative_ws(set)
    {
        if let Some(s) = by_ws.get(&ws) {
            return Some(s.clone());
        }
        return Some(ws.sample_text().to_string());
    }
    if let Some(g) = global {
        return Some(g.clone());
    }
    Some(WritingSystem::Latin.sample_text().to_string())
}

/// Build one dropdown row for a family. The family name renders in a
/// legible system font (and is the a11y name via the ComboBox row wrapper);
/// the sample renders in the font itself and is hidden from AT.
#[allow(clippy::too_many_arguments)]
fn build_font_row(
    name: &str,
    meta: &Rc<RefCell<HashMap<String, WritingSystemSet>>>,
    meta_ready: bool,
    mode: FontPreviewMode,
    global: &Option<String>,
    by_ws: &HashMap<WritingSystem, String>,
    by_family: &HashMap<String, String>,
    base: &TextStyle,
) -> Box<dyn Widget> {
    match mode {
        FontPreviewMode::NameInOwnFont => Box::new(
            TextWidget::new(lit!(name.to_string()))
                .style(TextStyle {
                    family: name.to_string(),
                    ..base.clone()
                })
                .single_line()
                .a11y_hidden(),
        ),
        FontPreviewMode::NameInSystemFont => Box::new(
            TextWidget::new(lit!(name.to_string()))
                .style(TextStyleRole::Body)
                .single_line()
                .a11y_hidden(),
        ),
        FontPreviewMode::NameThenSample => {
            let name_w = TextWidget::new(lit!(name.to_string()))
                .style(TextStyleRole::Body)
                .single_line()
                .a11y_hidden();
            let mut row = HStack::new()
                .spacing(12.0)
                .child(name_w)
                .child(Spacer::new());
            if let Some(sample) = sample_for(name, meta, meta_ready, global, by_ws, by_family) {
                row = row.child(
                    TextWidget::new(lit!(sample))
                        .style(TextStyle {
                            family: name.to_string(),
                            ..base.clone()
                        })
                        .single_line()
                        .a11y_hidden(),
                );
            }
            Box::new(row)
        }
    }
}

/// Adapts a stored `Rc<dyn ComboBoxStyle>` back into `impl ComboBoxStyle`
/// so `FontPicker::style` can forward it to `ComboBox::style` (which takes
/// the style by value).
struct SharedStyleAdapter(SharedComboBoxStyle);

impl ComboBoxStyle for SharedStyleAdapter {
    fn make_body(&self, cfg: &ComboBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        self.0.make_body(cfg, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    fn light_tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
    }

    fn ws(list: &[WritingSystem]) -> WritingSystemSet {
        let mut s = WritingSystemSet::new();
        for &w in list {
            s.insert(w);
        }
        s
    }

    fn info(name: &str, mono: bool) -> FontFamilyInfo {
        FontFamilyInfo {
            name: name.to_string(),
            monospaced: mono,
        }
    }

    #[test]
    fn passes_spacing_filter() {
        let mono = info("Courier", true);
        let prop = info("Arial", false);
        let empty = HashMap::new();
        assert!(passes(&mono, FontSpacingFilter::Any, None, false, &empty));
        assert!(passes(&prop, FontSpacingFilter::Any, None, false, &empty));
        assert!(passes(
            &mono,
            FontSpacingFilter::Monospaced,
            None,
            false,
            &empty
        ));
        assert!(!passes(
            &prop,
            FontSpacingFilter::Monospaced,
            None,
            false,
            &empty
        ));
        assert!(!passes(
            &mono,
            FontSpacingFilter::Proportional,
            None,
            false,
            &empty
        ));
        assert!(passes(
            &prop,
            FontSpacingFilter::Proportional,
            None,
            false,
            &empty
        ));
    }

    #[test]
    fn passes_writing_system_filter_respects_readiness() {
        // Display name "Arial", coverage keyed lowercase "arial" — exercises
        // the case-insensitive lookup in `passes`.
        let arial = info("Arial", false);
        let mut meta = HashMap::new();
        meta.insert("arial".to_string(), ws(&[WritingSystem::Latin]));
        // Index not ready → a writing-system filter is inert (shows all).
        assert!(passes(
            &arial,
            FontSpacingFilter::Any,
            Some(WritingSystem::Arabic),
            false,
            &meta
        ));
        // Ready → Latin font excluded by an Arabic filter, kept by a Latin one.
        assert!(!passes(
            &arial,
            FontSpacingFilter::Any,
            Some(WritingSystem::Arabic),
            true,
            &meta
        ));
        assert!(passes(
            &arial,
            FontSpacingFilter::Any,
            Some(WritingSystem::Latin),
            true,
            &meta
        ));
    }

    #[test]
    fn representative_ws_prefers_non_latin() {
        assert_eq!(
            representative_ws(ws(&[WritingSystem::Latin])),
            Some(WritingSystem::Latin)
        );
        assert_eq!(
            representative_ws(ws(&[WritingSystem::Latin, WritingSystem::Arabic])),
            Some(WritingSystem::Arabic)
        );
        assert_eq!(
            representative_ws(ws(&[WritingSystem::Symbol])),
            Some(WritingSystem::Symbol)
        );
        assert_eq!(representative_ws(WritingSystemSet::new()), None);
    }

    #[test]
    fn sample_for_precedence() {
        // Coverage + per-family samples are keyed lowercase; the lookups
        // (display names "NotoArabic" / "Wingdings") are case-folded.
        let meta = Rc::new(RefCell::new({
            let mut m = HashMap::new();
            m.insert("notoarabic".to_string(), ws(&[WritingSystem::Arabic]));
            m
        }));
        let mut by_family = HashMap::new();
        by_family.insert("wingdings".to_string(), "★☂".to_string());
        let mut by_ws = HashMap::new();
        by_ws.insert(WritingSystem::Arabic, "custom-ar".to_string());

        // Per-family override wins.
        assert_eq!(
            sample_for("Wingdings", &meta, true, &None, &by_ws, &by_family).as_deref(),
            Some("★☂")
        );
        // Per-writing-system override for the font's script.
        assert_eq!(
            sample_for("NotoArabic", &meta, true, &None, &by_ws, &by_family).as_deref(),
            Some("custom-ar")
        );
        // Font whose only script is Arabic → the Arabic default sample.
        assert_eq!(
            sample_for(
                "NotoArabic",
                &meta,
                true,
                &None,
                &HashMap::new(),
                &HashMap::new()
            ),
            Some(WritingSystem::Arabic.sample_text().to_string())
        );
        // Unknown font, meta ready → global override if set.
        assert_eq!(
            sample_for(
                "Mystery",
                &meta,
                true,
                &Some("g".to_string()),
                &HashMap::new(),
                &HashMap::new()
            )
            .as_deref(),
            Some("g")
        );
        // Meta not ready → Latin default.
        assert_eq!(
            sample_for(
                "Arial",
                &meta,
                false,
                &None,
                &HashMap::new(),
                &HashMap::new()
            ),
            Some(WritingSystem::Latin.sample_text().to_string())
        );
    }

    #[test]
    fn builds_and_lays_out_with_families() {
        let mut tree = light_tree();
        let sel = Signal::new(None::<String>);
        let id = tree.add(FontPicker::new(sel).families(["Arial", "Courier", "Times"]));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        assert!(tree.bounds(id).width > 0.0);
    }

    #[test]
    fn empty_without_backend_or_override_still_builds() {
        let mut tree = light_tree();
        let sel = Signal::new(None::<String>);
        let id = tree.add(FontPicker::new(sel));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        assert!(tree.bounds(id).width >= 0.0);
    }

    #[test]
    fn accessibility_is_combobox_role() {
        let mut tree = light_tree();
        let sel = Signal::new(Some("Arial".to_string()));
        let id = tree.add(
            FontPicker::new(sel)
                .families(["Arial", "Courier"])
                .label(lit!("Font family")),
        );
        tree.layout(SizeProposal::exact(300.0, 50.0));
        // The inner ComboBox carries the role; find it under the picker.
        let combo = tree.children(id)[0];
        let node = tree.accessibility_node(combo);
        assert_eq!(node.role(), bastyde_core::accesskit::Role::ComboBox);
        assert_eq!(node.name(), Some("Font family"));
    }

    #[test]
    fn reactive_spacing_filter_signal_drives_refilter_without_panic() {
        let mut tree = light_tree();
        let sel = Signal::new(None::<String>);
        let spacing = Signal::new(FontSpacingFilter::Any);
        let id = tree.add(
            FontPicker::new(sel)
                .families_with_meta(vec![
                    (
                        "Courier".to_string(),
                        FontMeta {
                            monospaced: true,
                            writing_systems: ws(&[WritingSystem::Latin]),
                        },
                    ),
                    (
                        "Arial".to_string(),
                        FontMeta {
                            monospaced: false,
                            writing_systems: ws(&[WritingSystem::Latin]),
                        },
                    ),
                ])
                .spacing_filter(spacing.clone()),
        );
        tree.layout(SizeProposal::exact(300.0, 50.0));
        // Flip the filter: the bound-signal effect fires + `replace_all`
        // runs; the widget must keep laying out.
        spacing.set(FontSpacingFilter::Monospaced);
        tree.layout(SizeProposal::exact(300.0, 50.0));
        assert!(tree.bounds(id).width > 0.0);
    }
}
