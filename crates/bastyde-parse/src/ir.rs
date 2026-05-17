//! Intermediate representation for the `bati!` DSL.
//!
//! The parser builds an IR tree; the lowering walks the tree and emits
//! the final builder-call token stream. Keeping parse and lower separate
//! keeps span handling predictable: each IR node carries the span of the
//! user token it originated from.

use proc_macro2::Span;
use syn::{Block, Expr, Ident, Local, Pat, Path};

/// The root of a `bati!` invocation.
pub struct BatiRoot {
    /// If `Some(ident)`, the macro was called as `bati!(ident => ...)`
    /// and expansion should wrap the root in `ident.add(...)` to return
    /// a `WidgetId`. If `None`, expansion returns the widget value
    /// directly.
    pub ctx: Option<Ident>,
    pub root: BatiElement,
}

/// An element: `Type[::ctor](args...) { body }`.
pub struct BatiElement {
    /// The full callable path the user wrote. `Button("x")` stores
    /// `Button`; `Button::new_literal("x")` stores the whole
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
    Property(BatiProperty),
    /// A bare element at body position — attaches via `.child(...)`.
    Child(BatiElement),
    /// `name = Element` — a binding that hoists `let name = ctx.add(...)`
    /// to the enclosing statement-forming block, then attaches via
    /// `.add_child(name)` on the parent (spec §3.3).
    Binding { name: Ident, element: BatiElement },
    /// `#{ expr }` at body position — the expr is expected to evaluate
    /// to a `WidgetId` and attaches via `.add_child(expr)` (spec §6.1).
    /// Phase 2 keeps the semantics simple: always WidgetId. The full
    /// `IntoBatiChild` routing (widget-or-id dispatch) is Phase 3.
    Escape { expr: Expr, span: Span },
    /// `let pat = expr;` at body position — spec §5.4. Introduces a
    /// local whose value is used by subsequent body items. Triggers
    /// statement-sequence lowering on the enclosing element.
    Let(Local),
    /// `rust { ... }` imperative escape — spec §5.6. Two forms:
    /// expression-producing (block tail has no semicolon, lowered as
    /// `.child(block)`) and side-effect (block tail ends in `;`,
    /// emitted inline as a side-effect statement).
    Rust {
        block: Block,
        span: Span,
        shape: RustShape,
    },
    /// `if cond { Element } [else if cond { Element }]* [else { Element }]?`
    /// — spec §5.1. Lowers to `.child_opt(...)` (no-else) or
    /// `.child(BatiBranch{N}::...)` (with else branches).
    If(BatiIf),
    /// `match expr { pat => Element, ... }` — spec §5.3. Lowers to
    /// `.child(match ... { ... BatiBranch{N}::... })` with arms
    /// dispatched by variant index.
    Match(BatiMatch),
    /// `for pat in iter { Element }` — spec §5.2. Lowers to
    /// `.children(iter.map(|pat| Element))`.
    For(BatiFor),
    /// `..expr` — spec §5.5. Inlines an iterator of `WidgetId`s as
    /// children. Forces statement-sequence lowering on the parent.
    Spread { expr: Expr, span: Span },
}

pub struct BatiIf {
    pub cond: Expr,
    pub then: BatiElement,
    pub else_branch: Option<Box<BatiElse>>,
    pub span: Span,
    /// Span of the `}` that closes this if's `then` block. Held by every
    /// `BatiIf` (including else-if recursion) so consumers can compute
    /// the rightmost byte of an if-chain. Inner `BatiElement`s carry no
    /// braces of their own — the structural form owns them.
    pub body_close: Span,
}

#[allow(clippy::large_enum_variant)]
pub enum BatiElse {
    ElseIf(BatiIf),
    Element {
        element: BatiElement,
        /// Span of the `}` that closes the trailing `else { ... }` block.
        body_close: Span,
    },
}

pub struct BatiMatch {
    pub scrutinee: Expr,
    pub arms: Vec<BatiMatchArm>,
    pub span: Span,
    /// Span of the `}` that closes the match block.
    pub body_close: Span,
}

pub struct BatiMatchArm {
    pub pat: Pat,
    pub guard: Option<(syn::Token![if], Expr)>,
    pub element: BatiElement,
}

pub struct BatiFor {
    pub pat: Pat,
    pub iter: Expr,
    pub lets: Vec<Local>,
    pub element: BatiElement,
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

pub struct BatiProperty {
    pub name: Ident,
    pub args: Vec<PropArg>,
}

/// A property argument. `Expr` and `Element` both emit `.prop_name(...)`;
/// `Escape` and `Binding` force the `_id` slot suffix per spec §A.3 and
/// hoist the binding when present.
pub enum PropArg {
    /// A plain Rust expression (scalars, closures, method calls).
    Expr(Expr),
    /// An embedded bati element — `tab_literal: "name", Card { ... }`.
    Element(BatiElement),
    /// `#{ expr }` — a WidgetId expression that routes to `.prop_id`.
    Escape(Expr),
    /// `name = Element` — hoists `let name = ctx.add(...)` and routes
    /// to `.prop_id(name)`.
    Binding { name: Ident, element: BatiElement },
}
