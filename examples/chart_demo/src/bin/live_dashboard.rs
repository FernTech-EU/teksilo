// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A worker publishes samples; a UI subscription updates one view model.
//! Run with `cargo run -p chart-demo --bin live_dashboard`.
//! Data is simulated, so this example needs no host, SSH or GPU tools.

use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::Duration;
use teksilo::core::{EventSource, SubscriptionHandle, accesskit::Live};
use teksilo::prelude::*;
use teksilo::widgets::*;
use teksilo_charts::{AxisConfig, ChartModel, ChartWindow, LineChart, SeriesId};

const METERS: [&str; 5] = ["GPU", "VRAM", "CPU", "RAM", "Disk"];
const RETAINED_SAMPLES: usize = 240;

#[derive(Clone, Debug, Default)]
struct Sample {
    tick: u64,
    fractions: [f32; 5],
}

impl Sample {
    fn simulated(tick: u64) -> Self {
        Self {
            tick,
            fractions: std::array::from_fn(|i| ((tick + i as u64 * 17) % 101) as f32 / 100.0),
        }
    }
}

type Callback = Arc<dyn Fn(Sample) + Send + Sync>;
#[derive(Clone, Default)]
struct Samples {
    callbacks: Arc<Mutex<BTreeMap<u64, Callback>>>,
    next_id: Arc<AtomicU64>,
}

impl Samples {
    fn publish(&self, sample: Sample) {
        // Invoke outside the lock so callbacks can subscribe or unsubscribe.
        let callbacks: Vec<_> = self.callbacks.lock().unwrap().values().cloned().collect();
        for callback in callbacks {
            callback(sample.clone());
        }
    }
}

struct Subscription {
    source: Samples,
    id: u64,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.source.callbacks.lock().unwrap().remove(&self.id);
    }
}
impl EventSource for Samples {
    type Origin = ();
    type Event = Sample;
    fn subscribe(&self, (): (), callback: Callback) -> SubscriptionHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.callbacks.lock().unwrap().insert(id, callback);
        SubscriptionHandle::new(Subscription {
            source: self.clone(),
            id,
        })
    }
}

#[derive(Clone, Debug)]
struct DashboardState {
    sample: Signal<Sample>,
    history: ChartModel<String>,
    series: [SeriesId; 5],
}
impl DashboardState {
    fn new() -> Self {
        let history = ChartModel::new();
        let series = METERS.map(|name| history.add_series(name));
        Self {
            sample: Signal::new(Sample::default()),
            history,
            series,
        }
    }

    // Called by the UI subscription, never by the worker.
    fn accept(&self, sample: Sample) {
        for (index, series) in self.series.iter().enumerate() {
            self.history.push_point(
                *series,
                sample.tick.to_string(),
                sample.fractions[index] * 100.0,
            );
            while self.history.point_count(*series) > RETAINED_SAMPLES {
                self.history.remove_point(*series, 0);
            }
        }
        self.sample.set(sample);
    }
}

#[derive(Debug)]
struct SampleListener;
impl Widget for SampleListener {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = ctx
            .app_state::<DashboardState>()
            .expect("dashboard state registered")
            .clone();
        ctx.subscribe_event((), move |sample: &Sample| state.accept(sample.clone()));
        Vec::new()
    }
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        LayoutResponse::ZERO
    }
}

fn dashboard(state: &DashboardState) -> impl Widget + use<> {
    let mut rows = VStack::new().spacing(8.0);
    for (index, name) in METERS.iter().enumerate() {
        rows = rows
            .child(
                TextWidget::new(lit!("")).text(
                    state
                        .sample
                        .map(move |s| format!("{name}: {:.0}%", s.fractions[index] * 100.0)),
                ),
            )
            .child(
                ProgressBar::new(0.0)
                    .label(lit!(*name))
                    .value(state.sample.map(move |s| s.fractions[index]))
                    .access_live(Live::Off),
            );
    }
    rows.child(
        LineChart::new(ChartWindow::new(state.history.clone(), 120))
            .axis_y(AxisConfig::new().range(0.0, 100.0))
            .legend(true),
    )
}

fn main() {
    let source = Samples::default();
    let worker_source = source.clone();
    let (stop, stopped) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut tick = 0;
        while let Err(mpsc::RecvTimeoutError::Timeout) =
            stopped.recv_timeout(Duration::from_secs(1))
        {
            tick += 1;
            // Replace this computation with blocking I/O in a real app.
            worker_source.publish(Sample::simulated(tick));
        }
    });
    let state = DashboardState::new();
    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .theme(intui::dark())
        .event_source(source.clone())
        .app_state(source)
        .app_state(state.clone())
        .initial_window(
            WindowConfig::new()
                .title("Live dashboard")
                .size(720, 640)
                .root(move |tree, _| {
                    tree.add(VStack::new().child(SampleListener).child(dashboard(&state)))
                }),
        )
        .run();
    let _ = stop.send(());
    worker.join().expect("sample worker stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo::core::{WidgetTree, accesskit::Role};

    #[test]
    fn a_known_sample_updates_each_visible_meter_and_text() {
        let state = DashboardState::new();
        let mut tree = WidgetTree::new().with_theme(intui::dark());
        tree.add(dashboard(&state));
        tree.layout(SizeProposal::exact(720.0, 640.0));
        let _ = tree.sync_accessibility();
        state.accept(Sample {
            tick: 1,
            fractions: [0.11, 0.22, 0.33, 0.44, 0.55],
        });
        tree.layout(SizeProposal::exact(720.0, 640.0));
        let _ = tree.render();
        let update = tree.sync_accessibility();
        let meters: Vec<_> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == Role::ProgressIndicator)
            .collect();
        assert_eq!(meters.len(), 5);
        for (index, name) in METERS.iter().enumerate() {
            let node = meters
                .iter()
                .find(|(_, n)| n.label() == Some(*name))
                .expect("named meter");
            let expected = (index + 1) as f64 * 0.11;
            assert!((node.1.numeric_value().unwrap() - expected).abs() < 1e-6);
            let label = format!("{name}: {}%", (index + 1) * 11);
            assert!(
                update
                    .nodes
                    .iter()
                    .any(|(_, n)| n.role() == Role::Label && n.value() == Some(label.as_str())),
                "missing {label}: {:?}",
                update
                    .nodes
                    .iter()
                    .map(|(_, n)| (n.role(), n.label(), n.value()))
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(state.history.point_count(state.series[0]), 1);
    }

    #[test]
    fn history_is_bounded_and_rebuilding_keeps_the_model() {
        let state = DashboardState::new();
        for tick in 0..300 {
            state.accept(Sample::simulated(tick));
        }
        let mut tree = WidgetTree::new().with_theme(intui::dark());
        tree.add(dashboard(&state));
        tree.layout(SizeProposal::exact(720.0, 640.0));
        for series in state.series {
            assert_eq!(state.history.point_count(series), RETAINED_SAMPLES);
        }
    }

    #[test]
    fn dropping_a_subscription_stops_delivery() {
        let source = Samples::default();
        let count = Arc::new(AtomicU64::new(0));
        let received = count.clone();
        let subscription = source.subscribe(
            (),
            Arc::new(move |_| {
                received.fetch_add(1, Ordering::Relaxed);
            }),
        );
        source.publish(Sample::default());
        drop(subscription);
        source.publish(Sample::default());
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
