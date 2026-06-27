// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `scene-magnetism` — a small node-graph demonstrating `bastyde-scene`
//! magnetism: typed snap-and-connect between anchor points.
//!
//! Each node is a lightweight, draggable `RectItem` carrying two magnets:
//! a `Source` output on its right edge and a `Target` input on its left.
//! The predicate accepts `Source` -> `Target` on different nodes and
//! rejects everything else. On connect, the app appends a persistent
//! `PathItem` wire (the "persistent consumer" choice — scene fires the
//! event and stores no connection itself) and declares a `FlowTo` AT
//! relation so the connection reads to a screen reader.
//!
//! Three ways to form a connection, all sharing one mechanism:
//!
//! - **Drag a node** so its output magnet aligns with another node's
//!   input magnet; release snaps and connects (the brick / Composer
//!   reparent gesture).
//! - **Drag a wire from a port**: press directly on a magnet handle and
//!   drag a transient wire to a compatible magnet; release connects (the
//!   node-graph gesture). The node does not move.
//! - **Keyboard**: focus the scene and press `m` to enter connect mode,
//!   then arrow-keys to a source magnet, Enter to activate it, arrows to
//!   a target, Enter to connect. Esc cancels.
//!
//! Run with: `cargo run -p scene-magnetism`

use bastyde::canvas::{Path, Point, Rect};
use bastyde::prelude::*;
use bastyde::tokens::Alignment;
use bastyde::widgets::{Expand, TextWidget, VStack, ZStack};
use bastyde_scene::{
    A11yNode, A11yRelation, Magnet, MagnetConnection, MagnetRef, MagnetRole, MagnetVerdict,
    MagnetismConfig, PathItem, RectItem, SceneLayer, SceneModel, SceneView,
};

const NODE_W: f32 = 150.0;
const NODE_H: f32 = 64.0;

/// Add a node rect at `(x, y)` with an output (Source) magnet on its
/// right edge and an input (Target) magnet on its left edge. The magnet
/// payloads carry the node's name to show payload round-tripping.
fn add_node(model: &SceneModel, x: f32, y: f32, name: &str, color: Color) {
    let id = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, NODE_W, NODE_H))
            .fill(color)
            .stroke(Color::new(0.0, 0.0, 0.0, 0.5), 1.5)
            .access_label(lit!(name.to_string())),
        Point::new(x, y),
    );
    model.add_magnet(
        id,
        Magnet::new(Point::new(NODE_W, NODE_H * 0.5))
            .role(MagnetRole::Source)
            .payload(format!("{name} out"))
            .label(lit!(format!("{name} output"))),
    );
    model.add_magnet(
        id,
        Magnet::new(Point::new(0.0, NODE_H * 0.5))
            .role(MagnetRole::Target)
            .payload(format!("{name} in"))
            .label(lit!(format!("{name} input"))),
    );
}

/// Source <-> Target on different nodes accept; everything else rejects.
fn predicate(a: &MagnetRef, b: &MagnetRef) -> MagnetVerdict {
    if a.item != b.item
        && matches!(
            (a.role, b.role),
            (MagnetRole::Source, MagnetRole::Target) | (MagnetRole::Target, MagnetRole::Source)
        )
    {
        MagnetVerdict::accept()
    } else {
        MagnetVerdict::Reject
    }
}

/// Append a persistent bezier wire between the two connected magnets and
/// declare a `FlowTo` AT relation from the source node to the target.
fn add_wire(model: &SceneModel, conn: &MagnetConnection) {
    let (from, to) = (conn.from.scene_pos, conn.to.scene_pos);
    // Orient the connection so the wire always runs output -> input.
    let (start, end, src_item, dst_item) = if conn.from.role == MagnetRole::Source {
        (from, to, conn.from.item, conn.to.item)
    } else {
        (to, from, conn.to.item, conn.from.item)
    };
    let dx = (end.x - start.x).abs().max(48.0) * 0.5;
    let mut path = Path::new();
    path.move_to(start);
    path.cubic_to(
        Point::new(start.x + dx, start.y),
        Point::new(end.x - dx, end.y),
        end,
    );
    let bounds = Rect::new(
        start.x.min(end.x),
        start.y.min(end.y) - 4.0,
        (end.x - start.x).abs(),
        (end.y - start.y).abs() + 8.0,
    );
    let wire = model.add_item(
        PathItem::new(path, bounds)
            .stroke_cosmetic(Color::new(0.20, 0.65, 0.45, 0.95), 2.5)
            .access_label(lit!("connection")),
        Point::ZERO,
    );
    // Wires paint behind the node rects.
    model.set_z(wire, -10.0);
    model.set_layer(wire, SceneLayer::Under);
    // The connection's accessibility meaning is consumer-owned: declare
    // it on the scene's relation layer (mechanism in scene, policy here).
    model.add_a11y_relation(
        A11yNode::Item(src_item),
        A11yRelation::FlowTo,
        A11yNode::Item(dst_item),
    );
}

fn build_view() -> SceneView {
    let model = SceneModel::new();
    add_node(
        &model,
        60.0,
        120.0,
        "Input",
        Color::new(0.30, 0.45, 0.85, 1.0),
    );
    add_node(
        &model,
        360.0,
        50.0,
        "Blur",
        Color::new(0.55, 0.40, 0.80, 1.0),
    );
    add_node(
        &model,
        360.0,
        220.0,
        "Sharpen",
        Color::new(0.80, 0.50, 0.35, 1.0),
    );
    add_node(
        &model,
        660.0,
        140.0,
        "Output",
        Color::new(0.30, 0.65, 0.45, 1.0),
    );

    let on_connect_model = model.clone();
    let config = MagnetismConfig::new(predicate).on_connect(move |conn, _ctx| {
        add_wire(&on_connect_model, conn);
    });

    SceneView::with_model(model)
        .magnetism(config)
        .min_zoom(0.4)
        .max_zoom(3.0)
}

fn build_root() -> impl Widget + 'static {
    VStack::new()
        .spacing(8.0)
        .child(
            TextWidget::new(lit!("bastyde-scene magnetism — node graph"))
                .style(TextStyleRole::BodyBold),
        )
        .child(TextWidget::new(lit!(
            "Drag a node so its right port meets another node's left port, or drag a wire \
             from a port, or press 'm' then use arrows + Enter to connect."
        )))
        .child(
            Expand::new().child(
                ZStack::new()
                    .alignment(Alignment::TOP_LEADING)
                    .child(Expand::new().child(build_view())),
            ),
        )
}

fn main() {
    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — scene magnetism")
                .size(1000, 700)
                .root(|tree, _state| tree.add(build_root())),
        )
        .run();
}
