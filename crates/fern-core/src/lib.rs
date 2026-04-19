pub mod accessibility;
pub mod action;
pub mod animated_quad;
pub mod animation;
pub mod app_event;
pub mod arena;
pub mod build_context;
pub mod color_prop;
pub mod drag_payload;
pub(crate) mod drag_state;
pub mod environment;
pub mod event;
pub(crate) mod event_handlers;
pub mod event_source;
pub mod focus;
pub mod gesture;
pub mod idle;
pub mod intent;
pub mod modal;
pub mod overlay;
pub mod shortcut;
pub mod signal;
pub mod binding;
pub mod widget;
pub mod widget_builder;
pub mod widget_builder_branching;
pub mod widget_id;
pub mod widget_tree;
pub mod window_chrome;

#[cfg(test)]
pub(crate) mod test_widgets;

pub use accessibility::{AccessNodeBuilder, AccessibilityInfo};
pub use animated_quad::{AnimatedQuadHandle, AnimatedQuadKind, AnimatedQuadRegistry};
pub use animation::AnimationScheduler;
pub use app_event::AppEvent;
pub use arena::WidgetArena;
pub use build_context::BuildContext;
pub use color_prop::{ColorProp, TextStyleProp};
pub use drag_payload::{DragData, DragPayload};
pub use drag_state::DropFeedback;
pub use environment::{Environment, LayoutDirection};
pub use event::{EventResponse, Key, Modifiers, PointerButton, ScrollDelta, WidgetEvent};
pub use event_source::{
    AppEventPoster, EventSource, EventSourceAdapter, SubscriptionHandle, SubscriptionId,
};
pub use focus::{FocusOrigin, FocusPolicy};
pub use gesture::{
    DoubleTapRecognizer, DragRecognizer, GestureArena, GestureEvent, GestureRecognizer,
    GestureResult, LongPressRecognizer, RawPointerEvent, SwipeDirection, SwipeRecognizer,
    TapRecognizer, TripleTapRecognizer,
};
pub use idle::IdleDeadline;
pub use modal::{
    ModalBuilder, ModalCloseBehavior, ModalContent, ModalPresentation, ModalRequest,
    QueuedModalRequest,
};
pub use overlay::{
    DismissBehavior, OverlayId, OverlayLayer, OverlayManager, OverlayPlacement, OverlayRequest,
};
pub use action::{Action, ActionBuilder, ActionHandler};
pub use intent::{Intent, IntentKind, IntentResponse};
pub use shortcut::{
    CaptureHandle, EffectiveShortcut, KeyCaptureCallback, KeyStroke, KeyStrokeOverride, Shortcut,
    ShortcutBuilder, ShortcutOnActivate, ShortcutRegistry, ShortcutScope, SlotOverride,
};
pub use animation::AnimationRequest;
pub use signal::{ObserverHandle, Prop, Signal, SignalAccessError};
pub use binding::{BindingLevel, BindingRegistry};
pub use widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
pub use widget_builder::{HandlerSet, WidgetBuilder, WidgetWithHandlers};
pub use widget_builder_branching::{
    FernBranch, FernBranch3, FernBranch4, IntoFernChild, IntoFernCondition,
};
pub use widget_id::WidgetId;
pub use widget_tree::WidgetTree;
pub use window_chrome::{
    HitRegions, PlatformError, PlatformTitleBarHost, ResizeBorders, ResizeEdge,
    TitleBarHostCallbacks,
};

pub use accesskit;
