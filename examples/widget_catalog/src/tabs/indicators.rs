//! Indicators tab — read-only status: ProgressBar, Spinner, Link, Badge, Avatar.
//!
//! `classic()` constructs every widget with imperative builder calls
//! (`Type::new().method(...)`). `fern()` constructs the *same* widget
//! tree via the `fern!` macro DSL so the toggle visibly proves the two
//! authoring styles produce identical output.

use fern_ui::prelude::*;
use fern_ui::tokens::Orientation;
use fern_ui::widgets::{
    Avatar, AvatarPresence, AvatarShape, AvatarSize, Badge, Divider, FixedSize, HStack, Link,
    ProgressBar, Spinner, TextWidget, VStack,
};

use crate::shared::{Signals, section, tab_header};

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
        "ProgressBar — determinate",
        ProgressBar::new(0.6).label(tr!(ind_progress_determinate_label())),
    );
    let progress_indet = section(
        ctx,
        "ProgressBar — indeterminate",
        ProgressBar::indeterminate(),
    );
    let progress_v = section(
        ctx,
        "ProgressBar — vertical",
        FixedSize::new().bind_height(120.0_f32).child(
            ProgressBar::new(0.4)
                .orientation(Orientation::Vertical)
                .thickness(8.0),
        ),
    );
    let spinner = section(
        ctx,
        "Spinner",
        HStack::new()
            .spacing(16.0)
            .child(Spinner::new(20.0))
            .child(Spinner::new(28.0))
            .child(Spinner::new(36.0).label(tr!(demo_loading()))),
    );
    let link = section(
        ctx,
        "Link",
        VStack::new()
            .spacing(6.0)
            .child(Link::new(tr!(ind_link_docs())).url("https://example.com"))
            .child(Link::new(tr!(ind_link_handler())).on_activate_fn(|_| {
                println!("link clicked");
            })),
    );
    let badge = section(
        ctx,
        "Badge",
        HStack::new()
            .spacing(8.0)
            .child(Badge::new_literal("New"))
            .child(Badge::new_literal("Beta"))
            .child(Badge::new_literal("Stable").color(SurfaceRole::AccentSubtle))
            .child(Badge::new_literal("3"))
            .child(Badge::new_literal("99+")),
    );
    let avatar = section(
        ctx,
        "Avatar",
        HStack::new()
            .spacing(12.0)
            .child(Avatar::with_initials_literal("CJ").size(AvatarSize::Medium))
            .child(
                Avatar::with_initials_literal("AB")
                    .shape(AvatarShape::RoundedSquare)
                    .seed("alice")
                    .presence(AvatarPresence::Online),
            )
            .child(
                Avatar::with_initials_literal("MN")
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

// ── fern! DSL version ──────────────────────────────────────────────────
//
// Same widget tree as `classic`, expressed in fern! syntax. The block
// shape is `Type::ctor(args) { method: value … bare_child … }` per the
// fern! reference. Children of a stack are written as bare expressions
// (UpperCamel calls) or via the `child:` property for free functions
// returning Widget.
pub fn fern(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    fern!(ctx => VStack {
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
                TextWidget::new_literal("ProgressBar — determinate") {
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
                TextWidget::new_literal("ProgressBar — indeterminate") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                ProgressBar::indeterminate()
            }

            // ProgressBar — vertical
            VStack {
                spacing: 6.0
                TextWidget::new_literal("ProgressBar — vertical") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_height: 120.0_f32
                    ProgressBar::new(0.4) {
                        orientation: Orientation::Vertical
                        thickness: 8.0
                    }
                }
            }

            // Spinner
            VStack {
                spacing: 6.0
                TextWidget::new_literal("Spinner") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 16.0
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
                TextWidget::new_literal("Link") {
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
                TextWidget::new_literal("Badge") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 8.0
                    Badge::new_literal("New")
                    Badge::new_literal("Beta")
                    Badge::new_literal("Stable") {
                        color: SurfaceRole::Raised
                    }
                    Badge::new_literal("3")
                    Badge::new_literal("99+")
                }
            }

            // Avatar
            VStack {
                spacing: 6.0
                TextWidget::new_literal("Avatar") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 12.0
                    Avatar::with_initials_literal("CJ") {
                        size: AvatarSize::Medium
                    }
                    Avatar::with_initials_literal("AB") {
                        shape: AvatarShape::RoundedSquare
                        seed: "alice"
                        presence: AvatarPresence::Online
                    }
                    Avatar::with_initials_literal("MN") {
                        size: AvatarSize::Large
                        seed: "mallory"
                        presence: AvatarPresence::Busy
                    }
                }
            }
        }
    )
}
