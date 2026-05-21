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
use objc2::{AnyThread, ClassType, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSDragOperation, NSDraggingContext,
    NSDraggingDestination, NSDraggingInfo, NSDraggingItem, NSDraggingSession, NSDraggingSource,
    NSImage, NSPasteboardTypeFileURL, NSPasteboardTypeString, NSPasteboardTypeURL,
    NSPasteboardWriting, NSView, NSWorkspace,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString, NSURL};
use raw_window_handle::RawWindowHandle;

use super::{
    ExternalDndBackend, ExternalDndEventPayload, ExternalDndGuard, ExternalDragEvent, NoopDndGuard,
};
use bastyde_core::{DragImageData, DropOutcome, ExternalDropData, OutboundDragData};

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
            // The dragging pasteboard is readable throughout the drag on
            // macOS, so the drop target can validate files on hover.
            let data = read_pasteboard(sender);
            self.post(ExternalDragEvent::Entered { data, position });
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

    // The drag-*source* side (outbound, app → OS). The overlay is the source
    // object for any drag this window starts via `begin_drag`.
    unsafe impl NSDraggingSource for DropView {
        // Advertise Copy only. Allowing Move would let the destination (e.g.
        // Finder) physically relocate a dragged file off disk — a dangerous
        // implicit default. Move-out should be an explicit opt-in, not the
        // baseline behavior.
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn dragging_source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }

        // The OS drag finished. Map the operation the destination performed to
        // our outcome and post it so the framework fires the source widget's
        // `on_drag_ended`.
        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn dragging_source_ended(
            &self,
            _session: &NSDraggingSession,
            _screen_point: NSPoint,
            operation: NSDragOperation,
        ) {
            let outcome = if operation.is_empty() {
                DropOutcome::Cancelled
            } else if operation.contains(NSDragOperation::Move) {
                DropOutcome::OsMove
            } else {
                DropOutcome::OsCopy
            };
            self.post(ExternalDragEvent::DragEnded { outcome });
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

    fn post(&self, event: ExternalDragEvent) {
        let ivars = self.ivars();
        ivars.poster.post_external(Box::new(ExternalDndEventPayload {
            window_id_owner: ivars.window_id,
            event,
        }));
    }

    /// Start a native OS drag exporting `data`. Called on the AppKit main
    /// thread synchronously from the framework's escalation path (winit is
    /// still dispatching the mouse-drag event), so `NSApp.currentEvent` is the
    /// live drag event that `beginDraggingSessionWithItems:event:source:`
    /// requires. Returns `true` if a session started.
    ///
    /// The `image` parameter is reserved for a future caller-supplied drag
    /// bitmap; today the framework passes `None` and we use the file icon (for
    /// file drags) or a small placeholder.
    fn begin_drag(&self, data: &OutboundDragData, _image: Option<&DragImageData>) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let app = NSApplication::sharedApplication(mtm);
        let Some(event) = app.currentEvent() else {
            return false;
        };

        // A drag item carries its origin frame in the source view's (flipped)
        // coordinates; anchor it at the current pointer so the drag image
        // tracks naturally.
        let origin = self.convertPoint_fromView(event.locationInWindow(), None);
        let frame = NSRect::new(origin, NSSize::new(32.0, 32.0));

        let workspace = NSWorkspace::sharedWorkspace();
        let mut items: Vec<Retained<NSDraggingItem>> = Vec::new();

        for path in &data.files {
            let s = NSString::from_str(&path.to_string_lossy());
            let url = NSURL::fileURLWithPath(&s);
            let writer: &ProtocolObject<dyn NSPasteboardWriting> = ProtocolObject::from_ref(&*url);
            let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer);
            let icon = workspace.iconForFile(&s);
            unsafe {
                item.setDraggingFrame_contents(frame, Some(AsRef::<AnyObject>::as_ref(&*icon)));
            }
            items.push(item);
        }
        for uri in &data.uris {
            let s = NSString::from_str(uri);
            let Some(url) = NSURL::URLWithString(&s) else {
                continue;
            };
            let writer: &ProtocolObject<dyn NSPasteboardWriting> = ProtocolObject::from_ref(&*url);
            let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer);
            let img = placeholder_image(mtm);
            unsafe {
                item.setDraggingFrame_contents(frame, Some(AsRef::<AnyObject>::as_ref(&*img)));
            }
            items.push(item);
        }
        if let Some(text) = &data.text {
            let s = NSString::from_str(text);
            let writer: &ProtocolObject<dyn NSPasteboardWriting> = ProtocolObject::from_ref(&*s);
            let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer);
            let img = placeholder_image(mtm);
            unsafe {
                item.setDraggingFrame_contents(frame, Some(AsRef::<AnyObject>::as_ref(&*img)));
            }
            items.push(item);
        }

        if items.is_empty() {
            return false;
        }

        let array = NSArray::from_retained_slice(&items);
        let source: &ProtocolObject<dyn NSDraggingSource> = ProtocolObject::from_ref(self);
        self.beginDraggingSessionWithItems_event_source(&array, &event, source);
        true
    }
}

/// A small blank drag image for non-file items (text / URLs), where there is
/// no natural file icon. Functional placeholder until caller-supplied
/// `DragImageData` rasterization lands.
fn placeholder_image(_mtm: MainThreadMarker) -> Retained<NSImage> {
    NSImage::initWithSize(NSImage::alloc(), NSSize::new(16.0, 16.0))
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

impl ExternalDndGuard for MacOsDndGuard {
    fn begin_drag(&self, data: &OutboundDragData, image: Option<&DragImageData>) -> bool {
        self.overlay.begin_drag(data, image)
    }
}

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
