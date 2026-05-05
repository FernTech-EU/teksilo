//! Animations drain test.
//!
//! A two-tab window — one with animated widgets (indeterminate
//! progress bars on the shader pipeline), one with a lone static
//! TextWidget. Useful for visually and numerically comparing the
//! idle-frame / CPU / GPU cost of the two tabs via the paint-epoch
//! dormant-pane gate.
//!
//! Run with:
//! ```sh
//! cargo run -p animations                 # stays on the Animated tab
//! cargo run -p animations -- --5s-tab      # flips to Static after 5s
//! ```
//!
//! Measure with the project's idle-drain tools:
//! ```sh
//! FERN_IDLE_TRACE=1 /tmp/measure_long.sh
//! ```
//! Expected: Animated tab → ~30 Hz rendered_frames, modest CPU (the
//! shader path keeps paint() out of the hot loop). Static tab →
//! zero trace lines, CPU near 0 %, GPU delta near baseline.

use std::thread;
use std::time::Duration;

use fern_ui::core::app_event::AppEvent;
use fern_ui::prelude::*;
use fern_ui::widgets::{HStack, ProgressBar, TabId, TabInfo, TabWidget, TextWidget, VStack};

/// External `AppEvent` payload — the 5 s sleeper thread sends one to
/// the UI thread, which downcasts and flips the tab signal.
#[derive(Debug)]
struct SwitchToStaticTab;

fn main() {
    let switch_after_5s = std::env::args().any(|a| a == "--5s-tab");

    // Shared tab-selection signal. Created outside the root builder
    // so the optional app-event handler can flip it from the 5 s
    // sleeper thread's reply without reaching into the tree.
    //
    // Two stable tab ids the root will assign to its two static
    // tabs. Captured here so the 5-second sleeper can flip the
    // selection by id without depending on internal indices.
    let animated_tab = TabId::fresh();
    let static_tab = TabId::fresh();
    let selected: Signal<Option<TabId>> = Signal::new(Some(animated_tab));
    let selected_for_root = selected.clone();

    let mut builder = FernAppBuilder::new()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Animations Drain Test")
                .size(640, 420)
                .root(move |tree, _state| {
                    tree.add(AnimationsRoot::new(
                        selected_for_root,
                        animated_tab,
                        static_tab,
                    ))
                }),
        );

    if switch_after_5s {
        let selected_for_handler = selected;
        builder = builder
            .on_app_event(move |event| {
                if let AppEvent::External(payload) = event
                    && payload.downcast_ref::<SwitchToStaticTab>().is_some()
                {
                    eprintln!("animations: --5s-tab elapsed; switching to Static tab");
                    selected_for_handler.set(Some(static_tab));
                }
            })
            .on_ready(|proxy| {
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(5));
                    proxy.send_external(SwitchToStaticTab);
                });
            });
    }

    builder.run();
}

// ---------------------------------------------------------------------------
// Root widget
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct AnimationsRoot {
    selected: Signal<Option<TabId>>,
    animated_tab: TabId,
    static_tab: TabId,
    root_child_id: Option<WidgetId>,
}

impl AnimationsRoot {
    fn new(selected: Signal<Option<TabId>>, animated_tab: TabId, static_tab: TabId) -> Self {
        Self {
            selected,
            animated_tab,
            static_tab,
            root_child_id: None,
        }
    }
}

impl Widget for AnimationsRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Allocate the static tabs with the pre-shared ids so the
        // app-event handler can flip selection by id.
        let tabs = ctx.add(
            TabWidget::new(self.selected.clone())
                .static_tab_with_id(
                    self.animated_tab,
                    TabInfo::new().title(fern_ui::i18n::LocalizedString::literal("Animated")),
                    animated_page(),
                )
                .static_tab_with_id(
                    self.static_tab,
                    TabInfo::new().title(fern_ui::i18n::LocalizedString::literal("Static")),
                    static_page(),
                ),
        );
        self.root_child_id = Some(tabs);
        vec![tabs]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}

// ---------------------------------------------------------------------------
// Animated tab — shader-driven progress bars
// ---------------------------------------------------------------------------

fn animated_page() -> impl Widget + 'static {
    VStack::new()
        .spacing(20.0)
        .child(
            TextWidget::new_literal("Shader-driven animated widgets")
                .style(TextStyleRole::BodyBold),
        )
        .child(
            TextWidget::new_literal(
                "Three indeterminate progress bars should sweep continuously — one \
                 `queue.write_buffer` + one `draw_indexed` per frame, no `paint()` re-run.",
            )
            .style(TextStyleRole::Body),
        )
        .child(labelled_bar("Default"))
        .child(labelled_bar("Accent on dark track"))
        .child(labelled_bar("Accent on dark track"))
        .child(
            TextWidget::new_literal(
                "Run with `--5s-tab` to auto-switch to the Static tab after 5 s and \
                 observe the frame rate and CPU drop to idle.",
            )
            .style(TextStyleRole::Small),
        )
}

fn labelled_bar(label: &str) -> impl Widget + use<> {
    HStack::new()
        .spacing(12.0)
        .child(TextWidget::new_literal(label.to_string()).style(TextStyleRole::Small))
        .child(ProgressBar::indeterminate())
}

// ---------------------------------------------------------------------------
// Static tab — no animations at all
// ---------------------------------------------------------------------------

fn static_page() -> impl Widget + 'static {
    VStack::new().spacing(20.0).child(
        TextWidget::new_literal(
            "Static tab — no animations, no timers, no per-frame work. CPU and GPU \
             should drop to idle; FERN_IDLE_TRACE=1 should emit zero lines.",
        )
        .style(TextStyleRole::BodyBold),
    )
}
