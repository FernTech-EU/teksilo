// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drag & Drop tab — DropZone (standalone target) + DropTarget
//! (wrapping container). Cannibalized from the `file-drop` example.
//! OS file drops are live once `install_external_dnd()` is wired (it
//! is, in main.rs). On X11 the keyboard Browse fallback is the path.

use bastyde::prelude::*;
use bastyde::widgets::{
    Divider, DropTarget, DropTargetVariant, DropZone, FixedSize, Padding, Panel, TextWidget, VStack,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_dragdrop_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_dragdrop_refs())
}

/// Prepend a line to the log signal (newest first).
fn prepend(log: &Signal<String>, line: String) {
    let current = log.get();
    log.set(format!("{line}\n{current}"));
}

fn images_zone(log: Signal<String>) -> FixedSize {
    FixedSize::new()
        .bind_width(360.0_f32)
        .bind_height(120.0_f32)
        .child(
            DropZone::new(tr!(dnd_zone_images_title()))
                .subtitle(tr!(dnd_zone_images_subtitle()))
                .accept_extensions(["png", "jpg", "jpeg", "gif"])
                .on_files_dropped(move |paths, _ctx| {
                    for p in &paths {
                        prepend(&log, format!("🖼  {}", p.display()));
                    }
                }),
        )
}

fn any_zone(log: Signal<String>) -> FixedSize {
    let files_log = log.clone();
    let text_log = log.clone();
    let urls_log = log;
    FixedSize::new()
        .bind_width(360.0_f32)
        .bind_height(120.0_f32)
        .child(
            DropZone::new(tr!(dnd_zone_any_title()))
                .subtitle(tr!(dnd_zone_any_subtitle()))
                .on_files_dropped(move |paths, _ctx| {
                    for p in &paths {
                        prepend(&files_log, format!("📄  {}", p.display()));
                    }
                })
                .on_text_dropped(move |text, _ctx| {
                    prepend(&text_log, format!("📝  {text}"));
                })
                .on_urls_dropped(move |urls, _ctx| {
                    for u in &urls {
                        prepend(&urls_log, format!("🔗  {u}"));
                    }
                }),
        )
}

/// A DropTarget wraps an ordinary Panel — the child stays visible and
/// the highlight is a border, not a fill. Accepts external file drops.
fn wrapping_target(log: Signal<String>) -> DropTarget {
    DropTarget::new()
        .accept_external_files()
        .variant(DropTargetVariant::Prominent)
        .child(
            Panel::new()
                .background(SurfaceRole::Raised)
                .corner_radius(8.0)
                .child(Padding::uniform(16.0).child(TextWidget::new(tr!(dnd_target_body())))),
        )
        .hint(TextWidget::new(tr!(dnd_target_hint())))
        .on_drop(move |payload, _pos, _ctx| {
            for p in payload.files() {
                prepend(&log, format!("✅  {}", p.display()));
            }
            true
        })
}

fn log_panel(log: Signal<String>) -> Panel {
    Panel::new()
        .background(SurfaceRole::Sunken)
        .corner_radius(8.0)
        .child(
            Padding::uniform(12.0).child(
                TextWidget::new(lit!(String::new()))
                    .style(TextStyleRole::Mono)
                    .bind_text(log),
            ),
        )
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let log = ctx.signal(tr!(dnd_log_initial()).resolve_now());
    let header = tab_header(ctx, title(), refs());
    let any = section(ctx, tr!(dnd_section_zone_any()), any_zone(log.clone()));
    let images = section(
        ctx,
        tr!(dnd_section_zone_images()),
        images_zone(log.clone()),
    );
    let target = section(ctx, tr!(dnd_section_target()), wrapping_target(log.clone()));
    let logs = section(ctx, tr!(dnd_section_log()), log_panel(log));

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(any)
            .add_child(images)
            .add_child(target)
            .add_child(logs),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // DropZone / DropTarget carry drop callbacks (closures) that bati!
    // property syntax can't express — pre-build and splice via `#{ id }`.
    let log = ctx.signal(tr!(dnd_log_initial()).resolve_now());
    let any_id = ctx.add(any_zone(log.clone()));
    let images_id = ctx.add(images_zone(log.clone()));
    let target_id = ctx.add(wrapping_target(log.clone()));
    let log_id = ctx.add(log_panel(log));

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_dragdrop_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_dragdrop_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dnd_section_zone_any())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ any_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dnd_section_zone_images())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ images_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dnd_section_target())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ target_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dnd_section_log())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ log_id }
            }
        }
    )
}
