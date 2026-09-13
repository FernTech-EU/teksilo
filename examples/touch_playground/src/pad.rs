// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pointer pad, and the readout beside it.
//!
//! # Why the inspector is a *pad* and not the whole window
//!
//! Nothing a widget can reach reports the tree's live pointers. `LayoutContext`
//! carries focus, shortcuts and overlays; an `EventContext` describes only the
//! sample being dispatched; `WidgetTree::hover_owner` and the pointer table
//! behind it are the tree's, and a widget has no handle on the tree. So a live
//! pointer inspector can only report **the pointers dispatched to it** — which
//! means it has to be a surface the user touches on purpose.
//!
//! The debug inspector's Pointers tab reaches the same conclusion and pays for
//! it with a full-window probe that has to be armed, because while it is up the
//! application beneath it receives nothing. A playground can do better: give
//! the probe its own rectangle, and the rest of the window stays usable.
//!
//! Two consequences to keep in mind while reading a session:
//!
//! * the `hover owner` row is the pad's own answer — the most recent
//!   hovering-capable pointer (a mouse, or a pen in proximity) that delivered a
//!   sample **to the pad**. The tree elects a hover owner across the whole
//!   window; these agree whenever the pointer is over the pad and need not
//!   otherwise.
//! * a contact that never touches the pad never appears. Put both fingers on
//!   the pad to see a two-contact session.
//!
//! The pad declares `TouchAction::NONE`: nothing the framework does by default
//! may run on a surface whose job is to report what the framework was handed.
//! That one is load-bearing — delete it and
//! `the_pad_reports_a_contact_with_its_frozen_action_and_no_pan` goes red, on the
//! frozen action it asserts.
//!
//! It declares **no** `MultiContact` policy, and that is a measurement rather
//! than an omission. `MultiContact::All` was written here first and then deleted
//! after `two_fingers_on_the_pad_are_both_reported` stayed green without it: a
//! handler that answers `EventResponse::Ignored` never becomes the arena owner,
//! so there is no live arena for the default `First` to refuse a second contact
//! from. A declaration no test can hold is a claim, not a mechanism — the same
//! conclusion the debug inspector's own watch overlay reached about its copy of
//! it. The behaviour is pinned by that test instead, which is what goes red if
//! the pad is ever made to answer `Handled`.

use std::cell::RefCell;
use std::collections::BTreeMap;

use teksilo::canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo::core::accessibility::AccessNodeBuilder;
use teksilo::core::binding::BindingLevel;
use teksilo::core::build_context::BuildContext;
use teksilo::core::event::{EventResponse, WidgetEvent};
use teksilo::core::kinetic::VelocityTracker;
use teksilo::core::pointer::touch_action::TouchAction;
use teksilo::core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo::core::widget_builder::HandlerSet;
use teksilo::core::widget_id::WidgetId;
use teksilo::tokens::CornerRadius;
use teksilo::tokens::{Color, PointerKind, SurfaceRole, TextRole};

use crate::state::{ALL_SCENARIOS, PlaygroundState, PointerRecord};

/// Human-readable flag list for a [`TouchAction`]. The derived `Debug` prints a
/// bit pattern, which is no use in a readout.
pub fn format_touch_action(action: TouchAction) -> String {
    if action == TouchAction::AUTO {
        return "AUTO".to_string();
    }
    if action.is_none() {
        return "NONE".to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    if action.allows_pan_x() {
        parts.push("PAN_X");
    }
    if action.allows_pan_y() {
        parts.push("PAN_Y");
    }
    if action.allows_pinch() {
        parts.push("PINCH_ZOOM");
    }
    parts.join("|")
}

/// A pointer kind as one short word.
pub fn format_kind(kind: PointerKind) -> String {
    match kind {
        PointerKind::Mouse => "mouse".to_string(),
        PointerKind::Touch => "touch".to_string(),
        PointerKind::Pen(tool) => format!("pen:{tool:?}").to_ascii_lowercase(),
        PointerKind::Unknown => "unknown".to_string(),
        // `PointerKind` is `#[non_exhaustive]`: a kind added later reads as
        // its `Debug` rather than silently as "unknown".
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

/// The touch surface that reports every pointer it is handed.
pub struct PointerPad {
    state: PlaygroundState,
}

impl PointerPad {
    pub fn new(state: PlaygroundState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for PointerPad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerPad").finish()
    }
}

impl Widget for PointerPad {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.clone();
        let self_id = ctx.self_id();
        state
            .pointers
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);

        // One velocity tracker per live pointer. The framework runs its own
        // tracker behind a pan, but that one belongs to the pan session and no
        // handler can reach it — so the pad keeps its own, fed from the same
        // samples. The number it shows is therefore *the pad's* estimate of the
        // pointer's speed, not the value a fling was seeded with.
        //
        // Behind an `Rc` because two handler closures share it and a handler
        // cannot borrow `self`. Rebuilt here on purpose: a rebuild destroys the
        // node's pointer state anyway.
        let trackers = std::rc::Rc::new(RefCell::new(BTreeMap::<u64, VelocityTracker>::new()));

        let cancel_state = state.clone();
        let handlers = HandlerSet::new()
            .focusable(false)
            // Nothing the framework runs by default may act on this surface: a
            // pan would move something, and a hold would open a menu over the
            // very sample the pad exists to report.
            .touch_action(TouchAction::NONE)
            .on_pointer_event({
                let trackers = trackers.clone();
                let state = state.clone();
                move |event, ctx| {
                    let pointer = ctx.pointer();
                    let id = pointer.id.get();
                    let position = match event {
                        WidgetEvent::PointerDown { position, .. }
                        | WidgetEvent::PointerUp { position, .. }
                        | WidgetEvent::PointerMove { position, .. } => Some(*position),
                        // `PointerEnter` / `PointerLeave` carry the pointer and
                        // no position: a boundary crossing is not a sample.
                        _ => None,
                    };
                    let ends = matches!(event, WidgetEvent::PointerUp { .. });

                    if pointer.kind.hovers() {
                        // The pad's own hover owner. The tree elects one across
                        // the whole window (`WidgetTree::hover_owner`) and no
                        // widget can ask it, so this is the observable half:
                        // the last hovering-capable pointer to reach the pad.
                        state.hover_owner.set(Some((id, pointer.kind)));
                    }

                    let mut speed = 0.0;
                    if let Some(p) = position {
                        let mut map = trackers.borrow_mut();
                        let tracker = map.entry(id).or_default();
                        tracker.add(pointer.time, p);
                        let v = tracker.velocity();
                        speed = (v.x * v.x + v.y * v.y).sqrt();
                    }

                    let mut rows = state.pointers.get();
                    rows.retain(|r| r.id != id);
                    if ends && !pointer.kind.hovers() {
                        // A lifted contact ceases to exist. A mouse and a pen in
                        // proximity stay, hovering — the pointer table's own rule.
                        trackers.borrow_mut().remove(&id);
                        state.log(format!("up   #{id} {}", format_kind(pointer.kind)));
                    } else {
                        if matches!(event, WidgetEvent::PointerDown { .. }) {
                            state.log(format!(
                                "down #{id} {} action={}",
                                format_kind(pointer.kind),
                                format_touch_action(ctx.touch_action()),
                            ));
                        }
                        rows.push(PointerRecord {
                            id,
                            kind: pointer.kind,
                            primary: pointer.primary,
                            down: !pointer.buttons.is_empty(),
                            position: position.unwrap_or(Point::ZERO),
                            pressure: pointer.axes.pressure,
                            tilt: pointer.axes.tilt,
                            twist: pointer.axes.twist,
                            contact: pointer.axes.contact.map(|s| (s.width, s.height)),
                            speed,
                            touch_action: format_touch_action(ctx.touch_action()),
                            pressed: ctx.press_is_inside(),
                            owns: ctx.owns_pointer(),
                        });
                        rows.sort_by_key(|r| r.id);
                    }
                    state.pointers.set(rows);
                    // `Ignored`, deliberately. The pad reports; it does not
                    // consume. Nothing is above it that wants the sample, and a
                    // reporting surface that answered `Handled` would be
                    // claiming a press it does not act on.
                    EventResponse::Ignored
                }
            })
            .on_pointer_cancel({
                let trackers = trackers.clone();
                move |pointer, reason, _ctx| {
                    let id = pointer.id.get();
                    trackers.borrow_mut().remove(&id);
                    let mut rows = cancel_state.pointers.get();
                    rows.retain(|r| r.id != id);
                    cancel_state.pointers.set(rows);
                    cancel_state.log(format!("cancel #{id} {reason:?}"));
                }
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Fills what it is given on a bounded axis and asks for a readable
        // minimum on an unbounded one.
        let w = proposal.width.unwrap_or(320.0).max(240.0);
        let h = proposal.height.unwrap_or(200.0).max(160.0);
        Size::new(w, h).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        canvas.fill_rounded_rect(
            bounds,
            CornerRadius::uniform(theme.shape.radius_popup),
            SurfaceRole::Raised.resolve(&theme.colors),
        );

        let rows = self.state.pointers.get();
        if rows.is_empty() {
            canvas.draw_text(
                "Touch, click or hover here",
                Rect::new(bounds.x + 12.0, bounds.y + 12.0, bounds.width - 24.0, 20.0),
                &theme.typography.body,
                TextRole::Secondary.resolve(&theme.colors),
            );
            return;
        }

        // One disc per live pointer, sized to the contact patch where the
        // digitizer reports one, plus a line along its velocity so a fling's
        // direction is visible at the moment of release.
        for row in &rows {
            let radius = row
                .contact
                .map(|(w, h)| (w.max(h) / 2.0).clamp(10.0, 48.0))
                .unwrap_or(14.0);
            let colour = disc_colour(row.kind, row.down);
            let centre = Point::new(bounds.x + row.position.x, bounds.y + row.position.y);
            canvas.fill_rounded_rect(
                Rect::new(
                    centre.x - radius,
                    centre.y - radius,
                    radius * 2.0,
                    radius * 2.0,
                ),
                CornerRadius::uniform(radius),
                colour,
            );
            canvas.draw_text(
                &format!("#{}", row.id),
                Rect::new(centre.x + radius + 4.0, centre.y - 8.0, 80.0, 16.0),
                &theme.typography.mono,
                TextRole::Primary.resolve(&theme.colors),
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The pad is where the pointers are reported, and a screen-reader user
        // has to be told it is a surface rather than a decoration. The numbers
        // live on the readout beside it, which names them.
        builder.set_role(teksilo::core::accesskit::Role::Group);
        builder
            .set_name("Pointer pad — every pointer that touches this area is reported beside it");
    }
}

/// A live pointer's disc colour: kind by hue, pressed by opacity.
fn disc_colour(kind: PointerKind, down: bool) -> Color {
    let alpha = if down { 0.55 } else { 0.22 };
    match kind {
        PointerKind::Touch => Color::from_rgba(0.0, 0.55, 0.95, alpha),
        PointerKind::Pen(_) => Color::from_rgba(0.85, 0.25, 0.65, alpha),
        PointerKind::Mouse => Color::from_rgba(0.35, 0.75, 0.3, alpha),
        PointerKind::Unknown => Color::from_rgba(0.6, 0.6, 0.6, alpha),
        _ => Color::from_rgba(0.6, 0.6, 0.6, alpha),
    }
}

/// The text readout: the token ladder in force, then one block per live
/// pointer, then the event log.
///
/// Painted text with one `Role::Label` carrying the whole thing, which is the
/// house convention for a panel that paints its own lines (`TextWidget` does the
/// same, and the debug inspector's tabs do it for the same reason): without it
/// the panel is a blank rectangle to a screen reader.
pub struct PointerReadout {
    state: PlaygroundState,
    lines: RefCell<Vec<String>>,
}

/// One painted line's height. Read-only text, so it is not a target and does
/// not follow the density ladder.
const LINE_HEIGHT: f32 = 15.0;

impl PointerReadout {
    pub fn new(state: PlaygroundState) -> Self {
        Self {
            state,
            lines: RefCell::new(Vec::new()),
        }
    }

    /// The whole readout, as text. Built in `layout_response` and consumed by
    /// both `paint` and `accessibility`, so the panel reads the same to a screen
    /// reader as it looks.
    fn build_lines(&self, ctx: &LayoutContext) -> Vec<String> {
        let tokens = &ctx.theme.input;
        let touch = tokens.profile(PointerKind::Touch);
        let pen = tokens.profile(PointerKind::Pen(teksilo::tokens::PenKind::Pen));
        let mouse = tokens.profile(PointerKind::Mouse);
        let mut lines = vec![
            format!(
                "density {:?}   target {} dp   grab {} dp   slop budget {} dp   spacing x{:.2}   touch {}",
                tokens.density,
                tokens.target_size,
                tokens.grab_size,
                tokens.slop_budget,
                tokens.spacing_factor,
                if tokens.touch_enabled { "on" } else { "off" },
            ),
            format!(
                "reveal {:?}   lines/notch {}   physics {:?}",
                tokens.reveal, tokens.lines_per_notch, tokens.scroll_physics.physics,
            ),
            String::new(),
            "profile        tap   drag    pan   hit   hold    drag-activation".to_string(),
        ];
        for (name, profile) in [("mouse", mouse), ("touch", touch), ("pen", pen)] {
            lines.push(format!(
                "{name:<13}{:>5} {:>6} {:>6} {:>5} {:>6}ms  {:?}",
                profile.tap_slop,
                profile.drag_slop,
                profile
                    .pan_slop
                    .map(|v| format!("{v}"))
                    .unwrap_or_else(|| "—".to_string()),
                profile.hit_slop,
                profile.long_press.as_millis(),
                profile.drag_activation,
            ));
        }

        lines.push(String::new());
        lines.push(match self.state.hover_owner.get() {
            Some((id, kind)) => format!("hover owner (pad's own): #{id} {}", format_kind(kind)),
            None => "hover owner (pad's own): none — a contact never hovers".to_string(),
        });
        let rows = self.state.pointers.get();
        lines.push(format!("live on the pad: {}", rows.len()));
        if rows.is_empty() {
            lines.push("  (nothing yet)".to_string());
        }
        for row in &rows {
            lines.push(format!(
                "  #{} {} {} {}   at ({:.0},{:.0})   {:.0} dp/s",
                row.id,
                format_kind(row.kind),
                if row.down { "down" } else { "hover" },
                if row.primary { "primary" } else { "-" },
                row.position.x,
                row.position.y,
                row.speed,
            ));
            lines.push(format!(
                "     frozen {}   press {}   capture {}",
                row.touch_action,
                if row.pressed { "held" } else { "-" },
                if row.owns { "mine" } else { "-" },
            ));
            let mut axes: Vec<String> = Vec::new();
            if let Some(p) = row.pressure {
                axes.push(format!("pressure {p:.2}"));
            }
            if let Some((tx, ty)) = row.tilt {
                axes.push(format!("tilt {tx:.0}/{ty:.0}deg"));
            }
            if let Some(t) = row.twist {
                axes.push(format!("twist {t:.0}deg"));
            }
            if let Some((w, h)) = row.contact {
                axes.push(format!("contact {w:.0}x{h:.0}"));
            }
            lines.push(if axes.is_empty() {
                "     axes: none reported".to_string()
            } else {
                format!("     {}", axes.join("   "))
            });
        }

        // Every scenario's verdict in one block, so a tester following
        // `docs/touch-verification.md` reads the whole column's state without
        // scrolling to each panel. The same strings appear beside their own
        // scenarios; `PlaygroundState::outcome` is the single source.
        lines.push(String::new());
        lines.push("scenario verdicts:".to_string());
        for scenario in ALL_SCENARIOS {
            lines.push(format!("  {scenario}: {}", self.state.outcome(scenario)));
        }

        lines.push(String::new());
        lines.push("log (newest last):".to_string());
        for line in self.state.events.get() {
            lines.push(format!("  {line}"));
        }
        lines
    }
}

impl std::fmt::Debug for PointerReadout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerReadout").finish()
    }
}

impl Widget for PointerReadout {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Relayout, not RepaintOnly: the readout's height is a line count, so a
        // pointer arriving changes how much room it needs.
        self.state
            .pointers
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        self.state
            .events
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        self.state
            .hover_owner
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        self.state
            .outcomes
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let lines = self.build_lines(ctx);
        let height = (lines.len().max(1) as f32) * LINE_HEIGHT;
        *self.lines.borrow_mut() = lines;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let colour = TextRole::Primary.resolve(&theme.colors);
        for (i, line) in self.lines.borrow().iter().enumerate() {
            let y = bounds.y + (i as f32) * LINE_HEIGHT;
            canvas.draw_text(
                line,
                Rect::new(bounds.x, y, bounds.width, LINE_HEIGHT),
                &theme.typography.mono,
                colour,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo::core::accesskit::Role::Label);
        builder.set_name(self.lines.borrow().join("\n"));
    }
}
