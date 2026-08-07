// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Indicators tab — read-only status: ProgressBar, Spinner, Link, Badge, Avatar.
//!
//! `classic()` constructs every widget with imperative builder calls
//! (`Type::new().method(...)`). `teksu()` constructs the *same* widget
//! tree via the `teksu!` macro DSL so the toggle visibly proves the two
//! authoring styles produce identical output.

use teksilo::prelude::*;
use teksilo::tokens::Orientation;
use teksilo::widgets::{
    Avatar, AvatarPresence, AvatarShape, AvatarSize, Badge, Divider, FixedSize, Link, ProgressBar,
    Spinner, TextWidget, VStack, Wrap,
};

use crate::shared::{Signals, demo_row, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_indicators_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_indicators_refs())
}

// ── classic builder version ────────────────────────────────────────────
pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());

    let progress_h = section(
        ctx,
        tr!(ind_progress_determinate_heading()),
        ProgressBar::new(0.6).label(tr!(ind_progress_determinate_label())),
    );
    let progress_indet = section(
        ctx,
        tr!(ind_progress_indeterminate_heading()),
        ProgressBar::indeterminate(),
    );
    let progress_v = section(
        ctx,
        tr!(ind_progress_vertical_heading()),
        FixedSize::new().height(120.0_f32).child(
            ProgressBar::new(0.4)
                .orientation(Orientation::Vertical)
                .thickness(8.0),
        ),
    );
    let spinner = section(
        ctx,
        lit!("Spinner"),
        demo_row(16.0)
            .child(Spinner::new(20.0))
            .child(Spinner::new(28.0))
            .child(Spinner::new(36.0).label(tr!(demo_loading()))),
    );
    let link = section(
        ctx,
        lit!("Link"),
        VStack::new()
            .spacing(6.0)
            .child(Link::new(tr!(ind_link_docs())).url("https://example.com"))
            .child(Link::new(tr!(ind_link_handler())).on_activate_fn(|_| {
                println!("link clicked");
            })),
    );
    let badge = section(
        ctx,
        lit!("Badge"),
        demo_row(8.0)
            .child(Badge::new(lit!("New")))
            .child(Badge::new(lit!("Beta")))
            .child(Badge::new(lit!("Stable")).background(SurfaceRole::AccentSubtle))
            .child(Badge::new(lit!("3")))
            .child(Badge::new(lit!("99+"))),
    );
    let avatar = section(
        ctx,
        lit!("Avatar"),
        demo_row(12.0)
            .child(Avatar::with_initials(lit!("CJ")).size(AvatarSize::Medium))
            .child(
                Avatar::with_initials(lit!("AB"))
                    .shape(AvatarShape::RoundedSquare)
                    .seed("alice")
                    .presence(AvatarPresence::Online),
            )
            .child(
                Avatar::with_initials(lit!("MN"))
                    .size(AvatarSize::Large)
                    .seed("mallory")
                    .presence(AvatarPresence::Busy),
            ),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(progress_h)
            .add_child(progress_indet)
            .add_child(progress_v)
            .add_child(spinner)
            .add_child(link)
            .add_child(badge)
            .add_child(avatar),
    )
}

// ── teksu! DSL version ──────────────────────────────────────────────────
//
// Same widget tree as `classic`, expressed in teksu! syntax. The block
// shape is `Type::ctor(args) { method: value … bare_child … }` per the
// teksu! reference. Children of a stack are written as bare expressions
// (UpperCamel calls) or via the `child:` property for free functions
// returning Widget.
pub fn teksu(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    teksu!(ctx => VStack {
            spacing: 20.0

            // header (title + refs)
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_indicators_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_indicators_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            // ProgressBar — determinate
            VStack {
                spacing: 6.0
                TextWidget::new(tr!(ind_progress_determinate_heading())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                ProgressBar::new(0.6) {
                    label: tr!(ind_progress_determinate_label())
                }
            }

            // ProgressBar — indeterminate
            VStack {
                spacing: 6.0
                TextWidget::new(tr!(ind_progress_indeterminate_heading())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                ProgressBar::indeterminate()
            }

            // ProgressBar — vertical
            VStack {
                spacing: 6.0
                TextWidget::new(tr!(ind_progress_vertical_heading())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    height: 120.0_f32
                    ProgressBar::new(0.4) {
                        orientation: Orientation::Vertical
                        thickness: 8.0
                    }
                }
            }

            // Spinner
            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Spinner")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 16.0
                    line_spacing: 16.0
                    Spinner::new(20.0)
                    Spinner::new(28.0)
                    Spinner::new(36.0) {
                        label: tr!(demo_loading())
                    }
                }
            }

            // Link
            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Link")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    Link::new(tr!(ind_link_docs())) {
                        url: "https://example.com"
                    }
                    Link::new(tr!(ind_link_handler())) {
                        on_activate_fn: |_| { println!("link clicked"); }
                    }
                }
            }

            // Badge
            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Badge")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    Badge::new(lit!("New"))
                    Badge::new(lit!("Beta"))
                    Badge::new(lit!("Stable")) {
                        background: SurfaceRole::Raised
                    }
                    Badge::new(lit!("3"))
                    Badge::new(lit!("99+"))
                }
            }

            // Avatar
            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Avatar")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Wrap {
                    spacing: 12.0
                    line_spacing: 12.0
                    Avatar::with_initials(lit!("CJ")) {
                        size: AvatarSize::Medium
                    }
                    Avatar::with_initials(lit!("AB")) {
                        shape: AvatarShape::RoundedSquare
                        seed: "alice"
                        presence: AvatarPresence::Online
                    }
                    Avatar::with_initials(lit!("MN")) {
                        size: AvatarSize::Large
                        seed: "mallory"
                        presence: AvatarPresence::Busy
                    }
                }
            }
        }
    )
}
