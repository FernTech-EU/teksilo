//! macOS external drag-and-drop backend.
//!
//! winit registers its content `NSView` only for `NSFilenamesPboardType` and
//! reports drops without a cursor position. To get position + text + URLs we
//! install our own drop target: a transparent, flipped overlay `NSView` added
//! on top of winit's content view, registered for file-URL / URL / string
//! pasteboard types and implementing `NSDraggingDestination`.
//!
//! The overlay overrides `hitTest:` to return null, so it is invisible to
//! mouse events (clicks pass through to winit's view below); AppKit's drag
//! destination search uses view geometry, not `hitTest:`, so the overlay still
//! receives drag callbacks. Overriding `isFlipped` to `true` makes its local
//! coordinate space top-left origin, matching bastyde's window-logical pixels.
//!
//! Each drag phase posts an [`ExternalDndEventPayload`] through the app's
//! [`AppEventPoster`]; `bastyde-app` routes it to the window's `WidgetTree`.
//! The registration is torn down by removing the overlay from its superview
//! when the guard drops (window close).

use std::ptr::NonNull;
use std::sync::Arc;

use bastyde_canvas::Point;
use bastyde_core::AppEventPoster;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::window::BastydeWindowId;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{ClassType, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSDragOperation, NSDraggingDestination, NSDraggingInfo,
    NSPasteboardTypeFileURL, NSPasteboardTypeString, NSPasteboardTypeURL, NSView,
};
use objc2_foundation::{NSArray, NSPoint, NSURL};
use raw_window_handle::RawWindowHandle;

use super::{
    ExternalDndBackend, ExternalDndGuard, ExternalDndEventPayload, ExternalDragEvent, NoopDndGuard,
};
use bastyde_core::ExternalDropData;

/// Per-view state for the overlay drop target.
struct DropViewIvars {
    window_id: BastydeWindowId,
    poster: Arc<dyn AppEventPoster>,
}

define_class!(
    // The overlay is an NSView (main-thread-only, inferred from the superclass).
    #[unsafe(super(NSView))]
    #[name = "BastydeExternalDropView"]
    #[ivars = DropViewIvars]
    struct DropView;

    impl DropView {
        // Top-left origin so converted drag coordinates match bastyde's
        // window-logical pixel space.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        // Mouse-transparent: clicks fall through to winit's view below. Drag
        // destination resolution does not use hitTest:, so drags still arrive.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }
    }

    unsafe impl NSObjectProtocol for DropView {}

    // The dragging-destination callbacks. Each builds an ExternalDragEvent and
    // posts it; the heavy work (hit-test, accept/reject, drop) happens in the
    // widget tree on the main loop tick.
    unsafe impl NSDraggingDestination for DropView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            let position = self.local_position(sender);
            let formats = self.advertised_formats(sender);
            self.post(ExternalDragEvent::Entered { formats, position });
            // Advertise Copy so the OS shows the "+" drop cursor; the widget
            // performs the real accept/reject. Returning None here would
            // suppress draggingUpdated/performDragOperation entirely.
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            let position = self.local_position(sender);
            self.post(ExternalDragEvent::Moved { position });
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.post(ExternalDragEvent::Left);
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let position = self.local_position(sender);
            let data = read_pasteboard(sender);
            let accepted = !data.is_empty();
            self.post(ExternalDragEvent::Dropped { data, position });
            accepted
        }
    }
);

impl DropView {
    fn new(
        mtm: MainThreadMarker,
        window_id: BastydeWindowId,
        poster: Arc<dyn AppEventPoster>,
        frame: objc2_foundation::NSRect,
    ) -> Retained<Self> {
        let this = mtm
            .alloc::<Self>()
            .set_ivars(DropViewIvars { window_id, poster });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// Convert the drag's window-base coordinate to this (flipped) view's
    /// local logical-pixel coordinate.
    fn local_position(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> Point {
        let window_loc = sender.draggingLocation();
        // `None` source view = convert from the window's base coordinate space.
        let local = self.convertPoint_fromView(window_loc, None);
        Point::new(local.x as f32, local.y as f32)
    }

    /// Pasteboard type identifiers the drag advertises (best-effort).
    fn advertised_formats(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
        let pb = sender.draggingPasteboard();
        match pb.types() {
            Some(types) => types.iter().map(|t| t.to_string()).collect(),
            None => Vec::new(),
        }
    }

    fn post(&self, event: ExternalDragEvent) {
        let ivars = self.ivars();
        ivars.poster.post_external(Box::new(ExternalDndEventPayload {
            window_id_owner: ivars.window_id,
            event,
        }));
    }
}

/// Extract files / URLs / text from the drag's pasteboard.
fn read_pasteboard(sender: &ProtocolObject<dyn NSDraggingInfo>) -> ExternalDropData {
    let pb = sender.draggingPasteboard();
    let mut data = ExternalDropData::default();

    // All URL objects on the pasteboard, split into file paths vs other URLs.
    let classes = NSArray::from_slice(&[NSURL::class()]);
    // SAFETY: `classes` holds the NSURL class; `None` options is valid.
    if let Some(objects) = unsafe { pb.readObjectsForClasses_options(&classes, None) } {
        for i in 0..objects.count() {
            let obj: Retained<AnyObject> = objects.objectAtIndex(i);
            if let Ok(url) = obj.downcast::<NSURL>() {
                if url.isFileURL() {
                    if let Some(path) = url.path() {
                        data.files.push(std::path::PathBuf::from(path.to_string()));
                    }
                } else if let Some(abs) = url.absoluteString() {
                    data.uris.push(abs.to_string());
                }
            }
        }
    }

    // Plain text, if present. (The type constant is an extern static.)
    if let Some(text) = pb.stringForType(unsafe { NSPasteboardTypeString }) {
        let s = text.to_string();
        if !s.is_empty() {
            data.text = Some(s);
        }
    }

    data
}

/// Registration guard: removes the overlay view from its superview on drop,
/// revoking the drop target.
pub struct MacOsDndGuard {
    overlay: Retained<DropView>,
}

impl ExternalDndGuard for MacOsDndGuard {}

impl Drop for MacOsDndGuard {
    fn drop(&mut self) {
        // Window teardown happens on the main thread; removing the overlay
        // here detaches it from the responder/view hierarchy.
        self.overlay.removeFromSuperview();
    }
}

/// macOS external-drag backend. See the module docs.
#[derive(Default)]
pub struct MacOsExternalDndBackend;

impl MacOsExternalDndBackend {
    pub fn new() -> Self {
        Self
    }
}

impl ExternalDndBackend for MacOsExternalDndBackend {
    fn attach(
        &mut self,
        parent: ParentHandle,
        window_id: BastydeWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        // Must run on the AppKit main thread to touch views.
        let Some(mtm) = MainThreadMarker::new() else {
            return Box::new(NoopDndGuard);
        };
        let RawWindowHandle::AppKit(handle) = parent.raw_window_handle() else {
            return Box::new(NoopDndGuard);
        };

        // Re-retain winit's content view.
        let ns_view_ptr: NonNull<NSView> = handle.ns_view.cast();
        let content_view: Retained<NSView> = unsafe { Retained::retain(ns_view_ptr.as_ptr()) }
            .expect("AppKit window handle has a live NSView");

        let bounds = content_view.bounds();
        let overlay = DropView::new(mtm, window_id, poster, bounds);

        // Track the content view's size so the drop target always covers it.
        overlay.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );

        // Accept the three formats we know how to extract.
        let types = NSArray::from_slice(&[
            unsafe { NSPasteboardTypeFileURL },
            unsafe { NSPasteboardTypeURL },
            unsafe { NSPasteboardTypeString },
        ]);
        overlay.registerForDraggedTypes(&types);

        // Add on top so it is frontmost for drag-destination resolution.
        content_view.addSubview(&overlay);

        Box::new(MacOsDndGuard { overlay })
    }
}
