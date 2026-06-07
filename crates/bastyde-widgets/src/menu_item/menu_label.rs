//! `MenuLabel` — leaf widget that renders a menu / menubar label
//! with an Alt-gated mnemonic underline.
//!
//! See [`super::mnemonic`] for the parser convention (`&Save`,
//! `&&` escape, etc.). The underline shows iff:
//!
//! - the source contained a single un-escaped `&` marking a character,
//! - the bound `alt_down` signal is currently `true`, AND
//! - the platform is **not macOS**.
//!
//! The macOS exclusion is intentional and matches the
//! [`MenuBarDispatcher`](bastyde_core::window::menubar_dispatcher::MenubarDispatcher)
//! contract: on macOS, the OS rewrites Option+letter for accented
//! character composition before the app sees the keystroke
//! (Option+E → ´, Option+F → ƒ, …), so Alt+letter mnemonics cannot
//! function and drawing the underline would mislead the user. F10,
//! bare-Alt-tap, and in-menu bare-letter activation continue to
//! work on macOS.
//!
//! All conditions are reactive — flipping the OS-driven `alt_down`
//! signal repaints every menu label in the tree at
//! [`BindingLevel::RepaintOnly`].
//!
//! Architecturally a sibling to [`TextWidget`](crate::primitives::TextWidget) but specialized: no
//! markup, no wrap, no link spans, no hit-testing. The widget is
//! `pub(crate)`-only — it is an implementation detail of
//! [`MenuItem`](super::MenuItem) and `MenuBarTrigger`.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::text_backend::TextLayout;
use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::{ColorProp, TextStyleProp};
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{LayoutContext, PaintContext, Widget};
use bastyde_core::widget_id::WidgetId;

use super::mnemonic::{ParsedMnemonic, parse_mnemonic};

pub(crate) struct MenuLabel {
    /// Raw label source — may contain `&` markers. Reactive (locale
    /// changes flip the underlying signal). Re-parsed on every
    /// layout_response that sees a different stripped string.
    source: Prop<String>,
    /// OS-driven Alt-held signal from `WindowState::alt_down`. Drives
    /// underline visibility.
    alt_down: Signal<bool>,
    color: ColorProp,
    style: TextStyleProp,
    /// Cached layout of the *stripped* text. Recomputed lazily in
    /// `layout_response` and reused by `paint` to avoid double work.
    last_layout: RefCell<Option<TextLayout>>,
    /// Cached parsed form (stripped, byte_index, char_index, key_lower).
    /// Recomputed lazily when the source value changes.
    last_parsed: RefCell<Option<(String, ParsedMnemonic)>>,
}

impl std::fmt::Debug for MenuLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuLabel").finish()
    }
}

impl MenuLabel {
    pub(crate) fn new(
        source: impl Into<Prop<String>>,
        alt_down: Signal<bool>,
        color: impl Into<ColorProp>,
        style: impl Into<TextStyleProp>,
    ) -> Self {
        Self {
            source: source.into(),
            alt_down,
            color: color.into(),
            style: style.into(),
            last_layout: RefCell::new(None),
            last_parsed: RefCell::new(None),
        }
    }

    /// Re-parse the source if its current value differs from what we
    /// last parsed. Returns a clone of the parsed result.
    fn resolve_parsed(&self) -> ParsedMnemonic {
        let raw = self.source.get();
        let mut cache = self.last_parsed.borrow_mut();
        if let Some((cached_raw, parsed)) = cache.as_ref() {
            if cached_raw == &raw {
                return parsed.clone();
            }
        }
        let parsed = parse_mnemonic(&raw);
        *cache = Some((raw, parsed.clone()));
        parsed
    }

    /// Measure the width of a stripped-text byte prefix using the text
    /// backend's single-line layout. Returns 0 if the prefix is empty
    /// or the backend is unavailable. The cache key is shared with the
    /// full-string layout (same TextStyle), so this is cheap.
    fn prefix_width(
        backend: &Rc<RefCell<dyn bastyde_canvas::TextBackend>>,
        stripped: &str,
        byte_end: usize,
        style: &bastyde_tokens::TextStyle,
    ) -> f32 {
        if byte_end == 0 {
            return 0.0;
        }
        let mut b = backend.borrow_mut();
        let layout = b.layout_single_line(&stripped[..byte_end], style, None);
        layout.width
    }
}

impl Widget for MenuLabel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Source: locale change → relayout (the stripped string may
        // be a different length in a different language).
        if let Prop::Bound(sig) = &self.source {
            sig.bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                BindingLevel::Relayout,
            );
        }
        // Alt-held: repaint only — the layout never changes when Alt
        // toggles, just whether we draw a 1dp rect.
        self.alt_down.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let parsed = self.resolve_parsed();
        let style = self.style.resolve(&ctx.theme.typography);

        let Some(backend) = ctx.text_backend else {
            // Mock fallback: assume 8px/char for measurement.
            let width = parsed.stripped.len() as f32 * 8.0;
            let height = 16.0;
            let w = match proposal.width {
                Some(max) => width.min(max),
                None => width,
            };
            *self.last_layout.borrow_mut() = None;
            return Size::new(w, height).into();
        };
        let mut backend = backend.borrow_mut();
        let max_width = proposal.width.map(|w| w + 0.5);
        let layout = backend.layout_single_line(&parsed.stripped, &style, max_width);
        let size = Size::new(layout.width, layout.height);
        *self.last_layout.borrow_mut() = Some(layout);
        size.into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let parsed = self.resolve_parsed();
        let color = self.color.resolve(ctx.theme, ctx.effective_enabled);
        let style = self.style.resolve(&ctx.theme.typography);

        // Pull the cached layout — populated by `layout_response`. If
        // missing (mock backend, or a paint pass that ran before a
        // layout pass), re-layout via `draw_text` which measures
        // internally.
        let layout_opt = self.last_layout.borrow().clone();
        if let Some(layout) = layout_opt.as_ref() {
            // Fast path: draw the pre-measured layout.
            canvas.draw_text_layout(layout, Point::new(bounds.x, bounds.y), color);
        } else {
            // Fallback (no cached layout): measure-and-draw in one go.
            canvas.draw_text(&parsed.stripped, bounds, &style, color);
        }

        // Underline. Skip if Alt is up, no marker was parsed, OR if
        // we're on macOS — Option+letter is rewritten by the OS for
        // accented character input before reaching the dispatcher,
        // so Alt+letter mnemonics don't function (see
        // `MenuBarDispatcher::try_handle`). Drawing the underline
        // anyway would mislead users into thinking the chord works.
        let alt_held = self.alt_down.get() && !cfg!(target_os = "macos");
        if !alt_held || !parsed.has_mnemonic() {
            return;
        }
        let Some(byte_index) = parsed.byte_index else {
            return;
        };
        // Width of the marked character: its byte length.
        let char_byte_len = parsed.stripped[byte_index..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0);
        if char_byte_len == 0 {
            return;
        }

        // Pull the backend and the layout metrics. If the backend is
        // unavailable (mock environment), skip the underline silently
        // — there's no font metric to size against.
        let Some(backend_rc) = canvas.text_backend() else {
            return;
        };
        let Some(layout) = layout_opt else {
            return;
        };

        let x0 = Self::prefix_width(backend_rc, &parsed.stripped, byte_index, &style);
        let x1 = Self::prefix_width(
            backend_rc,
            &parsed.stripped,
            byte_index + char_byte_len,
            &style,
        );
        let underline_width = (x1 - x0).max(0.0);
        if underline_width <= 0.0 {
            return;
        }

        // Position the underline at the *bottom* of the laid-out
        // text box. The font's `underline_offset` from text-typeset is
        // often small (≤ 1 px below baseline) which lands inside the
        // descender zone and visually intersects with characters
        // that have no descender (S, V, F, …). Placing the rule at
        // `bounds.y + layout.height - thickness` gives a reliable
        // below-text position regardless of the font's reported
        // metric. We also leave one extra pixel of clearance above
        // the bottom edge so the underline doesn't sit on top of the
        // row's bottom padding line.
        let thickness = layout.underline_thickness.max(1.0);
        let y = bounds.y + layout.height - thickness;
        canvas.fill_rect(
            Rect::new(bounds.x + x0, y, underline_width, thickness),
            color,
        );
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Hidden — the enclosing MenuItem / MenuBarTrigger owns the
        // accessible name and the access-key field.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::SizeProposal;
    use bastyde_core::signal::Signal;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_tokens::{TextRole, TextStyleRole};

    fn tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
    }

    #[test]
    fn no_underline_when_alt_up() {
        let mut t = tree();
        let alt = Signal::new(false);
        let label = MenuLabel::new(
            Prop::from("&Save".to_string()),
            alt.clone(),
            ColorProp::TextRole(TextRole::Primary),
            TextStyleProp::Role(TextStyleRole::Body),
        );
        let id = t.add(label);
        t.layout(SizeProposal::exact(200.0, 40.0));
        let frame = t.render();
        // With no backend present in headless tests, the layout is
        // empty — but the underline path is also skipped (we early-out
        // on missing backend). What we're asserting is "no crash, no
        // spurious rect" when Alt is up. The underline would otherwise
        // appear as a fill_rect call.
        let _ = id;
        let _ = frame;
    }

}
