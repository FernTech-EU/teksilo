// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Accordion — a collapsible section with a clickable header that shows or hides
//! its content when activated.
//!
//! In the default vertical mode a horizontally-spanning header row sits above the
//! content; clicking or pressing Space/Enter toggles visibility with an animated
//! height disclosure (via [`Collapse`]).
//! A horizontal mode flips the header into a narrow vertical strip with a rotated
//! label — used by top/bottom sides of a `DockingLayout`. Fill mode (`.fill(true)`)
//! is designed for fixed-size slots such as Splitter panes: the content fills all
//! available space and collapse animation is driven externally by the enclosing
//! pane rather than by an internal height tween.
//!
//! ## Accessibility
//!
//! The header is announced as `Role::Button` with `aria-expanded` reflecting the
//! current state, and `aria-controls` pointing at the content region
//! (`Role::Region`). Space/Enter toggle the disclosure; AT "click" actions are
//! also handled. The focus ring appears only on keyboard focus (not on pointer
//! clicks), matching the IntUI convention.
//!
//! ```rust
//! # use teksilo_widgets::accordion::Accordion;
//! # use teksilo_core::signal::Signal;
//! # use teksilo_i18n::lit;
//! let expanded = Signal::new(false);
//! let _accordion = Accordion::new(lit!("Advanced settings"), expanded);
//! ```

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::{ColorProp, TextStyleProp};
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, TextRole, TextStyleRole};

use crate::animations::collapse::Collapse;
use crate::primitives::{HStack, IconWidget, MinSize, Spacer, TextWidget, VStack};
use crate::tool_box::RotatedLabel;
use teksilo_i18n::LocalizedString;

/// Fixed header extent (px) along the main axis in [`Accordion::fill`] mode, so
/// a collapsed dock pane is exactly the header with no content sliver.
pub(crate) const ACCORDION_FILL_HEADER_EXTENT: f32 = 30.0;
/// The size a fill-mode accordion's enclosing Splitter pane collapses to —
/// the header extent plus the header→body gap. See `place_fill`.
pub(crate) const ACCORDION_FILL_COLLAPSED_EXTENT: f32 = ACCORDION_FILL_HEADER_EXTENT + 2.0;

// ---------------------------------------------------------------------------
// AccordionRegion — thin wrapper that exposes Role::Region for aria-controls.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct AccordionRegion {
    /// Kept as a `LocalizedString` (not eagerly resolved) so the region's
    /// AT name follows a live locale switch — `accessibility()` re-runs on
    /// the AT re-walk and re-resolves below.
    name: LocalizedString,
    child: Option<WidgetId>,
}

impl AccordionRegion {
    fn new(name: LocalizedString, child: WidgetId) -> Self {
        Self {
            name,
            child: Some(child),
        }
    }
}

impl Widget for AccordionRegion {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.child
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
            child.origin = teksilo_canvas::Point::new(bounds.x, bounds.y);
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Region);
        builder.set_name(self.name.resolve_now());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Accordion widget
// ---------------------------------------------------------------------------

/// Height of the accordion header row in pixels (vertical mode).
pub const ACCORDION_HEADER_HEIGHT: f32 = 28.0;
/// Horizontal padding inside the accordion header on the leading and trailing edges.
pub const ACCORDION_HEADER_PADDING_HORIZONTAL: f32 = 8.0;
/// Size of the chevron disclosure indicator icon in pixels.
pub const ACCORDION_INDICATOR_SIZE: f32 = 12.0;
/// Gap between the disclosure indicator and the title label.
pub const ACCORDION_INDICATOR_GAP: f32 = 6.0;
/// Corner radius of the keyboard-focus ring painted on the accordion header.
pub const ACCORDION_CORNER_RADIUS: f32 = 4.0;

/// Orientation of an [`Accordion`]: how its header sits relative to its
/// content. [`Vertical`](AccordionOrientation::Vertical) (the default) is a
/// horizontal header row above the content; [`Horizontal`](AccordionOrientation::Horizontal)
/// is a narrow vertical header **strip** (rotated-90° label, left/right
/// chevron) beside the content — used by top/bottom dock sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccordionOrientation {
    /// Header row above the content (default).
    #[default]
    Vertical,
    /// Vertical header strip beside the content.
    Horizontal,
}

/// A collapsible section widget whose header button shows or hides attached content.
///
/// Supply the title and a `Signal<bool>` for the expanded state, then attach
/// content via [`.content(w)`](Accordion::content) or
/// [`.content_id(id)`](Accordion::content_id). The signal can be toggled externally
/// (e.g. from a "collapse all" button) and the disclosure animation will follow.
pub struct Accordion {
    /// Header title. Kept as a `LocalizedString` (not eagerly resolved)
    /// so a `tr!(...)` / `tr_widget!(...)` source re-renders on locale
    /// change: the title `TextWidget` binds it as a reactive prop.
    title: LocalizedString,
    expanded: Signal<bool>,
    content_id: Option<WidgetId>,
    pending_content: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
    /// Region wrapper ID — used for `aria-controls` on the header button.
    region_id: Option<WidgetId>,
    /// Optional override for the header foreground color (title text +
    /// chevron icon). When `None`, the accordion uses [`TextRole::Primary`].
    /// Set this when the accordion is embedded inside a surface that uses a
    /// non-standard text color (rich tooltip, dark snackbar, etc.). Accepts
    /// any `impl Into<ColorProp>` — a literal `Color`, a role, or a
    /// `Signal<Color>` — so the override stays theme-reactive.
    title_color: Option<ColorProp>,
    /// Optional override for the header title's text style. Defaults to
    /// [`TextStyleRole::Body`] when `None`. Accepts a static
    /// [`TextStyle`](teksilo_tokens::TextStyle) or a
    /// [`TextStyleRole`].
    title_style: Option<TextStyleProp>,
    /// Header orientation (default [`AccordionOrientation::Vertical`]).
    orientation: AccordionOrientation,
    /// When set, the expanded content **fills** the accordion's allotted space
    /// for a fixed-size slot (e.g. a Splitter pane), with an **animated**
    /// collapse — rather than the default natural-height disclosure. See
    /// [`Accordion::fill`].
    fill: bool,
    /// Optional drag-source hook: when set, a drag gesture starting on the
    /// header fires this (it typically calls `ctx.start_drag(...)`). Tap-to-
    /// toggle still works — the gesture arena disambiguates.
    on_header_drag: Option<std::rc::Rc<dyn Fn(&mut EventContext)>>,
    /// Optional trailing header slot — a widget placed at the trailing end of
    /// the header (before the disclosure chevron), e.g. a "More actions" (`⋮`)
    /// options button or an inline action toolbar. Its own tap/press recognizer
    /// captures first (innermost-hit wins), so operating it does **not** toggle
    /// the disclosure. See [`Accordion::trailing`].
    trailing: Option<Box<dyn Widget>>,
    /// A pre-registered trailing slot by id (for callers that must build the
    /// slot in-context first). Takes precedence over `trailing`. See
    /// [`Accordion::trailing_id`].
    trailing_id: Option<WidgetId>,
    /// Fill-mode layout state: the header + animated body are direct children
    /// laid out by the accordion itself (so the body fills the leftover *and*
    /// animates). `None` in the default (VStack-rooted) mode.
    fill_header_id: Option<WidgetId>,
    fill_body_id: Option<WidgetId>,
}

impl Accordion {
    /// Create a new accordion with the given `title` and an external `expanded` signal.
    ///
    /// The accordion starts collapsed or expanded according to the initial value of
    /// `expanded`. Toggling the signal later drives the disclosure animation.
    pub fn new(title: impl Into<LocalizedString>, expanded: Signal<bool>) -> Self {
        Self {
            title: title.into(),
            expanded,
            content_id: None,
            pending_content: None,
            root_child_id: None,
            region_id: None,
            title_color: None,
            title_style: None,
            orientation: AccordionOrientation::Vertical,
            fill: false,
            on_header_drag: None,
            trailing: None,
            trailing_id: None,
            fill_header_id: None,
            fill_body_id: None,
        }
    }

    /// Set the header orientation (default [`AccordionOrientation::Vertical`]).
    pub fn orientation(mut self, orientation: AccordionOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Shorthand for [`Accordion::orientation`]`(`[`AccordionOrientation::Horizontal`]`)`.
    pub fn horizontal(mut self) -> Self {
        self.orientation = AccordionOrientation::Horizontal;
        self
    }

    /// Make the expanded content **fill** the accordion's allotted space (the
    /// leftover after the header) — instead of the default natural-height
    /// disclosure — while keeping the collapse/expand **animated**. Use when the
    /// accordion lives in a fixed-size slot such as a Splitter pane (a dock
    /// panel): the content lays out at exactly the available size (no narrow
    /// content, no overflow) and the header tween still plays. Default `false`.
    pub fn fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self
    }

    /// Make the header a **drag source**: a drag gesture starting on it fires
    /// `f` (which should begin a drag, e.g. `ctx.start_drag(source, payload)`).
    /// Tap-to-toggle is unaffected — the gesture arena tells a tap from a drag.
    pub fn on_header_drag(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_header_drag = Some(std::rc::Rc::new(f));
        self
    }

    /// Place a widget at the trailing end of the header, before the disclosure
    /// chevron — an options (`⋮`) button, an inline action toolbar, etc. The
    /// slot's own controls capture their gestures (innermost hit wins), so
    /// clicking them does not toggle the accordion. Mirrors
    /// [`ToolBoxItem::trailing`](crate::tool_box::ToolBoxItem) /
    /// [`TabWidget::bar_trailing_slot`](crate::tab_widget::TabWidget::bar_trailing_slot).
    pub fn trailing(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing = Some(Box::new(widget));
        self
    }

    /// Like [`trailing`](Self::trailing) but takes a **pre-registered** widget
    /// id — for callers that must build the slot in-context (e.g. a slot that
    /// itself adds boxed children). Takes precedence over `trailing`.
    pub fn trailing_id(mut self, id: WidgetId) -> Self {
        self.trailing_id = Some(id);
        self
    }

    /// Override the header foreground color used for the title text and
    /// chevron icon. Defaults to [`TextRole::Primary`]. Accepts a literal
    /// `Color`, a `TextRole`/`SurfaceRole`, or a `Signal<Color>`.
    pub fn title_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.title_color = Some(color.into());
        self
    }

    /// Override the header title's text style. Use this to make the
    /// disclosure label smaller (e.g. inside a tooltip) or to match a
    /// non-body typography role. Accepts a static
    /// [`TextStyle`](teksilo_tokens::TextStyle) or a
    /// [`TextStyleRole`].
    pub fn title_style(mut self, style: impl Into<TextStyleProp>) -> Self {
        self.title_style = Some(style.into());
        self
    }

    /// Set the content widget by pre-registered ID.
    pub fn content_id(mut self, id: WidgetId) -> Self {
        self.content_id = Some(id);
        self
    }

    /// Set an inline content widget (deferred insertion).
    pub fn content(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_content = Some(Box::new(widget));
        self
    }
}

impl std::fmt::Debug for Accordion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accordion")
            .field("title", &self.title)
            .finish()
    }
}

impl Widget for Accordion {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Resolve deferred content if provided
        if let Some(pending) = self.pending_content.take() {
            self.content_id = Some(ctx.add_boxed(pending));
        }

        let theme = ctx.theme();
        let accordion_corner_radius = ACCORDION_CORNER_RADIUS;
        let focus_ring_width = theme.shape.focus_ring_width;
        let expanded = self.expanded.clone();

        // The header's focus ring (Int UI accent border) shows only on
        // *keyboard* focus. `header_focused` is the header's own focus; ANDing it
        // with the tree's focus-visible modality suppresses the ring for any
        // pointer-origin focus — including focus that falls through to the header
        // when a trailing-slot control (e.g. a toolbar button) is clicked without
        // itself taking focus. (This replaces the old pointer-hover heuristic,
        // which failed once a captured pointer cleared the header's hover.)
        let header_focused = ctx.signal(false);
        let kb_focused = header_focused.and(&ctx.focus_visible());

        // Refresh this node's announced `aria-expanded` whenever the state
        // flips — from a tap, the keyboard, or an external `expanded.set(...)`.
        // Without binding the signal to the accordion's own node the
        // `accessibility()` output isn't re-queried, so the announced state
        // would go stale (same mechanism Button uses for its disclosure
        // pattern).
        self.expanded.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

        // Header foreground: caller override wins, otherwise the Primary text
        // role so the title tracks theme changes. The title style defaults to
        // the Body role for the same reason.
        let header_fg: ColorProp = self
            .title_color
            .clone()
            .unwrap_or_else(|| TextRole::Primary.into());
        let title_style: TextStyleProp = self
            .title_style
            .clone()
            .unwrap_or_else(|| TextStyleRole::Body.into());

        let horizontal = self.orientation == AccordionOrientation::Horizontal;

        // Optional trailing header slot (options `⋮` button / action toolbar),
        // inserted just before the disclosure chevron in either orientation.
        // Wrap it in a [`DeadZone`](crate::primitives::DeadZone) so operating the
        // controls (even with a few px of click jitter) never starts the header
        // drag and gap-taps don't toggle the disclosure — while a press anywhere
        // ELSE on the header still drags. The block is structural (the node-level
        // `gesture_dead_zone` flag stops drag-arming at the boundary), so it is
        // robust against the capture-release path a recognizer-shadowing absorber
        // loses to.
        let trailing_id = self
            .trailing_id
            .or_else(|| self.trailing.take().map(|w| ctx.add_boxed(w)))
            .map(|tid| ctx.add(crate::primitives::DeadZone::new().child_id(tid)));

        // Header: a horizontal row (vertical orientation) or a narrow vertical
        // strip with a rotated label (horizontal orientation). Two chevrons
        // toggled by `visible_when` so the glyph updates reactively.
        let header = if horizontal {
            // Vertical strip: [chevron_left|right] [rotated title] [spacer].
            // Chevron points right while collapsed (content opens to the
            // right), left once expanded.
            let chevron_left_id = ctx.add(IconWidget::chevron_left(16.0).color(header_fg.clone()));
            let chevron_right_id =
                ctx.add(IconWidget::chevron_right(16.0).color(header_fg.clone()));
            ctx.visible_when(chevron_left_id, expanded.clone());
            ctx.visible_when(chevron_right_id, expanded.map(|v| !*v));
            let title_id = ctx.add(
                RotatedLabel::new(self.title.clone(), header_fg.clone()).style(title_style.clone()),
            );
            let spacer_id = ctx.add(Spacer::new());
            let mut col = VStack::new()
                .spacing(8.0)
                .add_child(chevron_left_id)
                .add_child(chevron_right_id)
                .add_child(title_id);
            if let Some(t) = trailing_id {
                col = col.add_child(t);
            }
            ctx.add(col.add_child(spacer_id))
        } else {
            let chevron_down_id = ctx.add(IconWidget::chevron_down(16.0).color(header_fg.clone()));
            let chevron_right_id =
                ctx.add(IconWidget::chevron_right(16.0).color(header_fg.clone()));
            ctx.visible_when(chevron_down_id, expanded.clone());
            ctx.visible_when(chevron_right_id, expanded.map(|v| !*v));

            // Rigid: the header title never truncates. When the header is tight
            // the trailing slot (an options `⋮` / a shrinkable toolbar) absorbs
            // the deficit, so the disclosure's label always stays readable.
            let title_widget = TextWidget::new(self.title.clone())
                .color(header_fg)
                .style(title_style.clone())
                .single_line()
                .no_shrink()
                .a11y_hidden();
            let title_id = ctx.add(title_widget);
            let spacer_id = ctx.add(Spacer::new());

            let mut row = HStack::new()
                .spacing(8.0)
                .add_child(title_id)
                .add_child(spacer_id);
            if let Some(t) = trailing_id {
                row = row.add_child(t);
            }
            ctx.add(row.add_child(chevron_down_id).add_child(chevron_right_id))
        };

        // Int UI focus convention: an accent-colored border
        // appears on the header row itself on keyboard focus
        // instead of a separate ring. Header has no visible
        // rest-state border, so this border is width-zero at
        // rest and snaps to `focus_ring_width` on focus.
        let focus_border_role = kb_focused.map(|f| {
            if *f {
                BorderRole::Focused
            } else {
                BorderRole::Transparent
            }
        });
        let focus_border_width = kb_focused.map(move |f| if *f { focus_ring_width } else { 0.0 });
        let focus_rect_id = ctx.add(
            crate::primitives::RectWidget::new()
                .border_color(focus_border_role)
                .border_width(focus_border_width)
                .corner_radius(teksilo_tokens::CornerRadius::uniform(
                    accordion_corner_radius,
                )),
        );
        let header_with_ring = ctx.add(
            crate::primitives::ZStack::new()
                .add_child(focus_rect_id)
                .add_child(header),
        );

        if self.fill {
            // Fill mode: the header + a `FillBody` are laid out by the accordion
            // itself (custom layout below) so the content **fills** the leftover
            // the enclosing Splitter pane gives it. The collapse *animation* is
            // the Splitter pane resizing (driven externally by the `expanded`
            // signal) — not this widget. The header carries a fixed minimum
            // extent so a fully-collapsed pane is exactly the header.
            let header = if horizontal {
                ctx.add(MinSize::new(ACCORDION_FILL_HEADER_EXTENT, 0.0).child_id(header_with_ring))
            } else {
                ctx.add(MinSize::new(0.0, ACCORDION_FILL_HEADER_EXTENT).child_id(header_with_ring))
            };
            self.fill_header_id = Some(header);
            if let Some(content_id) = self.content_id {
                let region_id = ctx.add(AccordionRegion::new(self.title.clone(), content_id));
                self.region_id = Some(region_id);
                let body = ctx.add(FillBody::new(region_id));
                self.fill_body_id = Some(body);
            }
        } else {
            // Default: the classic VStack/HStack root.
            // - horizontal → dormancy (Collapse only animates height).
            // - vertical → the animated `Collapse` disclosure.
            let content_wrapper = self.content_id.map(|content_id| {
                let region_id = ctx.add(AccordionRegion::new(self.title.clone(), content_id));
                self.region_id = Some(region_id);
                if horizontal {
                    ctx.visible_when(region_id, self.expanded.clone());
                    region_id
                } else {
                    ctx.add(Collapse::new(self.expanded.clone()).child_id(region_id))
                }
            });
            let root = if horizontal {
                let mut hstack = HStack::new().spacing(2.0).add_child(header_with_ring);
                if let Some(w) = content_wrapper {
                    hstack = hstack.add_child(w);
                }
                ctx.add(hstack)
            } else {
                let mut vstack = VStack::new().spacing(2.0).add_child(header_with_ring);
                if let Some(w) = content_wrapper {
                    vstack = vstack.add_child(w);
                }
                ctx.add(vstack)
            };
            self.root_child_id = Some(root);
        }

        // --- V2 attached handlers ---
        // Handlers just flip `expanded`; the inner `Collapse` widget
        // observes the signal and drives the height/width tween.
        let expanded_tap = self.expanded.clone();
        let expanded_key = self.expanded.clone();
        let expanded_access = self.expanded.clone();
        let header_focused_focus = header_focused.clone();

        let mut handler_set = HandlerSet::new()
            .on_tap({
                move |_pos, _ctx: &mut EventContext| {
                    expanded_tap.set(!expanded_tap.get());
                }
            })
            .on_access_action({
                // An AT "press" / default-action toggles the disclosure, the
                // same as a pointer tap or Space/Enter. Without this an
                // assistive technology can navigate to the header (it
                // advertises `Action::Click`) but cannot operate it.
                move |action: teksilo_core::accesskit::Action,
                      _ctx: &mut EventContext|
                      -> EventResponse {
                    if action == teksilo_core::accesskit::Action::Click {
                        expanded_access.set(!expanded_access.get());
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .on_key({
                move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space | Key::Enter,
                            ..
                        } => EventResponse::Handled,
                        WidgetEvent::KeyUp {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            expanded_key.set(!expanded_key.get());
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_focus({
                move |gained: bool, _ctx: &mut EventContext| {
                    // The `kb_focused` ring signal ANDs this with focus-visible,
                    // so a pointer-origin focus never lights the ring.
                    header_focused_focus.set(gained);
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        // Optional drag source on the header (e.g. a dock panel's drag handle).
        // The whole header drags — EXCEPT the trailing slot, which is wrapped in
        // a `DeadZone` so its action buttons / `⋮` menu can be clicked (even with
        // click jitter) without starting the panel drag.
        if let Some(drag) = self.on_header_drag.clone() {
            handler_set = handler_set.on_drag(move |phase, ctx| {
                if let teksilo_core::gesture::DragPhase::Started { .. } = phase {
                    (drag)(ctx);
                }
            });
        }

        ctx.apply_self_handlers(handler_set);

        self.child_ids()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        if self.fill {
            // Fill the allotted slot (the dock pane forces our bounds anyway).
            return proposal
                .resolve(
                    proposal.width.unwrap_or(0.0),
                    proposal.height.unwrap_or(0.0),
                )
                .into();
        }
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return (size).into();
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if self.fill {
            self.place_fill(bounds, children, ctx);
            return;
        }
        for child in children.iter_mut() {
            child.origin = teksilo_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Button);
        // Resolve at walk time — the AT tree re-walks on locale change.
        builder.set_name(self.title.resolve_now());
        builder.set_expanded(self.expanded.get());
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::Focus);
        if let Some(region_id) = self.region_id {
            builder.push_controlled(teksilo_core::accessibility::widget_id_to_node_id(region_id));
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids()
    }

    fn clips_children(&self) -> bool {
        // Fill mode clips so a collapsing body never bleeds past the pane.
        self.fill
    }
}

impl Accordion {
    /// The accordion's children — `[header, body]` in fill mode (laid out by
    /// `place_fill`), else the single VStack/HStack root.
    fn child_ids(&self) -> Vec<WidgetId> {
        if self.fill {
            let mut ids = Vec::with_capacity(2);
            ids.extend(self.fill_header_id);
            ids.extend(self.fill_body_id);
            ids
        } else {
            self.root_child_id.into_iter().collect()
        }
    }

    /// Custom fill-mode layout: the header sits at the top (vertical) or the
    /// leading edge (horizontal); the `FillBody` takes the leftover the
    /// enclosing Splitter pane gives this accordion and clips to it. There is no
    /// internal tween — the Splitter pane folding to the header *is* the
    /// collapse animation.
    fn place_fill(&self, bounds: Rect, children: &mut [WidgetPlacement], ctx: &LayoutContext) {
        const GAP: f32 = 2.0;
        let Some(header_id) = self.fill_header_id else {
            return;
        };
        let horizontal = self.orientation == AccordionOrientation::Horizontal;

        // Header extent along the main axis (height for vertical, width for
        // horizontal); it fills the cross axis.
        let header_size = ctx
            .child_size(
                header_id,
                if horizontal {
                    SizeProposal {
                        width: None,
                        height: Some(bounds.height),
                    }
                } else {
                    SizeProposal {
                        width: Some(bounds.width),
                        height: None,
                    }
                },
            )
            .unwrap_or(Size::ZERO);

        // children order matches `child_ids()`: [header, body?].
        let header_rect = if horizontal {
            Rect::new(bounds.x, bounds.y, header_size.width, bounds.height)
        } else {
            Rect::new(bounds.x, bounds.y, bounds.width, header_size.height)
        };
        if let Some(c) = children.first_mut() {
            c.origin = header_rect.origin();
            c.size = header_rect.size();
        }

        let Some(body_id) = self.fill_body_id else {
            return;
        };
        // Leftover for the body, and the proposal that makes the `FillBody`
        // fill (and clip to) that leftover.
        let (body_origin, body_proposal) = if horizontal {
            let leftover = (bounds.width - header_size.width - GAP).max(0.0);
            (
                teksilo_canvas::Point::new(header_rect.right() + GAP, bounds.y),
                SizeProposal {
                    width: Some(leftover),
                    height: Some(bounds.height),
                },
            )
        } else {
            let leftover = (bounds.height - header_size.height - GAP).max(0.0);
            (
                teksilo_canvas::Point::new(bounds.x, header_rect.bottom() + GAP),
                SizeProposal {
                    width: Some(bounds.width),
                    height: Some(leftover),
                },
            )
        };
        let body_size = ctx.child_size(body_id, body_proposal).unwrap_or(Size::ZERO);
        if let Some(c) = children.get_mut(1) {
            c.origin = body_origin;
            c.size = body_size;
        }
    }
}

// ---------------------------------------------------------------------------
// FillBody — the fill-mode content body of a dock-panel Accordion: it fills
// whatever leftover the accordion gives it (which the enclosing Splitter pane
// animates as the panel collapses / expands) and clips any overflow. Absorbs
// taps/drags so only the accordion header toggles / drags / moves the panel.
// The collapse *animation* is the Splitter pane resizing, not this widget.
// ---------------------------------------------------------------------------

struct FillBody {
    content_id: WidgetId,
}

impl FillBody {
    fn new(content_id: WidgetId) -> Self {
        Self { content_id }
    }
}

impl std::fmt::Debug for FillBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FillBody").finish()
    }
}

impl Widget for FillBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Absorb taps + drags the content's own children didn't handle, so a
        // tap/drag on empty panel body never reaches the accordion header.
        ctx.apply_self_handlers(
            HandlerSet::new()
                .on_tap(|_e, _ctx| {})
                .on_drag(|_phase, _ctx| {}),
        );
        vec![self.content_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Fill the leftover the accordion proposes (bounded on both axes).
        proposal
            .resolve(
                proposal.width.unwrap_or(0.0),
                proposal.height.unwrap_or(0.0),
            )
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

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

    fn children(&self) -> Vec<WidgetId> {
        vec![self.content_id]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::WidgetBuilder;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    #[test]
    fn rich_tooltip_more_label_is_a_translatable_framework_string() {
        // Regression: the rich-tooltip "more" disclosure label used to be
        // a hardcoded `lit!("More")` frozen by `Accordion`'s eager
        // resolve. It must now resolve through the teksilo-widgets
        // framework bundle (`tooltip-more`) and follow the active locale.
        use teksilo_i18n::{I18nConfig, I18nManager};
        let cfg = I18nConfig::new()
            .supported_locales(["en-US".parse().unwrap(), "fr-FR".parse().unwrap()])
            .auto_detect_os_locale(false)
            .framework_locales(crate::framework_locales());
        let mgr = I18nManager::from_config(&cfg);
        assert_eq!(mgr.resolve_widget("tooltip-more", &[]), "More");
        mgr.set_locale("fr-FR".parse().unwrap());
        assert_eq!(
            mgr.resolve_widget("tooltip-more", &[]),
            "Plus",
            "tooltip-more must translate to French via the framework bundle"
        );
    }

    #[test]
    fn accordion_builds_collapsed() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let acc = tree.add(Accordion::new(lit!("Section"), expanded.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let b = tree.bounds(acc);
        assert!(b.width > 0.0);
    }

    #[test]
    fn click_toggles_expanded_state() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let acc = tree.add(Accordion::new(lit!("Section"), expanded.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        tree.click(acc);
        assert!(expanded.get());
        tree.click(acc);
        assert!(!expanded.get());
    }

    #[test]
    fn accordion_with_content() {
        use crate::primitives::TextWidget;
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let content = tree.add(TextWidget::new(lit!("Content text")));
        let acc = tree.add(Accordion::new(lit!("Details"), expanded.clone()).content_id(content));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let b = tree.bounds(acc);
        assert!(b.height > 0.0);
    }

    #[test]
    fn accessibility() {
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let acc = tree.add(Accordion::new(lit!("Details"), expanded));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let info = tree.accessibility_node(acc);
        assert_eq!(info.name(), Some("Details"));
        assert!(info.is_expanded());
    }

    #[test]
    fn access_action_click_toggles_expanded() {
        // A screen-reader "press" / default action must operate the
        // disclosure, not just a pointer tap. The accordion advertises
        // `Action::Click`; dispatching it has to flip `expanded`.
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let acc = tree.add(Accordion::new(lit!("Section"), expanded.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        tree.dispatch_event(WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(acc),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        assert!(expanded.get(), "AT click expands the accordion");

        tree.dispatch_event(WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(acc),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        assert!(!expanded.get(), "a second AT click collapses it");
    }

    #[test]
    fn announced_expanded_state_refreshes_on_external_toggle() {
        // Binding `expanded` to the accordion's own node keeps the announced
        // `aria-expanded` fresh when the state changes from outside the
        // widget — without re-querying accessibility() the AT state goes stale.
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let acc = tree.add(Accordion::new(lit!("Section"), expanded.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        // Realize the AT tree once so the binding is in place.
        let _ = tree.sync_accessibility();
        assert!(!tree.accessibility_node(acc).is_expanded());

        expanded.set(true);
        let _ = tree.sync_accessibility();
        assert!(
            tree.accessibility_node(acc).is_expanded(),
            "announced expanded state must follow an external set"
        );
    }

    #[test]
    fn external_signal_set_triggers_animation() {
        // Simulates an external mutation: app code sets `expanded` to
        // true without going through the accordion's tap handler. The
        // `Collapse` observer should still kick off the height tween.
        use crate::primitives::TextWidget;
        use std::time::Duration;

        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let content = tree.add(TextWidget::new(lit!("Some content")));
        let acc = tree.add(Accordion::new(lit!("Section"), expanded.clone()).content_id(content));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed = tree.bounds(acc).height;

        expanded.set(true);
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let after = tree.bounds(acc).height;

        assert!(
            after > collapsed,
            "external set must drive expansion: {} > {}",
            after,
            collapsed
        );
    }

    #[test]
    fn double_toggle_round_trips_height() {
        use crate::primitives::TextWidget;
        use std::time::Duration;

        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let content = tree.add(TextWidget::new(lit!("Some content")));
        let acc = tree.add(Accordion::new(lit!("Section"), expanded.clone()).content_id(content));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed_initial = tree.bounds(acc).height;

        // Expand then collapse.
        tree.click(acc);
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let expanded_h = tree.bounds(acc).height;

        tree.click(acc);
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed_again = tree.bounds(acc).height;

        assert!(expanded_h > collapsed_initial);
        assert!(
            (collapsed_again - collapsed_initial).abs() < 1.0,
            "after collapse round-trip, height should match initial: {} vs {}",
            collapsed_again,
            collapsed_initial
        );
    }

    #[test]
    fn content_dormant_when_collapsed() {
        use crate::primitives::TextWidget;
        use std::time::Duration;

        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let content = tree.add(TextWidget::new(lit!("Some content text here")));
        let acc = tree.add(Accordion::new(lit!("Section"), expanded.clone()).content_id(content));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed_height = tree.bounds(acc).height;

        // Click to expand
        tree.click(acc);
        assert!(expanded.get(), "should be expanded after click");

        // Tick animation to completion (accordion uses 200ms animation)
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let expanded_height = tree.bounds(acc).height;

        assert!(
            expanded_height > collapsed_height,
            "expanded height ({}) should be greater than collapsed height ({})",
            expanded_height,
            collapsed_height
        );
    }

    // ─── fill / drag / orientation (dock panel features) ────────────────

    #[test]
    fn fill_accordion_header_toggles_but_content_tap_does_not() {
        use crate::primitives::TextWidget;
        use teksilo_core::event::{Modifiers, PointerButton};

        fn tap_at(tree: &mut WidgetTree, p: teksilo_canvas::Point) {
            tree.dispatch_event(WidgetEvent::PointerDown {
                position: p,
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            });
            tree.dispatch_event(WidgetEvent::PointerUp {
                position: p,
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            });
        }

        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let content = tree.add(TextWidget::new(lit!("dock body")));
        let acc = tree.add(
            Accordion::new(lit!("Panel"), expanded.clone())
                .fill(true)
                .content_id(content),
        );
        tree.layout(SizeProposal::exact(220.0, 300.0));

        // A tap on the header (top of the accordion) toggles.
        let b = tree.bounds(acc);
        tap_at(&mut tree, teksilo_canvas::Point::new(b.x + 20.0, b.y + 6.0));
        assert!(!expanded.get(), "header tap collapses");
        tap_at(&mut tree, teksilo_canvas::Point::new(b.x + 20.0, b.y + 6.0));
        assert!(expanded.get(), "header tap re-expands");

        // A tap deep in the content area is absorbed — it must NOT toggle.
        tap_at(
            &mut tree,
            teksilo_canvas::Point::new(b.x + 110.0, b.y + 200.0),
        );
        assert!(expanded.get(), "content tap does not collapse the panel");
    }

    #[test]
    fn fill_accordion_header_drag_fires_hook() {
        use crate::primitives::TextWidget;
        use std::cell::Cell as StdCell;
        use std::rc::Rc;
        let dragged = Rc::new(StdCell::new(false));
        let sink = dragged.clone();
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let content = tree.add(TextWidget::new(lit!("dock body")));
        let acc = tree.add(
            Accordion::new(lit!("Panel"), expanded)
                .fill(true)
                .on_header_drag(move |_ctx| sink.set(true))
                .content_id(content),
        );
        tree.layout(SizeProposal::exact(220.0, 300.0));
        let b = tree.bounds(acc);
        let from = teksilo_canvas::Point::new(b.x + 20.0, b.y + 6.0);
        tree.drag(
            from,
            teksilo_canvas::Point::new(from.x + 130.0, from.y + 30.0),
        );
        assert!(dragged.get(), "dragging the header fires on_header_drag");
    }

    #[test]
    fn fill_accordion_body_fills_the_leftover() {
        use crate::primitives::TextWidget;
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let content = tree.add(TextWidget::new(lit!("dock body")));
        let acc = tree.add(
            Accordion::new(lit!("Panel"), expanded)
                .fill(true)
                .content_id(content),
        );
        tree.layout(SizeProposal::exact(220.0, 300.0));
        // children = [header, body]; the body fills the leftover after the
        // header, so header + body ≈ the pane. (The collapse animation is the
        // enclosing Splitter pane resizing — verified in the splitter tests.)
        let header_h = tree.bounds(tree.children(acc)[0]).height;
        let body_h = tree.bounds(tree.children(acc)[1]).height;
        assert!(
            (header_h + body_h - 300.0).abs() < 6.0,
            "header({header_h}) + body({body_h}) should fill the 300px pane"
        );
        assert!(body_h > 200.0, "body fills most of the pane, got {body_h}");
    }

    #[test]
    fn fill_accordion_body_stays_within_the_pane() {
        use crate::primitives::{FixedSize, TextWidget};
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        // Content far taller than the pane.
        let content = tree.add(
            FixedSize::new()
                .width(80.0_f32)
                .height(900.0_f32)
                .child(TextWidget::new(lit!("x"))),
        );
        let acc = tree.add(
            Accordion::new(lit!("Panel"), expanded)
                .fill(true)
                .content_id(content),
        );
        tree.layout(SizeProposal::exact(220.0, 300.0));
        // The collapse body never extends past the pane bottom (the oversized
        // content is clipped, not spilled).
        let body = tree.children(acc)[1];
        assert!(
            tree.bounds(body).bottom() <= 300.5,
            "body bottom {} must stay within the 300px pane",
            tree.bounds(body).bottom()
        );
    }

    #[test]
    fn trailing_slot_renders_and_captures_its_own_tap() {
        // The header trailing slot (e.g. an options `⋮` button) must be laid out
        // in the header AND, when it carries its own tap handler, consume the
        // tap so the accordion does not toggle (the gesture arena gives the
        // innermost hit precedence — the same property ToolBox slots rely on).
        use crate::primitives::{FixedSize, RectWidget};
        use std::cell::Cell as StdCell;
        use std::rc::Rc;
        use teksilo_core::event::{Modifiers, PointerButton};

        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let tapped = Rc::new(StdCell::new(false));
        let sink = tapped.clone();
        let trailing = tree.add(
            FixedSize::new()
                .width(24.0_f32)
                .height(24.0_f32)
                .child(RectWidget::new())
                .on_tap(move |_e, _ctx| sink.set(true)),
        );
        let content = tree.add(TextWidget::new(lit!("body")));
        let acc = tree.add(
            Accordion::new(lit!("Panel"), expanded.clone())
                .fill(true)
                .trailing_id(trailing)
                .content_id(content),
        );
        tree.layout(SizeProposal::exact(260.0, 140.0));

        // The trailing widget is laid out (non-zero bounds) inside the header.
        let tb = tree.bounds(trailing);
        assert!(tb.width > 0.0 && tb.height > 0.0, "trailing slot is placed");
        let _ = acc;

        // Tapping the trailing widget fires its handler and does NOT toggle.
        let p = teksilo_canvas::Point::new(tb.x + tb.width / 2.0, tb.y + tb.height / 2.0);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: p,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: p,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert!(tapped.get(), "trailing widget received the tap");
        assert!(
            expanded.get(),
            "tapping the trailing widget must not toggle the accordion"
        );
    }

    #[test]
    fn dragging_the_trailing_slot_does_not_start_the_header_drag() {
        // Regression: a draggable header (`on_header_drag`) must NOT be dragged
        // by interacting with its trailing controls — clicking/dragging an
        // options button there used to arm the header-drag recognizer and a few
        // px of jitter started dragging the whole dock. The trailing absorber
        // shadows the header drag.
        use crate::primitives::{FixedSize, RectWidget};
        use std::cell::Cell as StdCell;
        use std::rc::Rc;

        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let header_dragged = Rc::new(StdCell::new(false));
        let hd = header_dragged.clone();
        let trailing = tree.add(
            FixedSize::new()
                .width(24.0_f32)
                .height(24.0_f32)
                .child(RectWidget::new()),
        );
        let content = tree.add(TextWidget::new(lit!("body")));
        let acc = tree.add(
            Accordion::new(lit!("Panel"), expanded.clone())
                .fill(true)
                .trailing_id(trailing)
                .on_header_drag(move |_ctx| hd.set(true))
                .content_id(content),
        );
        tree.layout(SizeProposal::exact(260.0, 140.0));

        // Dragging from the trailing control must NOT start the header drag.
        let tb = tree.bounds(trailing);
        let from = teksilo_canvas::Point::new(tb.x + tb.width / 2.0, tb.y + tb.height / 2.0);
        tree.drag(
            from,
            teksilo_canvas::Point::new(from.x + 90.0, from.y + 12.0),
        );
        assert!(
            !header_dragged.get(),
            "dragging the trailing control must not start the header drag"
        );

        // Sanity: dragging the header title area still starts the drag.
        let ab = tree.bounds(acc);
        tree.drag(
            teksilo_canvas::Point::new(ab.x + 10.0, ab.y + 6.0),
            teksilo_canvas::Point::new(ab.x + 120.0, ab.y + 30.0),
        );
        assert!(
            header_dragged.get(),
            "dragging the header title still starts the drag"
        );
    }

    #[test]
    fn incremental_move_on_trailing_button_does_not_drag_header() {
        // The real-mouse case the user hit: pressing a header action button and
        // moving the pointer a few px (a normal click jitter) must NOT start the
        // header drag — even though the button's tap is cancelled by the move
        // and its capture is released mid-gesture.
        use crate::primitives::{FixedSize, RectWidget};
        use std::cell::Cell as StdCell;
        use std::rc::Rc;
        use teksilo_core::event::PointerButton;

        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let header_dragged = Rc::new(StdCell::new(false));
        let hd = header_dragged.clone();
        // A real interactive trailing control (captures the tap, like a button).
        let trailing = tree.add(
            FixedSize::new()
                .width(24.0_f32)
                .height(24.0_f32)
                .child(RectWidget::new())
                .on_tap(|_e, _ctx| {}),
        );
        let content = tree.add(TextWidget::new(lit!("body")));
        let _acc = tree.add(
            Accordion::new(lit!("Panel"), expanded.clone())
                .fill(true)
                .trailing_id(trailing)
                .on_header_drag(move |_ctx| hd.set(true))
                .content_id(content),
        );
        tree.layout(SizeProposal::exact(260.0, 140.0));

        let tb = tree.bounds(trailing);
        let (cx, cy) = (tb.x + tb.width / 2.0, tb.y + tb.height / 2.0);
        tree.pointer_down_button(teksilo_canvas::Point::new(cx, cy), PointerButton::Primary);
        // Incremental small moves (a real mouse stream), accumulating well past
        // the drag threshold.
        for i in 1..=10 {
            tree.pointer_move(teksilo_canvas::Point::new(cx + (i as f32) * 3.0, cy + 1.0));
        }
        tree.pointer_up_button(
            teksilo_canvas::Point::new(cx + 30.0, cy + 1.0),
            PointerButton::Primary,
        );
        assert!(
            !header_dragged.get(),
            "a jittery click on the trailing control must not start the header drag"
        );
    }

    #[test]
    fn horizontal_fill_accordion_builds() {
        use crate::primitives::TextWidget;
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let content = tree.add(TextWidget::new(lit!("c")));
        let acc = tree.add(
            Accordion::new(lit!("Panel"), expanded)
                .horizontal()
                .fill(true)
                .content_id(content),
        );
        tree.layout(SizeProposal::exact(320.0, 120.0));
        let b = tree.bounds(acc);
        assert!(
            b.width > 0.0 && b.height > 0.0,
            "horizontal accordion builds"
        );
    }
}
