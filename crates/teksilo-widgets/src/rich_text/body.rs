// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for the private `RichTextEditorBody`
//! leaf — layout (intrinsic / greedy), viewport recording, glyph / caret /
//! selection paint and the paragraph + text-run accessibility walk — together
//! with the pre-layout content-height estimate only it uses.

use super::*;

mod flow_walk;

use self::flow_walk::FlowWalk;
/// How tall this text is likely to be, before anything has laid it out.
///
/// See the call site in [`RichTextEditorBody::layout_response`] for why a guess
/// beats the zero it replaces. Two O(1) document reads and some arithmetic; no
/// shaping, no glyph cache, nothing that could be slow enough to matter in a
/// layout pass.
///
/// **The typography is the half that decides whether this is useful.** A first cut
/// counted bare lines at the font's natural height and came out well under the
/// truth for manuscript prose, which is set with a line-height multiplier and space
/// between paragraphs — the estimate was missing a third of the page and the rows it
/// sized still visibly grew when they finally laid out. So:
///
/// * lines are counted at the font's own advance, since that is what decides how
///   many characters fit on one, and
/// * each line is then given the **multiplied** height, and each block the space
///   above and below it that a body paragraph gets.
///
/// The mean advance is taken as a measured fraction of the font's line height —
/// [`MEAN_ADVANCE_OVER_LINE_HEIGHT`] below, which records how it was arrived at.
/// That is roughly right for proportional Latin text at ordinary sizes and roughly
/// wrong for everything else, which is acceptable for a number whose only
/// competition is a constant and whose lifetime is one frame.
/// Mean glyph advance as a fraction of the font's natural line height.
///
/// **Measured, not derived.** Thirty-two real manuscript scenes were laid out in a
/// running window and compared against what this function claimed for each, at a
/// 447 px measure in Literata at 1.6 line height:
///
/// | scene | guess | real | ratio |
/// |---|---|---|---|
/// | 23 443 chars | 19 586 | 17 768 | 1.10 |
/// | 20 798 chars | 17 028 | 15 578 | 1.09 |
/// | 16 493 chars | 13 860 | 12 827 | 1.08 |
///
/// The bias was 1.06–1.12 across a 1.4× range of scene sizes: a scale error, not
/// noise, and 0.37 solved back to 0.335 on every one of them. Two earlier values
/// were reasoned about rather than measured — 0.5 from "half the font size", then
/// 0.37 from dividing that by a nominal line height — and both were wrong by more
/// than this whole correction.
///
/// It is a *typical* value, and it is font-dependent: a wider or narrower face moves
/// it, which is why the accuracy test allows ±25% rather than pretending otherwise.
/// If that stops being good enough, the answer is to learn it from the first real
/// layout the process performs rather than to tune the constant again.
const MEAN_ADVANCE_OVER_LINE_HEIGHT: f32 = 0.335;

fn estimated_content_height(
    document: &teksilo_text::text_document::TextDocument,
    width: f32,
    font_line_h: f32,
    typography: &teksilo_text::EditorTypographyDefaults,
) -> f32 {
    if width <= 0.0 || font_line_h <= 0.0 {
        return 0.0;
    }
    // Characters per line from the **font's** line height: the multiplier below
    // spaces lines further apart, it does not make the glyphs wider.
    let per_line = (width / (font_line_h * MEAN_ADVANCE_OVER_LINE_HEIGHT)).max(1.0);
    let chars = document.character_count() as f32;
    let blocks = document.block_count().max(1) as f32;
    // Wrapped lines, plus **half** a line per block for the ragged last one of each.
    // Half rather than one: a block takes `ceil(chars / per_line)` lines, which
    // averages half a line more than the division, and charging a whole one over-
    // counted a scene of many short paragraphs by more than the wrapping itself.
    // Floored at one line per block, because an empty paragraph still takes a line.
    let lines = (chars / per_line + blocks * 0.5).max(blocks);
    let line_h = font_line_h * typography.line_height.max(0.1);
    let per_block = typography.paragraph_spacing_before + typography.paragraph_spacing_after;
    let h = lines * line_h + blocks * per_block.max(0.0);
    #[cfg(feature = "debug-traces")]
    if height_debug() {
        eprintln!(
            "HEIGHT-EST chars={chars:.0} blocks={blocks:.0} width={width:.1} \
             font_lh={font_line_h:.2} mult={:.2} per_line={per_line:.1} -> {h:.1}",
            typography.line_height
        );
    }
    h
}

/// Whether to print what the height guess and the real layout each came up with.
///
/// `TEKSILO_HEIGHT_DEBUG=1`, and only in a build with the `debug-traces` feature.
/// Read once — this sits in a layout pass, and an environment lookup per
/// measurement would be a real cost for a diagnostic that is off for everyone.
///
/// It earns its place: the guess above was wrong three separate ways before anyone
/// could see it, and the one that mattered most — being asked to measure at 76 px
/// when the text wraps at 447 — was invisible to every test and obvious in one line
/// of this output.
///
/// Behind a feature as well as a variable, and the traces are `#[cfg]` out rather
/// than merely switched off: a runtime `false` still leaves every format string in
/// the binary, which is measurable. A diagnostic nobody can switch on has no
/// business being reachable in a release, and an environment variable is reachable
/// by anyone.
#[cfg(feature = "debug-traces")]
fn height_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("TEKSILO_HEIGHT_DEBUG").is_some())
}

impl Widget for RichTextEditorBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Bind `caret_visible` to the framework's repaint tracker so
        // that every toggle in the frame-tick effect marks **this
        // body** widget `needs_paint` — the caret is painted in
        // `RichTextEditorBody::paint`. Skipped for `CaretPolicy::Hidden`.
        {
            let st = self.state.borrow();
            let caret_policy = st.policy.caret_policy;
            let caret_visible = st.caret_visible.clone();
            drop(st);
            if caret_policy != CaretPolicy::Hidden {
                let self_id = ctx.self_id();
                caret_visible.bind_to(
                    self_id,
                    ctx.binding_registry(),
                    teksilo_core::binding::BindingLevel::RepaintOnly,
                );
            }
        }

        // Bind document_version at `BindingLevel::AccessibilityOnly` so
        // text / format edits flip the tree's `a11y_dirty` flag through
        // **this body** — its `accessibility()` is the one that emits
        // the editor's Role::MultilineTextInput / Role::Document and
        // walks the flow snapshot.
        //
        // ALSO bind at `RepaintOnly` so the widget's needs_paint flips
        // on every text / format change. Without this, paint() only
        // ran on caret-blink (the only other RepaintOnly binding), and
        // the post-fix dispatch's `last_relayout_block_id.take()` was
        // consumed on the wrong tick — leaving text edits invisible
        // until a resize forced a full re-layout.
        {
            let st = self.state.borrow();
            let document_version = st.document_version.clone();
            drop(st);
            let self_id = ctx.self_id();
            document_version.bind_to(
                self_id,
                ctx.binding_registry(),
                teksilo_core::binding::BindingLevel::AccessibilityOnly,
            );
            document_version.bind_to(
                self_id,
                ctx.binding_registry(),
                teksilo_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // Bind `scroll_y`, `scroll_x`, `cursor_position`, `cursor_anchor`,
        // and `has_selection` at RepaintOnly so the widget marks
        // needs_paint immediately on scroll, cursor move, and selection
        // change. Without these, paint() only ran on caret-blink and
        // text-version bumps — so scroll/selection changes appeared
        // delayed by up to 500ms (in sync with the next caret toggle).
        //
        // The cursor_only render path inside text-typeset falls back to
        // a full render automatically when scroll drifted since
        // the last full render, so this binding is correctness-safe.
        {
            let st = self.state.borrow();
            let scroll_y = st.scroll_y.clone();
            let scroll_x = st.scroll_x.clone();
            let cursor_position = st.cursor_position.clone();
            let cursor_anchor = st.cursor_anchor.clone();
            let has_selection = st.has_selection.clone();
            drop(st);
            let self_id = ctx.self_id();
            for signal in [&scroll_y, &scroll_x] {
                signal.bind_to(
                    self_id,
                    ctx.binding_registry(),
                    teksilo_core::binding::BindingLevel::RepaintOnly,
                );
            }
            // Caret and anchor are repaint-only for geometry, but they ALSO
            // change what the a11y walk reports via `set_text_selection_to`. A
            // caret-only move (arrow key, click, drag-select) emits no document
            // event, so `document_version` never bumps; without an
            // `AccessibilityOnly` binding here `a11y_dirty` never flips and a
            // screen reader hears the caret frozen at the last edit. Bind both
            // levels — the two-level pattern `document_version` uses. Selecting
            // moves the caret and/or anchor, so `has_selection` (derived from
            // them) needs no separate a11y binding.
            for signal in [&cursor_position, &cursor_anchor] {
                signal.bind_to(
                    self_id,
                    ctx.binding_registry(),
                    teksilo_core::binding::BindingLevel::RepaintOnly,
                );
                signal.bind_to(
                    self_id,
                    ctx.binding_registry(),
                    teksilo_core::binding::BindingLevel::AccessibilityOnly,
                );
            }
            has_selection.bind_to(
                self_id,
                ctx.binding_registry(),
                teksilo_core::binding::BindingLevel::RepaintOnly,
            );
        }

        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let w = proposal.width.unwrap_or(200.0).max(0.0);

        // Greedy mode (default, behaviour unchanged): both knobs
        // unset → consume the proposal exactly as before.
        if self.min_lines.is_none() && self.max_lines.is_none() {
            let h = proposal.height.unwrap_or(100.0).max(0.0);
            return (Size::new(w, h)).into();
        }

        // Intrinsic mode: clamp content height to `[min_h, max_h]`
        // where each bound is `n * line_height`. The clamp is a
        // hard cap — we ignore the proposal's height and let the
        // vertical scroll bar take over past `max_lines`.
        // Remember the widest measure anything has asked for, before reading the
        // state below — see where it is used for why the widest and not this pass's.
        {
            let mut st = self.state.borrow_mut();
            if let Some(w) = proposal.width
                && w > st.widest_measured_width
            {
                st.widest_measured_width = w;
            }
        }
        let st = self.state.borrow();
        // `default_line_height()` is the *unscaled* line height (its standalone
        // shaper path uses font_scale = 1.0), but `content_height()` carries the
        // engine's font_scale. Scale the per-line bound to match, or a
        // text-scaled editor would clip at `max_lines` / under-size at
        // `min_lines`.
        let line_scale = st.effective_font_scale(ctx.text_scale);
        let line_h = st.engine.default_line_height() * line_scale;
        // **An estimate rather than a zero before the text has been laid out.**
        //
        // `content_height()` is `0` until `layout_full` has run, and that does not
        // happen until the editor has been through a frame on screen. A zero then
        // falls through to the `min_lines` floor below, so *every* unlaid-out editor
        // claims the same ten lines whatever it holds — a three-thousand-word scene
        // and an empty one measure identically.
        //
        // On a single editor that is invisible: it is on screen, so it lays out.
        // Down a **stream** it is not. A Full Book is a column of editors, most of
        // them below the fold, and the page's height is the sum of their claims —
        // so the scroll extent is wrong by an order of magnitude and settles, a row
        // at a time, as the writer reads. Anything drawing that extent draws the
        // settling: a margin lane gives each row a slice to match the claim, then
        // watches it grow tenfold the moment the row is reached.
        //
        // The estimate is deliberately crude — a measured mean advance of about a
        // third of the line height, half an extra line per block for the ragged last
        // line of each — and being crude is the point. It is thrown away the instant
        // a real layout exists, so its only job is to be closer than a constant,
        // which is not a demanding standard. It is **not** a floor: an over-estimate
        // corrects downwards when the layout lands, where a too-large `min_lines`
        // would leave blank space under short text for the life of the widget. It is thrown away the instant a real layout
        // exists, so its only job is to be closer than a constant, which is not a
        // demanding standard. It is **not** a floor: an over-estimate corrects
        // downwards when the layout lands, where a too-large `min_lines` would
        // leave blank space under short text for the life of the widget.
        //
        // ⚠ **Only when the width is actually known.** `w` above falls back to 200
        // for a proposal that carries none, which is fine for a width but ruinous
        // for a line count: `CenterColumnFlowing` measures its child width-only, and
        // estimating against the fallback made a scene wrap at a quarter of its real
        // ⚠ **Only when the width is actually known.** `w` above falls back to 200
        // for a proposal that carries none, which is fine for a width but ruinous
        // for a line count: `CenterColumnFlowing` measures its child width-only, and
        // estimating against the fallback made a scene wrap at a quarter of its real
        // measure and claim nearly twice its real height. So the estimate never reads
        // the proposal's width: it takes the viewport, or failing that the widest
        // measure anything has asked for, and falls back to the floor only when
        // neither is known yet.
        let content_h = match (st.engine.has_full_layout(), proposal.width) {
            (true, _) => st.engine.content_height(),
            // **At the width the text will actually wrap at**, which is the viewport
            // the body was last *placed* at — not the width of whichever measurement
            // pass happens to be asking.
            //
            // Measured in a real window those are not the same number, and the
            // difference is not small: a stream row was asked to measure at 76 px
            // during an early pass, estimated eight characters to a line and so six
            // times its true height, while the layout that followed wrapped it at
            // 447. A guess taken at the wrong measure is worse than no guess — it is
            // the same jump it was meant to remove, pointing the other way.
            //
            // Zero before the body has ever been placed, and then the proposal is the
            // only thing on offer; after the first placement the viewport is the
            // truth. `w` above is deliberately not reused: its 200 px fallback is a
            // sane default for a width and a ruinous one for a line count.
            (false, _) if st.estimate_height_before_layout => {
                // The viewport if this body has been placed, else the **widest**
                // width anything has asked it to measure at.
                //
                // Not the width of the pass that happens to be asking: a real window
                // proposes 76 px to a stream row whose text wraps at 447, and
                // guessing against that claimed six times the true height. Nor the
                // viewport alone, which was tried and is worse — a stream's rows are
                // rebuilt often enough that it is almost always still zero, so the
                // guess simply never ran and every row fell back to the floor it was
                // meant to replace.
                let width = st.viewport_width.max(st.widest_measured_width);
                if width > 0.0 {
                    estimated_content_height(
                        &st.document,
                        width,
                        line_h,
                        st.engine.typography_defaults(),
                    )
                } else {
                    0.0
                }
            }
            (false, _) => 0.0,
        };
        drop(st);

        let min_h = self.min_lines.map(|n| n as f32 * line_h).unwrap_or(0.0);
        let max_h = self
            .max_lines
            .map(|n| n as f32 * line_h)
            .unwrap_or(f32::INFINITY);
        let intrinsic_h = content_h.clamp(min_h, max_h);
        Size::new(w, intrinsic_h.max(0.0)).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // The body is a leaf, but the layout walker hands every widget its final
        // bounds here — and layout runs before paint, so this is the earliest
        // (hence authoritative) point at which the viewport can be adopted.
        // `sync_viewport` owns the whole handoff, including `engine.set_viewport`
        // and the relayout flag; paint calls it again as an idempotent echo. See
        // its docs for why the writes must not be split.
        self.state.borrow_mut().sync_viewport(bounds);
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let mut st = self.state.borrow_mut();

        // Sync the engine's default text color with the active theme
        // so dark / light mode swaps reach the rendered glyphs. The
        // engine reads `text_color` fresh on every `render()` and
        // does not bake it into a glyph cache, so a per-paint write
        // is cheap. A color the app pinned via
        // `RichTextEditor::text_color(...)` is written the same way,
        // resolved against the theme each paint rather than skipping it.
        //
        // The render frame DOES cache colors baked into glyph quads,
        // though — the cursor-only and block-only render paths reuse
        // those cached quads. So when the theme colour actually
        // changes, we must dispatch a full render this frame, or the
        // visible glyphs keep painting in the old colour until the
        // next typing / scroll event happens to bump up to a Full
        // path on its own.
        // An app-set `text_color` (Color / role / Signal) is resolved against
        // the active theme each paint; otherwise track the theme's `editor_fg`.
        {
            let new_color = match &st.text_color_prop {
                Some(prop) => prop.resolve(ctx.theme, true).to_array(),
                None => ctx.theme.colors.editor_fg.to_array(),
            };
            st.engine.set_text_color(new_color);
            if st.last_text_color != Some(new_color) {
                st.last_text_color = Some(new_color);
                st.pending_full_render = true;
            }
        }

        // Caret colour: app override resolved each paint, else the theme's
        // `editor_caret` role. The engine defaults the cursor to opaque black,
        // so without this the blinking caret stays black under a dark theme.
        // Cursor decorations are regenerated on every render (the cursor-only
        // path included), so a colour change only needs a render this frame —
        // force one so a swap doesn't wait for the next blink toggle.
        {
            let new_caret = match &st.caret_color_prop {
                Some(prop) => prop.resolve(ctx.theme, true).to_array(),
                None => ctx.theme.colors.editor_caret.to_array(),
            };
            st.engine.set_cursor_color(new_caret);
            if st.last_cursor_color != Some(new_caret) {
                st.last_cursor_color = Some(new_caret);
                st.pending_full_render = true;
            }
        }

        // Selection highlight. A custom colour (set via `.selection_color`) is
        // used as-is and is NOT auto-desaturated when the window goes inactive
        // — matching macOS, where an explicit selection colour opts out of
        // system management. Otherwise the theme drives it, window-aware: the
        // vivid `editor_selection_bg` while the window is active, the muted
        // `selection_bg_inactive` while it is not. Resolved each paint and
        // cached, so a change (theme, custom colour, or window-active flip)
        // just needs a render this frame.
        let new_sel = if let Some(prop) = st.selection_color_prop.as_ref() {
            prop.resolve(ctx.theme, true).to_array()
        } else if ctx.window_active {
            ctx.theme.colors.editor_selection_bg.to_array()
        } else {
            ctx.theme.colors.selection_bg_inactive.to_array()
        };
        if st.last_selection_color != Some(new_sel) {
            st.engine.set_selection_color(new_sel);
            st.last_selection_color = Some(new_sel);
            st.pending_full_render = true;
        }

        // Code block surface colours come from the same theme path
        // (`editor_code_block_bg` / `editor_code_block_fg`). Unlike
        // `text_color`, these are baked into the converted
        // `BlockLayoutParams` at `layout_full` / `relayout_block`
        // time, so the typesetter does NOT pick them up on a render
        // pass — we need a full re-layout when they change. Setting
        // `needs_full_layout = true` schedules that for the same
        // frame; `pending_full_render` covers the render side.
        let new_code_bg = ctx.theme.colors.editor_code_block_bg.to_array();
        let new_code_fg = Some(ctx.theme.colors.editor_code_block_fg.to_array());
        st.engine.set_code_block_background(new_code_bg);
        st.engine.set_code_block_foreground(new_code_fg);
        if st.last_code_block_bg != Some(new_code_bg) || st.last_code_block_fg != new_code_fg {
            st.last_code_block_bg = Some(new_code_bg);
            st.last_code_block_fg = new_code_fg;
            st.needs_full_layout = true;
            st.pending_full_render = true;
        }

        // Link colour rides the same path, and for the same reason: it is
        // baked into the shaped runs at layout time, so a theme swap needs a
        // full re-layout rather than a repaint. Sharing `TextRole::Link` with
        // every other link in the app is the point — a hyperlink in prose and
        // one in a panel should not be two different blues.
        let new_link_fg = Some(ctx.theme.colors.text_link.to_array());
        st.engine.set_link_foreground(new_link_fg);
        if st.last_link_fg != new_link_fg {
            st.last_link_fg = new_link_fg;
            st.needs_full_layout = true;
            st.pending_full_render = true;
        }

        // The engine reads the HiDPI display scale factor from the
        // shared `TypesetterBridge` on every `layout_full`, exactly
        // like `TextWidget` does internally. No widget-side plumbing
        // — this is a render-pipeline concern, invisible to the
        // widget author.

        // Logical font scale: a11y text scale (if followed) × per-editor
        // `font_size_scale`. Baked at `layout_full`, so a change forces a
        // relayout + render this frame.
        {
            let target = st.effective_font_scale(ctx.text_scale);
            if st.last_font_scale.is_nan() || (st.last_font_scale - target).abs() > f32::EPSILON {
                st.last_font_scale = target;
                st.engine.set_font_scale(target);
                st.needs_full_layout = true;
                st.pending_full_render = true;
            }
        }

        // Idempotent echo — `place_children` already adopted these exact bounds
        // during layout, so this is normally a no-op. It stays so that any path
        // which paints without a preceding layout still sizes the engine.
        st.sync_viewport(bounds);

        // First-frame guard + viewport-change guard: (re)run the
        // full layout so the render call produces glyphs sized
        // for the current bounds. With per-widget `DocumentFlow`
        // state inside the engine, `has_full_layout()` only
        // reports `false` when this widget has never laid out
        // or when the shared service's HiDPI scale factor has
        // changed since the last layout — there is no
        // cross-widget trampling left to guard against.
        //
        // `did_full_layout` is true on this paint iff we just ran
        // `layout_full` above — which means the render frame must
        // be rebuilt from scratch via `with_render_frame`. The
        // incremental render paths (`with_render_block_only`,
        // `with_render_cursor_only`) assume a valid prior full
        // render exists.
        let did_full_layout = st.needs_full_layout || !st.engine.has_full_layout();
        if did_full_layout {
            let flow = st.flow_snapshot();
            st.engine.layout_full(&flow);
            st.needs_full_layout = false;
            st.content_dirty = true;
            #[cfg(feature = "debug-traces")]
            if height_debug() {
                eprintln!(
                    "HEIGHT-REAL chars={} blocks={} width={:.1} -> {:.1}",
                    st.document.character_count(),
                    st.document.block_count(),
                    st.engine.layout_width(),
                    st.engine.content_height()
                );
            }
        }

        // The viewport got smaller since the last frame — a window resize, a
        // pane opening, the on-screen keyboard rising under a focused editor —
        // and the caret may now be outside it. `sync_viewport` recorded the
        // shrink (it is the only place that sees both sizes); here, with the
        // relayout it forced already run, is the earliest point the reveal can
        // be computed against real geometry.
        if std::mem::take(&mut st.pending_caret_reveal) {
            crate::rich_text::keyboard::ensure_caret_visible_locked(&mut st);
        }

        // Update the cursor display every paint so selection
        // highlights follow the caret without needing a frame tick.
        // The caret is suppressed in an inactive window for every policy — the
        // authoritative final gate, covering the one frame between a
        // window-active flip and the build-time effect running.
        let caret_on_now = if st.drop_caret && st.policy.caret_policy != CaretPolicy::Hidden {
            // A drag is overhead: show where it would land. Focus is still
            // wherever the drag started — often another editor entirely — so
            // the focus gate below would hide precisely the caret the writer
            // is aiming with. Steady, not blinking, and never in a read-only
            // editor (`Hidden`), which takes no drop anyway.
            st.window_active
        } else {
            match st.policy.caret_policy {
                CaretPolicy::Hidden => false,
                CaretPolicy::StaticVisible => st.has_focus && st.window_active,
                CaretPolicy::Blinking => st.caret_visible.get() && st.has_focus && st.window_active,
            }
        };
        let cursor_display = teksilo_text::CursorDisplay {
            position: st.cursor.position(),
            anchor: st.cursor.anchor(),
            affinity: st.cursor_affinity,
            visible: caret_on_now,
            selected_cells: Vec::new(),
        };
        st.engine.set_cursor(&cursor_display);

        // Forward the widget's scroll state to the typesetter so
        // viewport culling knows where the visible window is. text-
        // typeset's `render()` only emits glyphs whose flow Y falls
        // inside `[scroll_offset, scroll_offset + viewport_height]`,
        // and the emitted screen coordinates already have
        // `scroll_offset` subtracted — so the paint walker doesn't
        // apply any further offset beyond the widget origin.
        let scroll_y_logical = st.scroll_y.get();
        st.engine.set_scroll_offset(scroll_y_logical);

        // Window the render to the visible clip when opted in (dubious mode).
        // The editor is laid out at its full document height inside an outer
        // ScrollArea, so its own viewport spans the whole document and the
        // viewport-derived cull keeps everything. `ctx.clip_bounds` is the
        // accumulated ancestor clip — the intersection of every clipping
        // ancestor, so this is correct under nested ScrollAreas — mapped into
        // the editor's content space to the band actually on screen. A
        // half-viewport margin each side pre-renders content just off-screen so
        // scrolling never flashes a blank edge. Positioning and hit-testing are
        // untouched: `set_render_window` overrides culling only, and
        // `scroll_offset` stays as set above.
        let render_window = if st.window_to_clip {
            ctx.clip_bounds.map(|clip| {
                // `clip` and `bounds` are screen-space; the render cull works in
                // content space. The visible band's top is the editor's own scroll
                // offset plus however far its top sits above the clip: in dubious
                // mode `scroll_offset` is pinned to 0, but including it keeps the
                // window correct (rather than mis-culling) even for a self-scrolling
                // editor, so this can't silently render the wrong rows.
                let vis_top = (scroll_y_logical + (clip.y - bounds.y)).max(0.0);
                let vis_h = clip.height.max(0.0);
                let margin = vis_h * 0.5;
                ((vis_top - margin).max(0.0), vis_h + 2.0 * margin)
            })
        } else {
            None
        };
        st.engine.set_render_window(render_window);

        // Captured before the split-borrow below (which holds `st` mutably
        // for the rest of the method) so the preedit underline pass can
        // still see them. `cursor_affinity` matches what `caret_rect`
        // queries elsewhere.
        let scroll_x_logical = st.scroll_x.get();
        let ime_preedit_range = st.ime_preedit_range.clone();
        let ime_affinity = st.cursor_affinity;

        // Clip to bounds so overflowing glyphs don't bleed into siblings.
        canvas.set_clip(bounds);

        // Choose the cheapest render path that produces a correct
        // frame for this paint:
        // - Full render: we just rebuilt the layout (no prior frame
        //   to incrementally update), so emit everything from scratch.
        // - Block-only: the frame_loop relayed out exactly one block
        //   since the last paint (single-block edit). Reuse cached
        //   glyphs for the other N-1 blocks.
        // - Cursor-only: nothing structural changed since last
        //   paint — only the cursor blink or selection updated.
        //   Reuses every cached glyph and just refreshes cursor /
        //   selection decorations. Falls back to full render
        //   internally if scroll drifted.
        //
        // Pre-fix, paint() unconditionally called `with_render_frame`,
        // which walked every block on every paint — visible as a
        // ~17% chunk in `rasterize_glyph` / `render_run_glyphs` on
        // the flamegraph because caret blinks and signal updates
        // were forcing a full re-render at ~60 Hz.
        let block_relayout = st.last_relayout_block_id.take();
        let pending_full = std::mem::replace(&mut st.pending_full_render, false);
        enum RenderChoice {
            Full,
            Block(usize),
            CursorOnly,
        }
        // `pending_full` covers the case where `frame_loop::tick`
        // already ran `layout_full` this frame (e.g. on FormatChanged
        // or FlowElementsInserted events from a list-indent edit or
        // Enter key) but cleared `needs_full_layout` before paint ran.
        // Without it, paint would fall through to CursorOnly and the
        // new layout wouldn't render until something else forced a
        // Full pass (resize, scroll out and back into view).
        let choice = if did_full_layout || pending_full {
            RenderChoice::Full
        } else if let Some(bid) = block_relayout {
            RenderChoice::Block(bid)
        } else {
            RenderChoice::CursorOnly
        };

        // Split-borrow the state fields so the paint walker can hold
        // `&engine.with_render_frame(...)`, `&document`, and
        // `&mut image_cache` simultaneously.
        let state_ref: &mut EditorState = &mut st;
        // Read before the split borrow below, which reborrows `state_ref`
        // field by field.
        let selection_range = {
            let (s, e) = (
                state_ref.cursor.selection_start(),
                state_ref.cursor.selection_end(),
            );
            (s != e).then_some((s, e))
        };
        let EditorState {
            ref mut engine,
            ref document,
            ref mut image_cache,
            ref image_resolver,
            ref selected_image,
            ref resize_preview,
            ..
        } = *state_ref;
        let image_resolver = image_resolver.as_ref();
        let resize_preview_rect = resize_preview.get();
        let paint_closure = |frame: &teksilo_text::RenderFrame| {
            paint_frame(
                canvas,
                PaintParams {
                    frame,
                    origin: Point::new(bounds.x, bounds.y),
                    document,
                    image_cache,
                    image_resolver,
                    selection: selection_range,
                    // The same colour the typesetter drew underneath, resolved
                    // above for `engine.set_selection_color`.
                    selection_color: new_sel,
                    // The paint pass is the one place that has both the image
                    // rects and the selection, so it is what tells the pointer
                    // handler where the grips are.
                    selected_image_out: Some(selected_image),
                    resize_preview: resize_preview_rect,
                    draw_caret: caret_on_now,
                },
            );
        };
        match choice {
            RenderChoice::Full => engine.with_render_frame(paint_closure),
            RenderChoice::Block(bid) => engine.with_render_block_only(bid, paint_closure),
            RenderChoice::CursorOnly => engine.with_render_cursor_only(paint_closure),
        };

        // IME preedit underline. Walk the composing range char-by-char,
        // emitting one underline segment per visual line so a wrapped
        // composition underlines correctly. Engine coords are content-
        // space; screen = bounds + content − scroll (matches the glyphs).
        // On a read-only viewer there is never a preedit, so this is inert.
        if let Some(range) = ime_preedit_range
            && engine.has_full_layout()
            && range.start < range.end
        {
            let color = ctx.theme.colors.text_primary;
            let underline = |canvas: &mut Canvas, x0: f32, x1: f32, y: f32, h: f32| {
                let uy = y + h - 1.0;
                canvas.draw_line(
                    Point::new(x0, uy),
                    Point::new(x1, uy),
                    color,
                    teksilo_canvas::StrokeStyle::solid(1.0),
                );
            };
            let mut seg_x0: Option<f32> = None;
            let (mut seg_y, mut seg_h, mut last_x) = (0.0_f32, 0.0_f32, 0.0_f32);
            for p in range.start..=range.end {
                let c = engine.caret_rect(p, ime_affinity);
                let x = bounds.x + c[0] - scroll_x_logical;
                let y = bounds.y + c[1] - scroll_y_logical;
                match seg_x0 {
                    None => {
                        seg_x0 = Some(x);
                        seg_y = y;
                        seg_h = c[3];
                        last_x = x;
                    }
                    Some(x0) => {
                        if (y - seg_y).abs() > 0.5 {
                            underline(canvas, x0, last_x, seg_y, seg_h);
                            seg_x0 = Some(x);
                            seg_y = y;
                            seg_h = c[3];
                        }
                        last_x = x;
                    }
                }
            }
            if let Some(x0) = seg_x0 {
                underline(canvas, x0, last_x, seg_y, seg_h);
            }
        }

        canvas.clear_clip();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use self::policy::AccessibilityRole;
        use teksilo_core::accesskit::{Action, Role};

        let st = self.state.borrow();

        let role = match st.policy.access_role {
            AccessibilityRole::Editor => Role::MultilineTextInput,
            AccessibilityRole::Document => Role::Document,
        };
        builder.set_role(role);
        if st.policy.is_read_only() {
            builder.set_read_only();
        }

        // Walk the cached flow snapshot (or rebuild it if the last edit
        // cleared the cache) and hand it to `flow_walk`, which owns the tree
        // shape: an ordinary block's runs are **direct** children of this
        // node, and the containers that do get a node of their own (a table,
        // a blockquote, a heading) carry a text-range-capable node between
        // themselves and their runs. `flow_walk`'s module header has the
        // rule and the reason; the short version is that a plain
        // `Role::Paragraph` wrapper would be an object-navigation stop
        // nobody asked for, and any wrapper that does not support text
        // ranges makes macOS, Windows and AT-SPI drop every text-change
        // event for the runs beneath it.
        let snap = {
            let mut cache = st.accessibility_flow_snapshot.borrow_mut();
            if cache.is_none() {
                // A bare view (show_highlights=false) builds its AT tree from a
                // clean snapshot too, so screen readers never hear highlight-
                // driven formatting that no sighted user sees. The paint-only
                // overlay is skipped: the AT walk reads fragments, never the
                // overlay, so computing a paint span per spell/find range here
                // would be pure waste (it dominated the a11y rebuild on a large
                // spell-checked document).
                *cache = Some(st.flow_snapshot_for_a11y());
            }
            cache.as_ref().cloned()
        };

        // While composing (IME preedit active), expose the composition as
        // the AT selection so screen readers / braille track the tentative
        // text — the composing characters are already in the runs / value.
        // Falls back to the live cursor/selection otherwise.
        let (user_anchor, user_pos) = match st.ime_preedit_range.clone() {
            Some(range) => (range.start, range.end),
            None => (st.cursor.anchor(), st.cursor.position()),
        };
        let emits_children = !builder.emits_no_children();

        // A name probe (and an owner-less builder) can emit no children, and
        // `push_text_runs` writes nothing to the builder in that mode — so
        // the walk would shape every line of the document, resolve every
        // block's geometry and produce an empty emission whose `runs` no
        // caret can be resolved against. Skip it: on a document with a large
        // table that is thousands of layout queries for a tree nobody reads.
        let mut walk = FlowWalk::new(&st, emits_children, user_anchor, user_pos);
        if let (Some(snap), true) = (snap, emits_children) {
            walk.elements(builder, &snap.elements, None, None);
        }
        let caret_pair = walk.caret;
        let anchor_pair = walk.anchor;
        let syn_map = walk.syn_map;

        // Attach the text selection on the editor itself, referencing the
        // TextRun children both endpoints landed in.
        if let (Some(a), Some(c)) = (anchor_pair, caret_pair) {
            builder.set_text_selection_to(a, c);
        }

        // Only a real walk owns the map. A name probe emits no children (see
        // `AccessNodeBuilder::emits_no_children`), so overwriting here would
        // wipe the mapping the last real walk left behind and break every
        // screen-reader-initiated caret move until the next one.
        if emits_children {
            *st.synthetic_to_element.borrow_mut() = syn_map;
        }

        builder.add_action(Action::Focus);
        builder.add_action(Action::ScrollIntoView);
        builder.add_action(Action::SetTextSelection);
        if matches!(st.policy.access_role, AccessibilityRole::Editor) {
            builder.add_action(Action::SetValue);
            builder.add_action(Action::ReplaceSelectedText);
        }
    }

    fn clips_children(&self) -> bool {
        true
    }
}
