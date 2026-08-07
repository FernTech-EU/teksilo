// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Intermediate representation for the `teksu!` DSL.
//!
//! The parser builds an IR tree; the lowering walks the tree and emits
//! the final builder-call token stream. Keeping parse and lower separate
//! keeps span handling predictable: each IR node carries the span of the
//! user token it originated from.

use proc_macro2::Span;
use syn::{Block, Expr, Ident, Local, Pat, Path};

/// The root of a `teksu!` invocation.
pub struct TeksiRoot {
    /// If `Some(ident)`, the macro was called as `teksu!(ident => ...)`
    /// and expansion should wrap the root in `ident.add(...)` to return
    /// a `WidgetId`. If `None`, expansion returns the widget value
    /// directly.
    pub ctx: Option<Ident>,
    pub root: TeksiElement,
}

/// An element: `Type[::ctor](args...) { body }`.
pub struct TeksiElement {
    /// The full callable path the user wrote. `Button("x")` stores
    /// `Button`; `Button::new(lit!("x"))` stores the whole
    /// `Button::new_literal` path. Lowering appends `::new` only when
    /// `has_explicit_ctor` is false.
    pub type_path: Path,
    /// True when the user named a constructor explicitly (a lowercase
    /// last path segment, per Rust naming convention). Lowering then
    /// calls the path as-is without appending `::new`.
    pub has_explicit_ctor: bool,
    /// Positional arguments between the parens after the type path.
    /// Empty when the user wrote `VStack` with no parens (equivalent to
    /// `VStack()`).
    pub args: Vec<Expr>,
    /// Body items in source order.
    pub body: Vec<BodyItem>,
    /// Span of the type path's first segment — used for error reporting
    /// on constructor typos.
    pub head_span: Span,
    /// Span of the closing `)` after the args, if the user wrote any
    /// args parens (including empty `()`). `None` when no arg parens
    /// were written. Consumers (e.g. the formatter) use this to find
    /// the exact byte offset just past the args.
    pub args_close: Option<Span>,
    /// Span of the closing `}` of the body, if the user wrote any body
    /// braces (including empty `{}`). `None` when no body braces were
    /// written. Consumers use this to find the exact end of the body
    /// for trivia attribution.
    pub body_close: Option<Span>,
}

/// One item in an element's body block.
#[allow(clippy::large_enum_variant)]
pub enum BodyItem {
    /// `name: arg1, arg2, ...` — builder method call with N args.
    /// A bare lowercase ident with no body is modeled as `args == []`.
    Property(TeksiProperty),
    /// A bare element at body position — attaches via `.child(...)`.
    Child(TeksiElement),
    /// `name = Element` — a binding that hoists `let name = ctx.add(...)`
    /// to the enclosing statement-forming block, then attaches via
    /// `.add_child(name)` on the parent.
    Binding { name: Ident, element: TeksiElement },
    /// `#{ expr }` at body position — the expr is expected to evaluate
    /// to a `WidgetId` and attaches via `.add_child(expr)`.
    /// The semantics are simple: always WidgetId. The full
    /// `IntoTeksiChild` routing (widget-or-id dispatch) is not yet implemented.
    Escape { expr: Expr, span: Span },
    /// `let pat = expr;` at body position. Introduces a
    /// local whose value is used by subsequent body items. Triggers
    /// statement-sequence lowering on the enclosing element.
    Let(Local),
    /// `rust { ... }` imperative escape. Two forms:
    /// expression-producing (block tail has no semicolon, lowered as
    /// `.child(block)`) and side-effect (block tail ends in `;`,
    /// emitted inline as a side-effect statement).
    Rust {
        block: Block,
        span: Span,
        shape: RustShape,
    },
    /// `if cond { Element } [else if cond { Element }]* [else { Element }]?`
    /// Lowers to `.child_opt(...)` (no-else) or
    /// `.child(TeksiBranch{N}::...)` (with else branches).
    If(TeksiIf),
    /// `match expr { pat => Element, ... }`. Lowers to
    /// `.child(match ... { ... TeksiBranch{N}::... })` with arms
    /// dispatched by variant index.
    Match(TeksiMatch),
    /// `for pat in iter { Element }`. Lowers to
    /// `.children(iter.map(|pat| Element))`.
    For(TeksiFor),
    /// `..expr` — inlines an iterator of `WidgetId`s as
    /// children. Forces statement-sequence lowering on the parent.
    Spread { expr: Expr, span: Span },
}

pub struct TeksiIf {
    pub cond: Expr,
    pub then: TeksiElement,
    pub else_branch: Option<Box<TeksiElse>>,
    pub span: Span,
    /// Span of the `}` that closes this if's `then` block. Held by every
    /// `TeksiIf` (including else-if recursion) so consumers can compute
    /// the rightmost byte of an if-chain. Inner `TeksiElement`s carry no
    /// braces of their own — the structural form owns them.
    pub body_close: Span,
}

#[allow(clippy::large_enum_variant)]
pub enum TeksiElse {
    ElseIf(TeksiIf),
    Element {
        element: TeksiElement,
        /// Span of the `}` that closes the trailing `else { ... }` block.
        body_close: Span,
    },
}

pub struct TeksiMatch {
    pub scrutinee: Expr,
    pub arms: Vec<TeksiMatchArm>,
    pub span: Span,
    /// Span of the `}` that closes the match block.
    pub body_close: Span,
}

pub struct TeksiMatchArm {
    pub pat: Pat,
    pub guard: Option<(syn::Token![if], Expr)>,
    pub element: TeksiElement,
}

pub struct TeksiFor {
    pub pat: Pat,
    pub iter: Expr,
    pub lets: Vec<Local>,
    pub element: TeksiElement,
    pub span: Span,
    /// Span of the `}` that closes the for block.
    pub body_close: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RustShape {
    /// Block's last statement is an expression without trailing `;` —
    /// the block's value becomes a child via `.child(block)`.
    Expression,
    /// Block has a unit tail (last statement ends in `;` or is a
    /// `Stmt::Semi`) — emitted as a side-effect statement.
    SideEffect,
}

pub struct TeksiProperty {
    pub name: Ident,
    pub args: Vec<PropArg>,
}

/// A property argument. `Expr` and `Element` both emit `.prop_name(...)`;
/// `Escape` and `Binding` force the `_id` slot suffix per spec §A.3 and
/// hoist the binding when present.
pub enum PropArg {
    /// A plain Rust expression (scalars, closures, method calls).
    Expr(Expr),
    /// An embedded teksu element — `tab: "name", Card { ... }`.
    Element(TeksiElement),
    /// `#{ expr }` — a WidgetId expression that routes to `.prop_id`.
    Escape(Expr),
    /// `name = Element` — hoists `let name = ctx.add(...)` and routes
    /// to `.prop_id(name)`.
    Binding { name: Ident, element: TeksiElement },
}
