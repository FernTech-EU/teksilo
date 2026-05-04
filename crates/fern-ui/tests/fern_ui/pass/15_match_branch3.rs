//! Spec §5.3: `match` at body position dispatches to `FernBranchN`
//! based on arm count. Three distinct-type arms lower via FernBranch3.

use fern_ui::prelude::*;

#[derive(Debug)]
struct Spinner;
impl Spinner {
    fn new() -> Self {
        Self
    }
}
impl Widget for Spinner {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug)]
struct DataView {
    contents: String,
}
impl DataView {
    fn new(contents: String) -> Self {
        Self { contents }
    }
}
impl Widget for DataView {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug)]
struct ErrorBanner {
    msg: String,
}
impl ErrorBanner {
    fn new(msg: String) -> Self {
        Self { msg }
    }
}
impl Widget for ErrorBanner {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug)]
enum State {
    Loading,
    Loaded(String),
    Error(String),
}

#[derive(Debug, Default)]
struct Holder;
impl Holder {
    fn new() -> Self {
        Self
    }

    fn child<W: Widget + 'static>(self, _w: W) -> Self {
        self
    }
}
impl Widget for Holder {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn render(state: State) -> Holder {
    fern!(
        Holder {
            match state {
                State::Loading => Spinner,
                State::Loaded(data) => DataView(data.clone()),
                State::Error(msg) => ErrorBanner(msg.clone()),
            }
        }
    )
}

fn main() {
    let _a = render(State::Loading);
    let _b = render(State::Loaded("hello".to_string()));
    let _c = render(State::Error("oops".to_string()));
}
