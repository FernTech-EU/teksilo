// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Live images in a tree: the attachment of a source to one widget, and the
//! Signals through which its size and status reach layout.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::ImageGeometry;
use teksilo_canvas::live_image::{LiveImageConsumer, LiveImageSource, LiveImageStatus};

use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// The two Signals an attachment writes: a source's size and status as the
/// window's last layout saw them. A widget showing a live image owns them
/// from construction, so they exist before it mounts and survive a switch to
/// another source.
#[derive(Clone)]
pub struct LiveImageSignals {
    /// The size layout uses: the buffer's, else the size hint; `None` with
    /// neither. Written only by the layout pre-pass and by an attach. Bind it
    /// at `Relayout`.
    pub frame_size: Signal<Option<(u32, u32)>>,
    /// Written only by the layout pre-pass and by an attach. Bind it at
    /// `RepaintOnly`, and at `AccessibilityOnly` where it changes what is
    /// announced.
    pub status: Signal<LiveImageStatus>,
}

impl LiveImageSignals {
    /// Signals holding `source`'s current size and status.
    pub fn new(source: &LiveImageSource) -> Self {
        Self {
            frame_size: Signal::new(source.size()),
            status: Signal::new(source.status()),
        }
    }
}

impl std::fmt::Debug for LiveImageSignals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveImageSignals")
            .field("frame_size", &self.frame_size.get())
            .field("status", &self.status.get())
            .finish()
    }
}

/// The UI-thread side of one attachment of a source to a widget, in one
/// window. `Clone` is an `Rc` clone; `!Send`. The widget's rebuild or
/// destruction detaches it, as does the tree's drop.
#[derive(Clone)]
pub struct LiveImageAttachment {
    pub(crate) entry: Rc<AttachmentEntry>,
}

pub(crate) struct AttachmentEntry {
    pub(crate) consumer: LiveImageConsumer,
    pub(crate) signals: LiveImageSignals,
    pub(crate) widget: WidgetId,
    geometry: Cell<Option<ImageGeometry>>,
}

impl AttachmentEntry {
    pub(crate) fn new(
        consumer: LiveImageConsumer,
        signals: LiveImageSignals,
        widget: WidgetId,
    ) -> Self {
        Self {
            consumer,
            signals,
            widget,
            geometry: Cell::new(None),
        }
    }
}

impl AttachmentEntry {
    pub(crate) fn geometry(&self) -> Option<ImageGeometry> {
        self.geometry.get()
    }
}

impl LiveImageAttachment {
    /// What a widget hands `Canvas::draw_live_image`.
    pub fn consumer(&self) -> &LiveImageConsumer {
        &self.entry.consumer
    }

    pub fn signals(&self) -> &LiveImageSignals {
        &self.entry.signals
    }

    pub fn widget_id(&self) -> WidgetId {
        self.entry.widget
    }

    /// Record the placement the widget paints from, in its `place_children`,
    /// so automation and lookups through the tree see it.
    pub fn set_geometry(&self, geometry: ImageGeometry) {
        self.entry.geometry.set(Some(geometry));
    }

    /// The placement last recorded; `None` before the first layout.
    pub fn geometry(&self) -> Option<ImageGeometry> {
        self.entry.geometry()
    }

    /// Whether it was released: its widget was rebuilt or destroyed, or its
    /// tree dropped.
    pub fn is_detached(&self) -> bool {
        self.entry.consumer.is_detached()
    }
}

impl std::fmt::Debug for LiveImageAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveImageAttachment")
            .field("widget", &self.entry.widget)
            .field("consumer", &self.entry.consumer)
            .finish()
    }
}
