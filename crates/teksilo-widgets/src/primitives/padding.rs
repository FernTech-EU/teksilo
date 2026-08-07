// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Padding — a single-child layout container that adds insets around its child.
//!
//! `Padding` shrink-wraps a child widget and enlarges it by configurable insets
//! on each of the four sides. Horizontal insets are **leading/trailing**
//! (logical), not left/right (physical), so they flip automatically in RTL
//! locales. Each inset accepts a static `f32` or a reactive `Signal<f32>`; a
//! bound inset schedules a relayout whenever the signal fires, so theme-derived
//! spacing values take effect without rebuilding the widget tree.
//!
//! The grow weight, shrink weight, and compression floor reported by the child
//! are forwarded through the padding so a flexible or shrinkable child inside a
//! `Padding` stays flexible or shrinkable from the parent's perspective.
//!
//! ## When to use
//!
//! - Adding whitespace around a widget without wrapping it in a stack.
//! - Applying asymmetric insets (e.g. extra leading inset for a list item).
//! - Reacting to a `Signal`-driven spacing token.
//!
//! Use [`Padding::uniform`] when all four sides are equal, and
//! [`Padding::symmetric`] when horizontal and vertical insets differ.
//!
//! ```rust
//! # use teksilo_widgets::primitives::{Padding, TextWidget};
//! # use teksilo_i18n::lit;
//! // 12 dp padding on every side:
//! let _w = Padding::uniform(12.0)
//!     .child(TextWidget::new(lit!("Hello")));
//! ```

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use teksilo_core::WidgetId;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::signal::Prop;
use teksilo_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};

/// A layout container that adds padding (insets) around a single child.
///
/// See the [module documentation](self) for the full feature description and
/// an example. Construct with [`Padding::new`], [`Padding::uniform`], or
/// [`Padding::symmetric`]; attach a child with `.child(widget)` or
/// `.child_id(id)`.
#[derive(Debug)]
pub struct Padding {
    top: Prop<f32>,
    trailing: Prop<f32>,
    bottom: Prop<f32>,
    leading: Prop<f32>,
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
}

impl Padding {
    /// Create a padding with explicit per-side insets.
    ///
    /// Argument order mirrors CSS shorthand: `(top, trailing, bottom, leading)`.
    /// `trailing` and `leading` are **logical** — they map to physical right and
    /// left in LTR and are swapped in RTL.
    pub fn new(
        top: impl Into<Prop<f32>>,
        trailing: impl Into<Prop<f32>>,
        bottom: impl Into<Prop<f32>>,
        leading: impl Into<Prop<f32>>,
    ) -> Self {
        Self {
            top: top.into(),
            trailing: trailing.into(),
            bottom: bottom.into(),
            leading: leading.into(),
            child_id: None,
            pending_child: None,
        }
    }

    /// Create a padding with the same inset on all four sides.
    pub fn uniform(amount: impl Into<Prop<f32>>) -> Self {
        let amount = amount.into();
        Self {
            top: amount.clone(),
            trailing: amount.clone(),
            bottom: amount.clone(),
            leading: amount,
            child_id: None,
            pending_child: None,
        }
    }

    /// Create a padding with equal top/bottom insets and equal leading/trailing insets.
    ///
    /// `vertical` applies to both top and bottom; `horizontal` applies to both
    /// leading and trailing sides (logical, RTL-aware).
    pub fn symmetric(vertical: impl Into<Prop<f32>>, horizontal: impl Into<Prop<f32>>) -> Self {
        let vertical = vertical.into();
        let horizontal = horizontal.into();
        Self {
            top: vertical.clone(),
            trailing: horizontal.clone(),
            bottom: vertical,
            leading: horizontal,
            child_id: None,
            pending_child: None,
        }
    }

    /// Set child by pre-registered ID.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    fn horizontal_inset(&self) -> f32 {
        self.leading.get() + self.trailing.get()
    }

    fn vertical_inset(&self) -> f32 {
        self.top.get() + self.bottom.get()
    }
}

impl Widget for Padding {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // Register each inset prop for dirty-tracking so bound insets
        // (e.g. a theme-derived signal) trigger a relayout when they fire.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.top.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::Relayout,
        );
        self.trailing.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::Relayout,
        );
        self.bottom.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::Relayout,
        );
        self.leading.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::Relayout,
        );
        self.child_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let h_inset = self.horizontal_inset();
        let v_inset = self.vertical_inset();

        // Query the child, then add insets — forwarding its grow weight,
        // shrink weight, and compression floor so a padded flexible/shrinkable
        // child stays flexible/shrinkable (the floor grows by the insets).
        if let Some(child_id) = self.child_id {
            let inner_proposal = SizeProposal {
                width: proposal.width.map(|w| (w - h_inset).max(0.0)),
                height: proposal.height.map(|h| (h - v_inset).max(0.0)),
            };
            if let Some(r) = ctx.child_layout_response(child_id, inner_proposal) {
                let size = Size::new(r.size.width + h_inset, r.size.height + v_inset);
                let min = Size::new(r.min.width + h_inset, r.min.height + v_inset);
                return teksilo_core::widget::LayoutResponse::flexible(size, r.flex)
                    .with_shrink(r.shrink)
                    .with_min(min);
            }
        }

        let size = proposal.resolve(h_inset, v_inset);
        Size::new(size.width.max(h_inset), size.height.max(v_inset)).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let top = self.top.get();
        let h_inset = self.horizontal_inset();
        let v_inset = self.vertical_inset();
        // Flip leading/trailing to physical left/right for RTL locales.
        let phys_left = if ctx.is_rtl() {
            self.trailing.get()
        } else {
            self.leading.get()
        };
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x + phys_left, bounds.y + top);
            child.size = Size::new(
                (bounds.width - h_inset).max(0.0),
                (bounds.height - v_inset).max(0.0),
            );
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
