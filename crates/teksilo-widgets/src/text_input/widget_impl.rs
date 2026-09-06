// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for [`TextInput`]: the composed
//! frame (border, placeholder overlay, clear button, slots), layout,
//! placement and accessibility.

use super::*;
impl Widget for TextInput {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // TextInput is a heavy composite. We snapshot the theme once for
        // static layout params (padding, border width, field height); the
        // placeholder, clear-icon tint, and border/width are driven by
        // roles and state signals, so theme switches repaint via the
        // paint-time role resolver without riding through a zip here.
        let _theme = ctx.theme();
        use crate::styles::recipe_text_input_style as field_dims;
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let interaction = self.interaction.clone();
        let validation = self.validation.clone();

        // ── Build the inner editing primitive ──────────────────────
        //
        // The inner field owns the bound text signal, the document,
        // engine, caret, clipboard, context menu — everything
        // interactive. The composite just styles it.
        let inner_height =
            (field_dims::TEXT_FIELD_HEIGHT - 2.0 * field_dims::TEXT_FIELD_BORDER_WIDTH).max(0.0);
        let text_area_height =
            (inner_height - 2.0 * field_dims::TEXT_FIELD_PADDING_VERTICAL).max(0.0);

        let mut field = TextInputField::new(self.text.clone()).share_handle(&self.field_handle);
        field = field
            .enabled(self.enabled.clone())
            .read_only(self.read_only)
            .placeholder(self.placeholder.clone())
            .text_height(text_area_height)
            .interaction_signal(interaction.clone());
        if let Some(max) = self.max_length {
            field = field.max_length(max);
        }
        if let Some(f) = self.char_filter.take() {
            // Re-wrap the Rc'd closure into a plain closure for the
            // primitive's builder surface, which owns its own Rc.
            field = field.char_filter(move |c| (f)(c));
        }
        // Cloned, not taken: the payload is already an `Rc`, and `build` runs
        // again on every rebuild of this widget. Taking it would leave the
        // second build with no assistive-technology write path, so a `SpinBox`
        // or date editor would silently stop committing an AT `SetValue` after
        // the first rebuild.
        if let Some(cb) = self.on_access_set_value.clone() {
            field = field.on_access_set_value(move |text, ctx| (cb)(text, ctx));
        }
        if let Some(cb) = self.on_submit.take() {
            field = field.on_submit_fn(move |ctx| (cb)(ctx));
        }
        if let Some(cb) = self.on_blur.take() {
            field = field.on_blur_fn(move |ctx| (cb)(ctx));
        }
        if !self.suffix.is_empty() {
            field = field.suffix(std::mem::take(&mut self.suffix));
        }
        if let Some(mask) = self.input_mask.take() {
            field = field.input_mask(mask);
        }
        field = field.input_purpose(self.input_purpose);
        if let Some(active) = self.active_descendant.clone() {
            field = field.active_descendant(active);
        }
        if let Some(controls) = self.controls.clone() {
            field = field.controls(controls);
        }
        let validator_installed = self.validator.is_some();
        if let Some(validator) = self.validator.take() {
            // ValidatorFn is `Rc<dyn Fn(&str) -> ValidationOutcome>`.
            // The primitive's builder takes a fresh closure; wrap the
            // Rc in one so the caller can keep their own clones if
            // they captured it before.
            field = field.validator(move |s| (validator)(s));
        }

        // Expose the field's text signal for downstream reactivity
        // (placeholder visibility, clear-button visibility) before
        // the field is consumed by `ctx.add`.
        let text_signal_for_vis = field.text();

        // Capture the inner field's reactive accessors BEFORE
        // `ctx.add` consumes it, so composing widgets that called
        // `caret_position()` / `caret_setter()` /
        // `validation_feedback_signal()` on us pre-build see live
        // updates through the slots we mirror into.
        let inner_caret = field.caret_position();
        let inner_setter = field.caret_setter();
        let inner_feedback = field.validation_feedback_signal();

        // Add the field directly so we can capture its own WidgetId (needed to
        // wire the validation strip as its `described_by`, below); wrap it by
        // id instead of moving it into `Padding`.
        //
        // The accessible name goes on the *field*, not on the composite's
        // outer node. The outer node is a `Role::GenericContainer`, and
        // `accesskit_consumer::common_filter` excludes that role from the
        // filtered tree unconditionally — a name written there reaches no
        // screen reader on any platform. The field is the node that carries
        // `Role::TextInput` and holds focus, so it is the one that must be
        // named. (`PasswordField` names its inner field the same way.)
        // `LocalizedString -> Prop<String>` keeps the name locale-reactive.
        let field_id = match self.label.clone() {
            Some(label) => ctx.add(field.access_label(label)),
            None => ctx.add(field),
        };
        self.field_id_slot.set(Some(field_id));

        // Text editing area, wrapped in vertical padding so slots
        // (IconButton etc.) sit flush against top/bottom of the
        // inner border area and are vertically centered by the HStack.
        let padded_field = Padding::new(
            field_dims::TEXT_FIELD_PADDING_VERTICAL,
            0.0,
            field_dims::TEXT_FIELD_PADDING_VERTICAL,
            0.0,
        )
        .child_id(field_id);

        // The placeholder lives in a local ZStack with the text field so
        // it shares the same column in the HStack — no overlap with
        // leading/trailing slots. The text field is the last ZStack child
        // so it wins hit-testing (ZStack tests children in reverse order).
        // `respect_intrinsic` on these `Expand` wrappers preserves the
        // wrapped field's natural width (≈200 dp from `TextInputField`)
        // as the column's intrinsic width. The enclosing `ZStack`
        // measures its children with an unspecified proposal, so the
        // parent's offered width never reaches the `HStack` during
        // measurement — without auto-basis the column reports 0 dp and
        // the whole composite collapses to `MinSize`'s 65 dp floor.
        let text_column_id = if !self.placeholder.resolve_now().is_empty() {
            // Match the inner TextInputField's text style + single-line
            // behaviour so the placeholder layout box has the same
            // intrinsic height as the rich-text engine's frame. Without
            // `single_line()` the placeholder defaults to Wrap, which
            // can report extra vertical leading space.
            let ph = TextWidget::new(self.placeholder.clone())
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary)
                .single_line()
                .a11y_hidden();
            // Align the placeholder on the column's vertical midline,
            // pinned to the leading edge where the typed text starts.
            // `Padding(top=padding_vertical, bottom=padding_vertical)`
            // pinned the placeholder to the top of its inset box, but
            // the rich-text engine inside the field paints glyphs with
            // its own line-leading offset, so the two paths drifted
            // by a few pixels; aligning purely on the layout-box midline
            // matches the engine's frame midline. Align mode measures the
            // placeholder under the column's bounds, so the `single_line()`
            // TextWidget caps itself at the available width and truncates
            // with a trailing "…" when the field is too narrow, instead of
            // painting its full line past the frame.
            let ph_id = ctx.add(
                Expand::new()
                    .respect_intrinsic()
                    .align_child(Alignment::CENTER_LEADING)
                    .child(ph),
            );
            let visible = text_signal_for_vis.map(|t| t.is_empty());
            ctx.visible_when(ph_id, visible);

            // `Expand::horizontal().respect_intrinsic()` keeps the field's
            // natural (mask-aware) width as the column's basis — so the
            // composite reports a snug width when unconstrained and fills a
            // wide frame via flex. Wrapping it in `Shrinkable` adds a shrink
            // weight so a narrow row compresses the column below that basis and
            // the field scrolls instead of overflowing.
            ctx.add(
                Shrinkable::new().child(
                    Expand::horizontal().respect_intrinsic().child(
                        ZStack::new()
                            .add_child(ph_id) // below (placeholder)
                            .child(padded_field), // on top (text field, gets hits)
                    ),
                ),
            )
        } else {
            ctx.add(
                Shrinkable::new()
                    .child(Expand::horizontal().respect_intrinsic().child(padded_field)),
            )
        };

        // HStack: [leading] [text_column] [clear] [trailing]
        let mut row = HStack::new().spacing(4.0);

        if let Some(leading) = self.leading_slot.take() {
            let leading_id = ctx.add_boxed(leading);
            row = row.add_child(leading_id);
        }

        row = row.add_child(text_column_id);

        // Clear button (opt-in). The clear affordance clears the
        // bound text signal — the field's ext→internal effect
        // picks this up and wipes the document.
        if self.show_clear_button {
            let icon = (crate::icon_button::BuiltInIcons::global().clear)()
                .icon_size(12.0)
                .color(TextRole::Secondary);
            let text_for_clear = self.text.clone();
            let clear_id = ctx.add(
                MinSize::new(16.0, 16.0)
                    .child(crate::primitives::Center::new().child(icon))
                    .on_tap(move |_pos, ctx| {
                        text_for_clear.set(String::new());
                        ctx.request_frame();
                    })
                    .cursor(CursorIcon::Pointer),
            );
            let visible = text_signal_for_vis.map(|t| !t.is_empty());
            ctx.visible_when(clear_id, visible);
            let reserve_id = ctx.add(
                crate::primitives::FixedSize::new()
                    .width(16.0_f32)
                    .height(16.0_f32)
                    .child_id(clear_id),
            );
            row = row.add_child(reserve_id);
        }

        if let Some(trailing) = self.trailing_slot.take() {
            let trailing_id = ctx.add_boxed(trailing);
            row = row.add_child(trailing_id);
        }

        let row_id = ctx.add(row);

        // Derive the cfg signals the style needs. Map our internal
        // `InteractionState` (5-way) to the trait's 3 boolean signals,
        // and the composite `ValidationState` (carries a message) to
        // the trait's flat `TextInputValidationLevel` enum.
        let is_focused = interaction.map(|s| *s == InteractionState::Focused);
        let is_hovered = interaction.map(|s| *s == InteractionState::Hovered);
        // `is_disabled` derives from the arena (not from interaction).
        let effective_enabled = ctx.effective_enabled_signal(self_id);
        let is_disabled = effective_enabled.map(|on| !*on);
        let validation_level = validation.map(|v| match v {
            ValidationState::None => TextInputValidationLevel::None,
            ValidationState::Error(_) => TextInputValidationLevel::Error,
            ValidationState::Warning(_) => TextInputValidationLevel::Warning,
            ValidationState::Corrected(_) => TextInputValidationLevel::Corrected,
        });

        // Resolve the active style: per-call override > theme slot >
        // built-in `RecipeTextInputStyle` default. The style paints the
        // bordered/filled frame + the corner radius + the horizontal
        // padding around the editor row.
        let style: SharedTextInputStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.text_input.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeTextInputStyle::default()));

        let cfg = TextInputStyleConfig {
            editor: row_id,
            is_focused,
            is_hovered,
            is_disabled,
            validation: validation_level,
            variant: self.variant,
        };
        let chrome_id = style.make_body(&cfg, ctx);

        let min_w = self.min_width.unwrap_or(65.0);
        let frame_id =
            ctx.add(MinSize::new(min_w, field_dims::TEXT_FIELD_HEIGHT).child_id(chrome_id));

        // ── Inline validation strip ────────────────────────────────
        // Maps `Signal<ValidationState>` to the `Signal<ValidationFeedback>`
        // that `ValidationStrip` consumes. Empty/Pristine renders nothing
        // (zero height) so the layout doesn't reflow.
        let strip_feedback: Signal<ValidationFeedback> = self.validation.map(|v| match v {
            ValidationState::None => ValidationFeedback::Pristine,
            ValidationState::Error(msg) | ValidationState::Warning(msg) => {
                ValidationFeedback::Invalid {
                    message: msg.clone(),
                }
            }
            ValidationState::Corrected(msg) => ValidationFeedback::Corrected {
                message: msg.clone(),
                since: std::time::Instant::now(),
            },
        });
        let strip_id = ctx.add(ValidationStrip::new(strip_feedback));

        // WCAG 3.3.1 / 3.3.3 (EN 301 549 11.5.2.7): associate the inline
        // validation strip with the field so a screen reader announces the
        // error / warning / correction message as the field's description when
        // it gains focus. The strip renders nothing while Pristine, but the
        // relation is harmless then and live the moment a message appears.
        ctx.access_described_by(field_id, strip_id);

        // Wrap frame + strip in a VStack with the configured gap. The frame is
        // wrapped in `Expand::horizontal().respect_intrinsic()` so it claims
        // the VStack's full width (a `VStack` lays a child out at its measured
        // width, not stretched) while keeping the frame's natural width as the
        // basis when unconstrained. A bounded proposal narrows it and the
        // `Shrinkable` column compresses to fit.
        let framed_id = ctx.add(Expand::horizontal().respect_intrinsic().child_id(frame_id));
        let root_id = ctx.add(
            VStack::new()
                .spacing(field_dims::TEXT_FIELD_VALIDATION_STRIP_GAP)
                .add_child(framed_id)
                .add_child(strip_id),
        );

        // Tooltip — three mutually-exclusive setters; setters clear
        // the others so exactly one branch runs.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, root_id, text, delay);
        }

        // The interaction signal no longer carries Disabled — the
        // framework's arena enabled-state is the single source of
        // truth. Style chrome that needs `is_disabled` derives it
        // from `effective_enabled_signal(self_id)`.

        // Bridge `validation_feedback` source → composite state.
        // No dedupe — each commit changes the feedback identity even
        // when the user-visible message stays the same (e.g. repeated
        // Invalid commits), and the strip is cheap to repaint.
        if let Some(src) = self.feedback_to_bridge.clone() {
            let target = self.validation.clone();
            ctx.effect(&src, move |fb| {
                target.set(feedback_to_state(fb));
            });
        } else if validator_installed {
            // Auto-bridge: a validator was installed but no explicit
            // `validation_feedback` source was provided. Mirror
            // the inner field's published outcome into our display
            // state so calling `.validator(...)` on TextInput "just
            // works" — the strip and border respond without a
            // separate `.validation_feedback(...)` call.
            let target = self.validation.clone();
            let src = inner_feedback.clone();
            ctx.effect(&src, move |fb| {
                target.set(feedback_to_state(fb));
            });
        }

        // Mirror inner field accessors into the slots that were
        // captured before build by composing widgets.
        //
        // - caret_position: only mirror if the slot was lazy-initialized
        //   (i.e. someone called `caret_position()` on us pre-build).
        //   Seed with the current value, then forward changes.
        // - caret_setter: store the inner field's setter Rc; the closure
        //   we returned to callers forwards through this slot at call time.
        // - validation_feedback_signal: always mirror (the slot's signal
        //   is created in `new()` and may already have observers).
        if let Some(target) = self.caret_position_slot.borrow().clone() {
            target.set(inner_caret.get());
            ctx.effect(&inner_caret, move |pos| {
                if target.get() != *pos {
                    target.set(*pos);
                }
            });
        }
        *self.caret_setter_slot.borrow_mut() = Some(inner_setter);
        let outer_feedback = self.feedback_signal.clone();
        outer_feedback.set(inner_feedback.get());
        ctx.effect(&inner_feedback, move |fb| {
            outer_feedback.set(fb.clone());
        });

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // The `Shrinkable` + `respect_intrinsic` editor column reports the
        // field's natural (mask-aware) width when unconstrained, fills a wide
        // frame via flex, and compresses on a deficit — so the composite just
        // forwards its child's response.
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
        if let Some(p) = children.first_mut() {
            p.origin = Point::new(bounds.x, bounds.y);
            p.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    /// Focus belongs on the inner field, never on this composite: the outer
    /// node is a `Role::GenericContainer` and is not focusable at all.
    ///
    /// Without this, a `TextInput` inside deferred modal content could not be
    /// focused on open. The modal pipeline asks the content tree for a hint
    /// before falling back to the first focusable descendant, and a composite
    /// that answered nothing was skipped over.
    fn initial_focus_hint(&self) -> Option<WidgetId> {
        self.field_id_slot.get()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The inner TextInputField handles Role::TextInput. The outer
        // composite is transparent to a11y: `Role::GenericContainer` is
        // excluded from the filtered tree by
        // `accesskit_consumer::common_filter`, so nothing written here
        // reaches a screen reader. In particular the `label` is NOT set
        // here — it is applied to the inner field in `build`, which is the
        // node that survives the filter and holds focus.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
        // Framework a11y walker sets `set_disabled` from arena state.
    }
}
