// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub mod aspect_ratio;
pub mod center;
pub mod column_flow;
pub mod dead_zone;
pub mod divider;
pub mod expand;
pub mod fixed_size;
pub mod form_layout;
pub mod grid;
pub mod hstack;
pub mod icon_widget;
pub mod image_mask;
pub mod image_widget;
/// Shared main-then-cross negotiation for linear containers (`HStack`,
/// `VStack`). Distributes the main axis (grow via `flex`, shrink via
/// `shrink`/`min`) and measures each child's cross axis at its *final* main
/// size — the height-for-width pass. Internal helper, not a widget.
pub(crate) mod linear_layout;
pub mod masonry;
pub mod max_size;
pub mod min_size;
pub mod padding;
pub mod rect_widget;
pub mod shrinkable;
pub mod spacer;
pub mod switcher;
/// `TextInputField` — raw editable text surface primitive built on the
/// `text-typeset` / `text-document` stack. Consumed by `TextInput` (the
/// styled composite) and `SpinBox` (numeric input) and available to
/// third-party composites that need inline editable text.
pub mod text_input_field;
pub mod text_widget;
pub mod twist_arrow;
pub mod validation_strip;
pub mod vstack;
pub mod wrap;
pub mod zstack;

pub use aspect_ratio::AspectRatio;
pub use center::Center;
pub use column_flow::ColumnFlow;
pub use dead_zone::DeadZone;
pub use divider::Divider;
pub use expand::Expand;
pub use fixed_size::FixedSize;
pub use form_layout::FormLayout;
pub use grid::{Grid, TrackSize};
pub use hstack::HStack;
pub use icon_widget::IconWidget;
pub use image_mask::ImageMaskShape;
pub use image_widget::{ImageFit, ImageWidget};
pub use masonry::MasonryLayout;
pub use max_size::MaxSize;
pub use min_size::MinSize;
pub use padding::Padding;
pub use rect_widget::RectWidget;
pub use shrinkable::Shrinkable;
pub use spacer::Spacer;
pub use switcher::Switcher;
pub use text_input_field::{
    AtRevealPolicy, EchoMode, InputPurpose, TextFieldHandle, TextInputField,
};
pub use text_widget::TextWidget;
pub use twist_arrow::TwistArrow;
pub use validation_strip::ValidationStrip;
pub use vstack::VStack;
pub use wrap::Wrap;
pub use zstack::ZStack;
