#![allow(clippy::type_complexity)]

pub mod accessibility;
pub mod action;
pub mod animated_quad;
pub mod animation;
pub mod animation_builder;
pub mod app_event;
pub mod arena;
pub mod binding;
pub mod build_context;
pub mod color_prop;
pub mod drag_payload;
pub(crate) mod drag_state;
pub mod environment;
pub mod event;
pub(crate) mod event_handlers;
pub mod event_source;
pub mod focus;
pub mod frame_tick_scheduler;
pub mod gesture;
pub mod idle;
pub mod ime;
pub mod intent;
pub mod modal;
pub mod motion_visibility;
pub mod overlay;
pub mod presets;
pub mod raw_handle;
pub mod shortcut;
pub mod signal;
pub mod styles;
pub mod telemetry;
pub mod widget;
pub mod widget_builder;
pub mod widget_builder_branching;
pub mod widget_id;
pub mod widget_tree;
pub mod window;
pub mod window_chrome;

#[cfg(test)]
pub(crate) mod test_widgets;

pub use accessibility::{AccessNodeBuilder, AccessibilityInfo};
pub use action::{Action, ActionBuilder, ActionHandler};
pub use animated_quad::{AnimatedQuadHandle, AnimatedQuadKind, AnimatedQuadRegistry};
pub use animation::AnimationRequest;
pub use animation::AnimationScheduler;
pub use animation_builder::AnimationSpec;
pub use app_event::AppEvent;
pub use arena::WidgetArena;
pub use binding::{BindingLevel, BindingRegistry};
pub use build_context::BuildContext;
pub use color_prop::{ColorProp, TextStyleProp};
pub use drag_payload::{DragData, DragOrigin, DragPayload, ExternalDropData};
pub use drag_state::DropFeedback;
pub use environment::{Environment, LayoutDirection};
pub use event::{
    ButtonMask, EventResponse, Key, Modifiers, PointerButton, ScrollDelta, WidgetEvent,
};
pub use event_source::{
    AppEventPoster, EventSource, EventSourceAdapter, SubscriptionHandle, SubscriptionId,
};
pub use focus::{FocusOrigin, FocusPolicy};
pub use frame_tick_scheduler::{FrameTickScheduler, FrameTickSubscription};
pub use gesture::{
    DoubleTapRecognizer, DragRecognizer, GestureArena, GestureEvent, GestureRecognizer,
    GestureResult, LongPressRecognizer, RawPointerEvent, SwipeDirection, SwipeRecognizer, TapEvent,
    TapRecognizer, TripleTapRecognizer,
};
pub use idle::IdleDeadline;
pub use ime::{ImeContext, ImePurpose};
pub use intent::{Intent, IntentKind, IntentResponse};
pub use modal::{
    ModalBuilder, ModalCloseBehavior, ModalContent, ModalPresentation, ModalRequest,
    QueuedModalRequest,
};
pub use overlay::{
    DismissBehavior, OverlayId, OverlayLayer, OverlayManager, OverlayPlacement, OverlayRequest,
};
pub use raw_handle::ParentHandle;
pub use shortcut::{
    CaptureHandle, EffectiveShortcut, KeyCaptureCallback, KeyStroke, KeyStrokeOverride, Shortcut,
    ShortcutBuilder, ShortcutOnActivate, ShortcutRegistry, ShortcutScope, SlotOverride,
};
pub use signal::{ObserverHandle, Prop, Signal, SignalAccessError};
pub use styles::{Theme, ThemeAppearance, ThemeExtensions};
pub use widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget,
    WidgetPlacement,
};
pub use widget_builder::{
    AccessSubtreeMode, AccessibilityOverrides, HandlerSet, WidgetBuilder, WidgetWithHandlers,
};
pub use widget_builder_branching::{
    BatiBranch, BatiBranch3, BatiBranch4, IntoBatiChild, IntoBatiCondition,
};
pub use widget_id::WidgetId;
pub use widget_tree::WidgetTree;
pub use window::state::WindowStateInit;
pub use window::{
    DecorationsMode, BastydeWindowId, ModalConfig, NoopWindowOps, PostRootBuilder, RootBuilder,
    UserAttentionKind, WindowCommand, WindowConfig, WindowIcon, WindowOps, WindowPlacement,
    WindowState,
};
pub use window_chrome::{
    ControlTarget, HitRegions, PlatformError, PlatformTitleBarHost, ResizeBorders, ResizeEdge,
    TitleBarHostCallbacks, TitleBarHoverEvent, TitleBarSyntheticEvent,
};

pub use accesskit;
