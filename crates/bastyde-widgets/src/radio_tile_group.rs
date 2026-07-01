// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! RadioTileGroup — an N-ary group of [`RadioTile`]s with single selection.
//!
//! Like [`SegmentedControl`](crate::segmented_control::SegmentedControl), the
//! tile count is not fixed: add any number of tiles, all sharing one
//! `Signal<usize>`. The group owns:
//!
//! - **Layout** — an equal-size [`TileLayout::Row`], an adaptive wrapping
//!   [`TileLayout::Grid`], a full-width [`TileLayout::Column`], or a compact
//!   fixed-height [`TileLayout::Vertical`] settings list. Row and Grid equalize
//!   tile size (uniform width + the tallest tile's height) via a custom
//!   `place_children` measuring each tile height-for-width — stacks have no
//!   cross-axis stretch, so the group does the sizing.
//! - **Keyboard** — the WAI-ARIA *roving radiogroup* pattern: the group is a
//!   single Tab stop; Arrow keys move selection (selection follows focus),
//!   Home/End jump, disabled tiles are skipped. `Increment`/`Decrement` AT
//!   actions mirror the arrows for switch access.
//! - **Accessibility** — `Role::RadioGroup` with `active_descendant` pointing
//!   at the selected tile; each tile is `Role::RadioButton` and declares its
//!   siblings via `push_to_radio_group` (for "N of M").
//!
//! ```ignore
//! let selected = ctx.signal(0_usize);
//! RadioTileGroup::new(selected)
//!     .label(tr!(project_format()))
//!     .tile(RadioTile::new().icon(a).title(tr!(single_file())).description(tr!(single_file_desc())))
//!     .tile(RadioTile::new().icon(b).title(tr!(bundle())).description(tr!(bundle_desc())))
//!     .layout(TileLayout::Row)
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::styles::SharedRadioTileStyle;
use bastyde_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;

use crate::radio_tile::RadioTile;
use crate::styles::{RADIO_TILE_CORNER_RADIUS, RADIO_TILE_VERTICAL_ROW_HEIGHT};
use bastyde_i18n::LocalizedString;

/// How a [`RadioTileGroup`] arranges its tiles.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub enum TileLayout {
    /// A single horizontal row of equal-width, equal-height tiles (the tiles
    /// stretch to the tallest). The reference "two cards side-by-side" layout.
    #[default]
    Row,
    /// A wrapping grid whose column count adapts to the available width:
    /// `cols = floor((width + spacing) / (min_tile_width + spacing))`, at least
    /// one. All cells share the same width and the tallest tile's height.
    Grid {
        /// Minimum width a tile may have before the grid drops a column.
        min_tile_width: f32,
    },
    /// A vertical column of full-width tiles, each its natural height. Tiles
    /// keep their full card content (icon + title + description).
    Column,
    /// A vertical list of **compact** fixed-height full-width rows: `[radio]
    /// [icon] [title] [Spacer] [trailing]`, no description — the settings-list
    /// look. Every row is a fixed height taken from the active
    /// `RadioTileStyle` (the theme's `RadioTileRecipe::vertical_row_height`,
    /// 44 dp by default; override per-group with [`RadioTileGroup::row_height`]),
    /// and the group switches each tile to the compact arrangement (leading
    /// radio) automatically.
    Vertical,
}

/// Space (logical px) reserved around the tiles for the whole-group keyboard
/// focus ring — the SegmentedControl envelope model.
fn focus_ring_envelope(theme: &bastyde_core::Theme) -> f32 {
    theme.shape.focus_ring_offset + theme.shape.focus_ring_width
}

/// An N-ary, single-selection group of selectable-card radios. See the
/// [module docs](self).
pub struct RadioTileGroup {
    pending: Vec<RadioTile>,
    selected: Signal<usize>,
    label: Option<LocalizedString>,
    layout: TileLayout,
    /// Gap between tiles along the main axis (and between grid columns).
    /// `None` uses a layout-appropriate default: 6 dp for the compact
    /// `Vertical` list, 12 dp for `Row` / `Grid` / `Column`.
    spacing: Option<f32>,
    line_spacing: f32,
    /// Fixed row height for [`TileLayout::Vertical`]; `None` uses
    /// [`VERTICAL_ROW_HEIGHT`].
    row_height: Option<f32>,
    initial_enabled: bool,
    style_override: Option<SharedRadioTileStyle>,
    /// Written by the group's `on_focus`.
    group_focused: Signal<bool>,
    /// `group_focused AND focus-visible` — drives the whole-group keyboard
    /// focus ring (only after Tab navigation, not a mouse click). Computed in
    /// `build()`, read in `paint()`.
    ring_visible: Signal<bool>,
    /// Shared sibling-id buffer (the `RadioGroup` pattern) for
    /// `push_to_radio_group`.
    group_ids: Rc<RefCell<Vec<WidgetId>>>,
    tile_ids: Vec<WidgetId>,
    /// Live column count, updated during layout and read by the Grid keyboard
    /// navigation (which has no `LayoutContext`).
    col_count: Rc<Cell<usize>>,
}

impl RadioTileGroup {
    /// Create a group bound to the shared selection signal. Add tiles with
    /// [`tile`](Self::tile) / [`tiles`](Self::tiles).
    pub fn new(selected: Signal<usize>) -> Self {
        Self {
            pending: Vec::new(),
            selected,
            label: None,
            layout: TileLayout::default(),
            spacing: None,
            line_spacing: 12.0,
            row_height: None,
            initial_enabled: true,
            style_override: None,
            group_focused: Signal::new(false),
            ring_visible: Signal::new(false),
            group_ids: Rc::new(RefCell::new(Vec::new())),
            tile_ids: Vec::new(),
            col_count: Rc::new(Cell::new(1)),
        }
    }

    /// Accessible name for the group (announced before individual tiles).
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Add a tile. Its `value` (position) and shared selection signal are
    /// assigned automatically.
    pub fn tile(mut self, tile: RadioTile) -> Self {
        self.pending.push(tile);
        self
    }

    /// Add several tiles from an iterator.
    pub fn tiles(mut self, tiles: impl IntoIterator<Item = RadioTile>) -> Self {
        self.pending.extend(tiles);
        self
    }

    /// Choose the layout (default [`TileLayout::Row`]).
    pub fn layout(mut self, layout: TileLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Override the gap between tiles along the main axis (and grid columns).
    /// Defaults to 6 dp for `TileLayout::Vertical`, 12 dp otherwise.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = Some(spacing);
        self
    }

    /// Gap between rows in [`TileLayout::Grid`].
    pub fn line_spacing(mut self, spacing: f32) -> Self {
        self.line_spacing = spacing;
        self
    }

    /// Override the fixed row height for [`TileLayout::Vertical`] compact rows.
    /// Takes precedence over the theme value
    /// (`RadioTileRecipe::vertical_row_height`, 44 dp by default). No effect on
    /// other layouts.
    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = Some(height);
        self
    }

    /// Set the initial enabled state for the whole group.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Forward a `RadioTileStyle` to every tile that doesn't set its own
    /// `.style(...)`.
    pub fn style(mut self, style: impl bastyde_core::styles::RadioTileStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Next selectable index in `dir` (true = forward), wrapping and skipping
    /// disabled tiles. Returns `current` if no other tile is enabled.
    fn step(current: usize, forward: bool, disabled: &[bool]) -> usize {
        let n = disabled.len();
        if n == 0 {
            return current;
        }
        let mut i = current;
        for _ in 0..n {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            if !disabled[i] {
                return i;
            }
        }
        current
    }

    fn first_enabled(disabled: &[bool]) -> usize {
        (0..disabled.len()).find(|&i| !disabled[i]).unwrap_or(0)
    }

    fn last_enabled(disabled: &[bool]) -> usize {
        (0..disabled.len())
            .rev()
            .find(|&i| !disabled[i])
            .unwrap_or(disabled.len().saturating_sub(1))
    }

    /// Vertical move by `±cols` in a grid, snapping to the nearest enabled tile
    /// in that column-ish region; stays put if the move leaves the grid.
    fn step_vertical(current: usize, down: bool, cols: usize, disabled: &[bool]) -> usize {
        let n = disabled.len();
        if n == 0 || cols == 0 {
            return current;
        }
        let target = if down {
            current + cols
        } else if current >= cols {
            current - cols
        } else {
            return current;
        };
        if target >= n {
            return current;
        }
        if !disabled[target] {
            return target;
        }
        // Landed on a disabled tile — scan forward to the nearest enabled one.
        Self::step(target, true, disabled)
    }

    /// Compute per-tile rects (relative to the group origin) and the group's
    /// total size for the given available width. Also refreshes `col_count`.
    fn compute_layout(&self, avail_w: Option<f32>, ctx: &LayoutContext) -> (Vec<Rect>, Size) {
        let n = self.tile_ids.len();
        if n == 0 {
            self.col_count.set(1);
            return (Vec::new(), Size::new(0.0, 0.0));
        }
        let nf = n as f32;
        let sp = self.spacing.unwrap_or(match self.layout {
            TileLayout::Vertical => 6.0,
            _ => 12.0,
        });
        let lsp = self.line_spacing;

        let measure_h = |id: WidgetId, w: f32| -> f32 {
            ctx.measure_intrinsic(
                id,
                SizeProposal {
                    width: Some(w),
                    height: None,
                },
            )
            .map(|s| s.height)
            .unwrap_or(0.0)
        };

        // Resolve an unbounded width to a natural single-row / single-column
        // estimate so the group still reports a finite size.
        let avail = avail_w.unwrap_or_else(|| {
            let maxw = self
                .tile_ids
                .iter()
                .map(|&id| {
                    ctx.measure_intrinsic(id, SizeProposal::unspecified())
                        .map(|s| s.width)
                        .unwrap_or(0.0)
                })
                .fold(0.0_f32, f32::max);
            match self.layout {
                TileLayout::Column | TileLayout::Vertical => maxw,
                _ => maxw * nf + (nf - 1.0) * sp,
            }
        });

        match self.layout {
            TileLayout::Row => {
                self.col_count.set(n);
                let tile_w = ((avail - (nf - 1.0) * sp) / nf).max(0.0);
                let row_h = self
                    .tile_ids
                    .iter()
                    .map(|&id| measure_h(id, tile_w))
                    .fold(0.0_f32, f32::max);
                let mut rects = Vec::with_capacity(n);
                let mut x = 0.0;
                for _ in 0..n {
                    rects.push(Rect::new(x, 0.0, tile_w, row_h));
                    x += tile_w + sp;
                }
                (rects, Size::new(avail, row_h))
            }
            TileLayout::Column => {
                self.col_count.set(1);
                let mut rects = Vec::with_capacity(n);
                let mut y = 0.0;
                for &id in &self.tile_ids {
                    let h = measure_h(id, avail);
                    rects.push(Rect::new(0.0, y, avail, h));
                    y += h + sp;
                }
                let total_h = (y - sp).max(0.0);
                (rects, Size::new(avail, total_h))
            }
            TileLayout::Vertical => {
                // Compact rows are a fixed height (the settings-list
                // convention) — not measured per tile. Precedence: an explicit
                // `.row_height(..)` override, else the active `RadioTileStyle`'s
                // theme value (group style → theme slot → recipe default).
                self.col_count.set(1);
                let h = self.row_height.unwrap_or_else(|| {
                    self.style_override
                        .as_ref()
                        .or(ctx.theme.style_slots.radio_tile.as_ref())
                        .map(|s| s.vertical_row_height())
                        .unwrap_or(RADIO_TILE_VERTICAL_ROW_HEIGHT)
                });
                let mut rects = Vec::with_capacity(n);
                let mut y = 0.0;
                for _ in 0..n {
                    rects.push(Rect::new(0.0, y, avail, h));
                    y += h + sp;
                }
                let total_h = (h * nf + (nf - 1.0) * sp).max(0.0);
                (rects, Size::new(avail, total_h))
            }
            TileLayout::Grid { min_tile_width } => {
                let cols = (((avail + sp) / (min_tile_width + sp)).floor() as usize).clamp(1, n);
                self.col_count.set(cols);
                let colsf = cols as f32;
                let cell_w = ((avail - (colsf - 1.0) * sp) / colsf).max(0.0);
                let cell_h = self
                    .tile_ids
                    .iter()
                    .map(|&id| measure_h(id, cell_w))
                    .fold(0.0_f32, f32::max);
                let rows = n.div_ceil(cols);
                let mut rects = Vec::with_capacity(n);
                for i in 0..n {
                    let r = (i / cols) as f32;
                    let c = (i % cols) as f32;
                    rects.push(Rect::new(
                        c * (cell_w + sp),
                        r * (cell_h + lsp),
                        cell_w,
                        cell_h,
                    ));
                }
                let total_h = rows as f32 * cell_h + (rows.saturating_sub(1)) as f32 * lsp;
                (rects, Size::new(avail, total_h))
            }
        }
    }
}

impl std::fmt::Debug for RadioTileGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioTileGroup")
            .field("layout", &self.layout)
            .field("num_tiles", &self.pending.len().max(self.tile_ids.len()))
            .field("label", &self.label)
            .finish()
    }
}

impl Widget for RadioTileGroup {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }

        // Whole-group keyboard focus ring: visible only when the group holds
        // focus AND the last input was keyboard (`:focus-visible`).
        let focus_visible = ctx.focus_visible();
        let ring_visible = self.group_focused.and(&focus_visible);
        self.ring_visible = ring_visible.clone();

        // Re-walk AT on selection change so `active_descendant` stays current;
        // repaint the ring when focus/modality flips.
        {
            let registry = ctx.binding_registry();
            self.selected
                .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
            ring_visible.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        }

        let pending = std::mem::take(&mut self.pending);
        let n = pending.len();
        self.group_ids.borrow_mut().clear();
        self.tile_ids.clear();
        let mut disabled: Vec<bool> = Vec::with_capacity(n);

        // Two-pass: inject selection + group wiring before adding each tile,
        // then record its id in the shared sibling buffer (the RadioGroup
        // pattern).
        for (i, mut tile) in pending.into_iter().enumerate() {
            tile.set_selection(i, self.selected.clone());
            tile.set_grouped(self.group_focused.clone(), self.group_ids.clone(), i + 1, n);
            if self.layout == TileLayout::Vertical {
                tile.set_vertical_arrangement();
            }
            if let Some(style) = &self.style_override {
                tile.set_style_if_unset(style.clone());
            }
            disabled.push(!tile.is_enabled());
            let id = ctx.add(tile);
            self.group_ids.borrow_mut().push(id);
            self.tile_ids.push(id);
        }

        let disabled: Rc<Vec<bool>> = Rc::new(disabled);
        let layout = self.layout;
        let col_count = self.col_count.clone();

        let mut handlers = HandlerSet::new().focusable(true);

        // Roving keyboard: selection follows focus (WAI-ARIA radiogroup).
        {
            let selected = self.selected.clone();
            let disabled = disabled.clone();
            let col_count = col_count.clone();
            handlers = handlers.on_key(move |event, _ctx: &mut EventContext| {
                if n == 0 {
                    return EventResponse::Ignored;
                }
                let cur = selected.get().min(n - 1);
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                let next = match (layout, key) {
                    // Grid: 2-D navigation.
                    (TileLayout::Grid { .. }, Key::ArrowRight) => Self::step(cur, true, &disabled),
                    (TileLayout::Grid { .. }, Key::ArrowLeft) => Self::step(cur, false, &disabled),
                    (TileLayout::Grid { .. }, Key::ArrowDown) => {
                        Self::step_vertical(cur, true, col_count.get(), &disabled)
                    }
                    (TileLayout::Grid { .. }, Key::ArrowUp) => {
                        Self::step_vertical(cur, false, col_count.get(), &disabled)
                    }
                    // Row / Column: any arrow moves linearly.
                    (_, Key::ArrowRight | Key::ArrowDown) => Self::step(cur, true, &disabled),
                    (_, Key::ArrowLeft | Key::ArrowUp) => Self::step(cur, false, &disabled),
                    (_, Key::Home) => Self::first_enabled(&disabled),
                    (_, Key::End) => Self::last_enabled(&disabled),
                    _ => return EventResponse::Ignored,
                };
                if next != cur {
                    selected.set(next);
                }
                EventResponse::Handled
            });
        }

        // Track group focus (drives tile focus rings + selection surface).
        {
            let group_focused = self.group_focused.clone();
            handlers = handlers.on_focus(move |gained, _ctx: &mut EventContext| {
                group_focused.set(gained);
            });
        }

        // Increment / Decrement AT actions mirror the arrow keys.
        {
            let selected = self.selected.clone();
            let disabled = disabled.clone();
            handlers = handlers.on_access_action(move |action, _ctx: &mut EventContext| {
                if n == 0 {
                    return EventResponse::Ignored;
                }
                let cur = selected.get().min(n - 1);
                if action == bastyde_core::accesskit::Action::Increment {
                    selected.set(Self::step(cur, true, &disabled));
                    EventResponse::Handled
                } else if action == bastyde_core::accesskit::Action::Decrement {
                    selected.set(Self::step(cur, false, &disabled));
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        self.tile_ids.clone()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Reserve a focus-ring envelope around the tiles (the SegmentedControl
        // model) so the whole-group ring has room outside the tile bounds.
        let env = focus_ring_envelope(ctx.theme);
        let inner_w = proposal.width.map(|w| (w - env * 2.0).max(0.0));
        let (_rects, size) = self.compute_layout(inner_w, ctx);
        Size::new(size.width + env * 2.0, size.height + env * 2.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let env = focus_ring_envelope(ctx.theme);
        let inner_w = (bounds.width - env * 2.0).max(0.0);
        let (rects, _size) = self.compute_layout(Some(inner_w), ctx);
        for (child, rect) in children.iter_mut().zip(rects.iter()) {
            child.origin = Point::new(bounds.x + env + rect.x, bounds.y + env + rect.y);
            child.size = Size::new(rect.width, rect.height);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // One keyboard focus ring around the whole group, drawn in the
        // reserved envelope outside the tiles. `focus_ring` desaturates itself
        // in an inactive window (theme-side).
        if !self.ring_visible.get() {
            return;
        }
        let shape = &ctx.theme.shape;
        let half = shape.focus_ring_width * 0.5;
        let ring_rect = Rect::new(
            bounds.x + half,
            bounds.y + half,
            (bounds.width - half * 2.0).max(0.0),
            (bounds.height - half * 2.0).max(0.0),
        );
        let ring_radius = RADIO_TILE_CORNER_RADIUS + shape.focus_ring_offset + half;
        canvas.stroke_rounded_rect(
            ring_rect,
            CornerRadius::uniform(ring_radius),
            ctx.theme.colors.focus_ring,
            shape.focus_ring_width,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::RadioGroup);
        if let Some(ref name) = self.label {
            builder.set_name(name.resolve_now());
        }
        // Roving focus: focus stays on the group; point at the selected tile.
        let idx = self.selected.get();
        if let Some(&id) = self.tile_ids.get(idx) {
            builder.set_active_descendant(bastyde_core::accessibility::widget_id_to_node_id(id));
        }
        builder.add_action(bastyde_core::accesskit::Action::Focus);
        builder.add_action(bastyde_core::accesskit::Action::Increment);
        builder.add_action(bastyde_core::accesskit::Action::Decrement);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.tile_ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::event::Modifiers;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    fn group_with(
        selected: Signal<usize>,
        layout: TileLayout,
        descriptions: &[&'static str],
    ) -> RadioTileGroup {
        let labels = ["A", "B", "C", "D", "E", "F"];
        let mut g = RadioTileGroup::new(selected).layout(layout);
        for (i, desc) in descriptions.iter().enumerate() {
            g = g.tile(
                RadioTile::new()
                    .title(lit!(labels[i]))
                    .description(lit!(*desc)),
            );
        }
        g
    }

    fn tile_ids(tree: &WidgetTree, n: usize) -> Vec<WidgetId> {
        ["A", "B", "C", "D", "E", "F"][..n]
            .iter()
            .map(|l| {
                tree.find_by_label(l)
                    .unwrap_or_else(|| panic!("tile {l} not found"))
            })
            .collect()
    }

    #[test]
    fn click_selects_tile_and_deselects_siblings() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(group_with(
            selected.clone(),
            TileLayout::Row,
            &["one", "two", "three"],
        ));
        tree.layout(SizeProposal::exact(600.0, 300.0));
        let ids = tile_ids(&tree, 3);

        assert_eq!(selected.get(), 0);
        tree.click(ids[1]);
        assert_eq!(selected.get(), 1);
        tree.click(ids[2]);
        assert_eq!(selected.get(), 2);
    }

    #[test]
    fn roving_arrows_move_selection_and_wrap() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let g = tree.add(group_with(
            selected.clone(),
            TileLayout::Row,
            &["one", "two", "three"],
        ));
        tree.layout(SizeProposal::exact(600.0, 300.0));

        tree.focus(g);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 1);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 2);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 0, "wraps around");
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(selected.get(), 2, "wraps backwards");
        tree.press_key(Key::End, Modifiers::NONE);
        assert_eq!(selected.get(), 2);
        tree.press_key(Key::Home, Modifiers::NONE);
        assert_eq!(selected.get(), 0);
    }

    #[test]
    fn roving_skips_disabled_tile() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let g = tree.add(
            RadioTileGroup::new(selected.clone())
                .layout(TileLayout::Row)
                .tile(RadioTile::new().title(lit!("A")))
                .tile(RadioTile::new().title(lit!("B")).enabled(false))
                .tile(RadioTile::new().title(lit!("C"))),
        );
        tree.layout(SizeProposal::exact(600.0, 300.0));
        tree.focus(g);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(
            selected.get(),
            2,
            "ArrowRight skips the disabled middle tile"
        );
    }

    #[test]
    fn row_layout_equalizes_width_and_height() {
        // Tiles carry very different description lengths → different natural
        // heights. A Row must give them equal width AND equal height.
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(group_with(
            selected,
            TileLayout::Row,
            &[
                "short",
                "a considerably longer description that will wrap across several lines in the tile",
                "medium length text here",
            ],
        ));
        tree.layout(SizeProposal::exact(600.0, 400.0));
        let ids = tile_ids(&tree, 3);
        let b0 = tree.bounds(ids[0]);
        let b1 = tree.bounds(ids[1]);
        let b2 = tree.bounds(ids[2]);

        assert!((b0.width - b1.width).abs() < 0.5, "equal widths");
        assert!((b1.width - b2.width).abs() < 0.5, "equal widths");
        assert!(
            (b0.height - b1.height).abs() < 0.5,
            "equal heights despite different content"
        );
        assert!(
            (b1.height - b2.height).abs() < 0.5,
            "equal heights despite different content"
        );
        assert!(b1.height > 0.0);
    }

    #[test]
    fn column_layout_gives_full_width_tiles() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(group_with(selected, TileLayout::Column, &["one", "two"]));
        tree.layout(SizeProposal::exact(500.0, 400.0));
        let ids = tile_ids(&tree, 2);
        // Full width (minus the focus-ring envelope), equal, and stacked.
        assert!((tree.bounds(ids[0]).width - tree.bounds(ids[1]).width).abs() < 0.5);
        assert!(tree.bounds(ids[0]).width > 480.0);
        assert!(tree.bounds(ids[1]).y > tree.bounds(ids[0]).y);
    }

    #[test]
    fn grid_layout_wraps_into_expected_columns() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        // 4 tiles, min 200 wide, 640 available, 12 gap → 3 columns (3*200+2*12=624<=640),
        // so the 4th tile wraps to a second row below tile 0.
        tree.add(group_with(
            selected,
            TileLayout::Grid {
                min_tile_width: 200.0,
            },
            &["one", "two", "three", "four"],
        ));
        tree.layout(SizeProposal::exact(640.0, 600.0));
        let ids = tile_ids(&tree, 4);
        let b0 = tree.bounds(ids[0]);
        let b3 = tree.bounds(ids[3]);
        // Tile 3 wraps under tile 0 (same column, lower row).
        assert!(b3.y > b0.y, "4th tile is on a second row");
        assert!(
            (b3.x - b0.x).abs() < 0.5,
            "4th tile aligns under the first column"
        );
    }

    #[test]
    fn vertical_layout_is_compact_full_width_list() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let g = tree.add(
            RadioTileGroup::new(selected.clone())
                .layout(TileLayout::Vertical)
                .tile(
                    RadioTile::new()
                        .title(lit!("None"))
                        .trailing(lit!("empty binder")),
                )
                .tile(
                    RadioTile::new()
                        .title(lit!("Novel"))
                        .trailing(lit!("20 chapters")),
                )
                .tile(
                    RadioTile::new()
                        .title(lit!("Notebook"))
                        .trailing(lit!("free-form notes")),
                ),
        );
        tree.layout(SizeProposal::exact(500.0, 400.0));
        let none = tree.find_by_label("None").unwrap();
        let novel = tree.find_by_label("Novel").unwrap();
        // Full-width rows (minus the envelope), equal, stacked, each the
        // theme's fixed compact height.
        assert!((tree.bounds(none).width - tree.bounds(novel).width).abs() < 0.5);
        assert!(tree.bounds(none).width > 480.0);
        assert!((tree.bounds(none).height - RADIO_TILE_VERTICAL_ROW_HEIGHT).abs() < 0.5);
        assert!((tree.bounds(novel).height - RADIO_TILE_VERTICAL_ROW_HEIGHT).abs() < 0.5);
        assert!(tree.bounds(novel).y > tree.bounds(none).y);
        // Roving works vertically.
        tree.focus(g);
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(selected.get(), 1);
        // Each row is still a RadioButton.
        assert_eq!(
            tree.accessibility_node(none).role(),
            bastyde_core::accesskit::Role::RadioButton
        );
    }

    #[test]
    fn vertical_row_height_override_wins() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            RadioTileGroup::new(selected)
                .layout(TileLayout::Vertical)
                .row_height(40.0)
                .tile(RadioTile::new().title(lit!("A")))
                .tile(RadioTile::new().title(lit!("B"))),
        );
        tree.layout(SizeProposal::exact(400.0, 400.0));
        let a = tree.find_by_label("A").unwrap();
        assert!((tree.bounds(a).height - 40.0).abs() < 0.5);
    }

    #[test]
    fn keyboard_focus_adds_one_group_ring() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let g = tree.add(group_with(selected, TileLayout::Row, &["one", "two"]));
        tree.layout(SizeProposal::exact(600.0, 300.0));

        // Not focused (mouse modality) → no group ring, only the two tile
        // borders.
        let base = tree
            .render()
            .shapes
            .iter()
            .filter(|s| s.stroke_width > 0.0)
            .count();

        // Keyboard focus (Tab / arrow) → exactly one extra stroke: the
        // whole-group focus ring.
        tree.focus(g);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        let with_ring = tree
            .render()
            .shapes
            .iter()
            .filter(|s| s.stroke_width > 0.0)
            .count();
        assert_eq!(
            with_ring,
            base + 1,
            "keyboard focus draws exactly one whole-group ring"
        );
    }

    #[test]
    fn accessibility_group_and_tiles() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let g = tree.add(
            group_with(selected, TileLayout::Row, &["one", "two", "three"]).label(lit!("Format")),
        );
        tree.layout(SizeProposal::exact(600.0, 300.0));

        let ginfo = tree.accessibility_node(g);
        assert_eq!(ginfo.role(), bastyde_core::accesskit::Role::RadioGroup);
        assert_eq!(ginfo.name(), Some("Format"));

        let ids = tile_ids(&tree, 3);
        assert_eq!(
            tree.accessibility_node(ids[0]).role(),
            bastyde_core::accesskit::Role::RadioButton
        );
        assert!(!tree.accessibility_node(ids[0]).is_toggled());
        assert!(
            tree.accessibility_node(ids[1]).is_toggled(),
            "selected tile is toggled"
        );
        assert!(!tree.accessibility_node(ids[2]).is_toggled());
    }

    #[test]
    fn toggled_updates_after_keyboard_selection() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let g = tree.add(group_with(selected, TileLayout::Row, &["one", "two"]));
        tree.layout(SizeProposal::exact(600.0, 300.0));
        let ids = tile_ids(&tree, 2);
        assert!(tree.accessibility_node(ids[0]).is_toggled());

        tree.focus(g);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        // AccessibilityOnly binding must have re-walked the AT tree.
        assert!(!tree.accessibility_node(ids[0]).is_toggled());
        assert!(tree.accessibility_node(ids[1]).is_toggled());
    }
}
