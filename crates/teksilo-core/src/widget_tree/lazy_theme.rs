// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The theme the paint walk hands a node, resolved only when something paints.
//!
//! Most of a frame replays cached paint: a clean node merges its last frame
//! and reads no theme. The paint walk still passes through every node, so a
//! node carrying a theme override must not clone the theme and run its
//! override just for being passed through. A [`LazyTheme`] stands for the
//! theme at one point of the walk and works it out on the first call to
//! [`LazyTheme::get`], from its parent scope, so an override over a subtree
//! that is all cache hits costs nothing, and one over a subtree where
//! something repaints runs once, however many widgets below it repaint.
//!
//! One hook asks on every frame, painted or not: `after_paint`, which runs
//! after the subtree whether it repainted or replayed. A node in the
//! override's subtree that wants it (`Widget::wants_after_paint`) makes the
//! override resolve once per frame.
//!
//! The layout walk keeps resolving eagerly (`WidgetArena::theme_below`):
//! every node it visits is laid out and reads its theme.

use std::cell::OnceCell;

use crate::arena::WidgetArena;
use crate::styles::Theme;
use crate::widget_id::WidgetId;

/// The theme at one point of the paint walk, worked out on first use and
/// kept for the rest of the walk.
///
/// The walk makes one for its starting node ([`LazyTheme::entry`]) and one
/// for each node that carries an override ([`LazyTheme::below`]). A node
/// without an override hands its children its parent's scope as it is, so
/// asking for a theme climbs only through override nodes, each answered from
/// its cell once it has been filled.
pub(super) struct LazyTheme<'p> {
    inherited: Inherited<'p>,
    /// The node whose override applies on top of `inherited`.
    own_override: Option<WidgetId>,
    /// The theme, once someone asked for it and it had to be computed.
    resolved: OnceCell<Theme>,
}

enum Inherited<'p> {
    /// The window's theme, with no override above.
    Base(&'p Theme),
    /// What `parent` resolves to from `base`: a walk that starts below an
    /// override (overlay content hanging under its anchor).
    Ancestors { base: &'p Theme, parent: WidgetId },
    /// The scope of the nearest ancestor in this walk.
    Scope(&'p LazyTheme<'p>),
}

impl<'p> LazyTheme<'p> {
    /// The scope a walk starting at `id` begins from: what `id` inherits from
    /// its ancestors, without its own override.
    pub(super) fn entry(arena: &WidgetArena, id: WidgetId, base: &'p Theme) -> Self {
        let inherited = match arena.parent(id) {
            Some(parent) if arena.has_theme_override_above(id) => {
                Inherited::Ancestors { base, parent }
            }
            _ => Inherited::Base(base),
        };
        Self {
            inherited,
            own_override: None,
            resolved: OnceCell::new(),
        }
    }

    /// The scope for `id`, a node carrying an override, below `parent`.
    pub(super) fn below(parent: &'p LazyTheme<'p>, id: WidgetId) -> Self {
        Self {
            inherited: Inherited::Scope(parent),
            own_override: Some(id),
            resolved: OnceCell::new(),
        }
    }

    /// The theme for this point of the walk. Computes it the first time, and
    /// only if an override is involved: a scope with none lends the theme it
    /// inherits.
    pub(super) fn get<'s>(&'s self, arena: &WidgetArena) -> &'s Theme {
        match (&self.inherited, self.own_override) {
            (Inherited::Base(theme), None) => theme,
            (Inherited::Scope(parent), None) => parent.get(arena),
            _ => self.resolved.get_or_init(|| {
                let mut theme = match &self.inherited {
                    Inherited::Base(theme) => (*theme).clone(),
                    Inherited::Scope(parent) => parent.get(arena).clone(),
                    Inherited::Ancestors { base, parent } => {
                        arena.resolve_theme(*parent, base).into_owned()
                    }
                };
                if let Some(id) = self.own_override {
                    arena.apply_theme_override(id, &mut theme);
                }
                theme
            }),
        }
    }
}
