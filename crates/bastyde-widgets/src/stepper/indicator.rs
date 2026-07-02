// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`StepIndicator`] — one step's marker in the indicator strip: a numbered
//! status circle followed by the step title (Ant/MUI horizontal-steps layout).
//!
//! Carries the step's accessibility node: `Role::Tab` (non-linear, clickable)
//! or `Role::ListItem` (linear), with `selected` / `aria-current="step"` /
//! `posinset` / `setsize` / `controls → panel`.
//!
//! v1 paints the marker circle + number/glyph + the title; connector lines
//! between markers and a Tier-3 `StepIndicatorStyle` trait are deferred to v2.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::RecipeColor;
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{LocalizedString, lit};
use bastyde_tokens::{BorderRole, FontWeight, SurfaceRole, TextRole, TextStyle, TextStyleRole};

use super::StepperOrientation;
use super::step::StepStatus;
use crate::primitives::{TextWidget, VStack};

pub(crate) const DEFAULT_CIRCLE_SIZE: f32 = 28.0;
const MARKER_LABEL_GAP: f32 = 10.0;

type ActivateFn = Rc<dyn Fn(&mut EventContext)>;

pub(crate) struct StepIndicator {
    index: usize,
    total: usize,
    title: LocalizedString,
    supporting_text: Option<LocalizedString>,
    status: Signal<StepStatus>,
    current: Signal<usize>,
    orientation: StepperOrientation,
    clickable: bool,
    circle_size: f32,
    on_activate: Option<ActivateFn>,
    /// Shared, declaration-order panel ids — used to resolve `controls`.
    panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    label_block_id: Option<WidgetId>,
}

impl StepIndicator {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        index: usize,
        total: usize,
        title: LocalizedString,
        supporting_text: Option<LocalizedString>,
        status: Signal<StepStatus>,
        current: Signal<usize>,
        orientation: StepperOrientation,
        clickable: bool,
        circle_size: f32,
        on_activate: Option<ActivateFn>,
        panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    ) -> Self {
        Self {
            index,
            total,
            title,
            supporting_text,
            status,
            current,
            orientation,
            clickable,
            circle_size,
            on_activate,
            panel_ids,
            label_block_id: None,
        }
    }

    fn is_active(&self) -> bool {
        self.current.get() == self.index
    }

    /// Marker (circle) bounding rect inside `bounds` — leading edge,
    /// vertically centred.
    fn marker_rect(&self, bounds: Rect) -> Rect {
        let d = self.circle_size;
        let cy = bounds.y + (bounds.height - d).max(0.0) / 2.0;
        Rect::new(bounds.x, cy, d, d)
    }
}

impl std::fmt::Debug for StepIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepIndicator")
            .field("index", &self.index)
            .field("total", &self.total)
            .finish()
    }
}

impl Widget for StepIndicator {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Locale-reactive title (+ optional supporting line). Resolving inside
        // a locale-bound `map` keeps the bound text widgets retitling on a
        // locale switch without rebuilding the stepper.
        let locale = ctx.locale_signal();
        let title = self.title.clone();
        let title_sig = locale.map(move |_| title.resolve_now());

        let mut block = VStack::new().spacing(2.0).child(
            TextWidget::new(lit!(""))
                .text(title_sig)
                .style(TextStyleRole::Body)
                .color(if self.is_active() {
                    TextRole::Primary
                } else {
                    TextRole::Secondary
                })
                .single_line(),
        );
        if let Some(ref supporting) = self.supporting_text {
            let supporting = supporting.clone();
            let locale2 = ctx.locale_signal();
            let supporting_sig = locale2.map(move |_| supporting.resolve_now());
            block = block.child(
                TextWidget::new(lit!(""))
                    .text(supporting_sig)
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary)
                    .single_line(),
            );
        }
        let block_id = ctx.add(block);
        self.label_block_id = Some(block_id);

        // Repaint the marker (status color / active highlight) and re-emit the
        // a11y node (selected / aria-current) when the status or active step
        // changes — the stepper does not rebuild on navigation.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        for level in [
            bastyde_core::binding::BindingLevel::RepaintOnly,
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        ] {
            self.status.bind_to(self_id, registry, level);
            self.current.bind_to(self_id, registry, level);
        }

        // Clickable (non-linear) markers activate on tap / Enter / Space.
        if self.clickable {
            if let Some(activate) = self.on_activate.clone() {
                let activate2 = activate.clone();
                let handlers = HandlerSet::new()
                    .focusable(true)
                    .cursor(CursorIcon::Pointer)
                    .on_tap(move |_pos, ctx| activate(ctx))
                    .on_access_action(move |action, ctx| {
                        if action == bastyde_core::accesskit::Action::Click {
                            activate2(ctx);
                            bastyde_core::event::EventResponse::Handled
                        } else {
                            bastyde_core::event::EventResponse::Ignored
                        }
                    });
                ctx.apply_self_handlers(handlers);
            }
        }

        vec![block_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let d = self.circle_size;
        // Titles are single-line, so their height is width-independent —
        // measuring at the full proposal is fine.
        let label = self
            .label_block_id
            .and_then(|id| ctx.child_size(id, proposal))
            .map(|r| Size::new(r.width, r.height))
            .unwrap_or(Size::new(0.0, 0.0));
        let w = d + MARKER_LABEL_GAP + label.width;
        let h = d.max(label.height);
        proposal.resolve(w, h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let d = self.circle_size;
        for child in children.iter_mut() {
            let x = bounds.x + d + MARKER_LABEL_GAP;
            let w = (bounds.width - d - MARKER_LABEL_GAP).max(0.0);
            child.origin = Point::new(x, bounds.y);
            child.size = Size::new(w, bounds.height);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let status = self.status.get();
        let marker = self.marker_rect(bounds);
        let center = Point::new(
            marker.x + marker.width / 2.0,
            marker.y + marker.height / 2.0,
        );
        let radius = marker.width / 2.0;

        // Per-status marker fill / stroke / glyph color.
        let (fill, stroke, fg, glyph): (SurfaceRole, Option<BorderRole>, TextRole, Glyph) =
            match status {
                StepStatus::Active => {
                    (SurfaceRole::Accent, None, TextRole::OnAccent, Glyph::Number)
                }
                StepStatus::Complete => {
                    (SurfaceRole::Accent, None, TextRole::OnAccent, Glyph::Check)
                }
                StepStatus::Error => (
                    SurfaceRole::Content,
                    Some(BorderRole::Error),
                    TextRole::Error,
                    Glyph::Bang,
                ),
                StepStatus::Disabled => (
                    SurfaceRole::Content,
                    Some(BorderRole::Divider),
                    TextRole::Disabled,
                    Glyph::Number,
                ),
                StepStatus::Skipped => (
                    SurfaceRole::Content,
                    Some(BorderRole::Divider),
                    TextRole::Secondary,
                    Glyph::Dash,
                ),
                StepStatus::Upcoming | StepStatus::Optional => (
                    SurfaceRole::Content,
                    Some(BorderRole::Divider),
                    TextRole::Secondary,
                    Glyph::Number,
                ),
            };

        canvas.fill_circle(center, radius, RecipeColor::from(fill).resolve(theme));
        if let Some(border) = stroke {
            canvas.stroke_circle(
                center,
                radius - 1.0,
                RecipeColor::from(border).resolve(theme),
                2.0,
            );
        }

        // Glyph (number / check / bang / dash), centred in the circle.
        let text = match glyph {
            Glyph::Number => (self.index + 1).to_string(),
            Glyph::Check => "✓".to_string(),
            Glyph::Bang => "!".to_string(),
            Glyph::Dash => "–".to_string(),
        };
        let Some(backend) = canvas.text_backend().cloned() else {
            return;
        };
        let style = TextStyle {
            family: theme.typography.body_bold.family.clone(),
            size: self.circle_size * 0.46,
            weight: FontWeight::SEMI_BOLD,
            line_height: 1.0,
            letter_spacing: 0.0,
        };
        let layout = backend.borrow_mut().layout_single_line(&text, &style, None);
        let pos = Rect::new(
            center.x - layout.width / 2.0,
            center.y - layout.height / 2.0,
            layout.width,
            layout.height,
        );
        canvas.draw_text(&text, pos, &style, RecipeColor::from(fg).resolve(theme));
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        let active = self.is_active();
        if self.clickable {
            builder.set_role(bastyde_core::accesskit::Role::Tab);
            builder.set_selected(active);
            builder.add_action(bastyde_core::accesskit::Action::Focus);
            builder.add_action(bastyde_core::accesskit::Action::Click);
            // controls → the content panel at this index.
            if let Some(&panel_id) = self.panel_ids.borrow().get(self.index) {
                builder
                    .push_controlled(bastyde_core::accessibility::widget_id_to_node_id(panel_id));
            }
        } else {
            builder.set_role(bastyde_core::accesskit::Role::ListItem);
        }
        builder.set_name(self.title.resolve_now());
        if active {
            builder.set_aria_current(bastyde_core::accesskit::AriaCurrent::Step);
        }
        builder.inner_mut().set_position_in_set(self.index + 1);
        builder.inner_mut().set_size_of_set(self.total);
        let _ = self.orientation; // orientation drives strip layout, not the node
    }

    fn children(&self) -> Vec<WidgetId> {
        self.label_block_id.into_iter().collect()
    }
}

enum Glyph {
    Number,
    Check,
    Bang,
    Dash,
}
