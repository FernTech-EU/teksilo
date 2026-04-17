//! Intermediate representation for the `fern!` DSL.
//!
//! The parser builds an IR tree; the lowering walks the tree and emits
//! the final builder-call token stream. Keeping parse and lower separate
//! keeps span handling predictable: each IR node carries the span of the
//! user token it originated from.

use proc_macro2::Span;
use syn::{Block, Expr, Ident, Local, Pat, Path};

/// The root of a `fern!` invocation.
pub(crate) struct FernRoot {
    /// If `Some(ident)`, the macro was called as `fern!(ident => ...)`
    /// and expansion should wrap the root in `ident.add(...)` to return
    /// a `WidgetId`. If `None`, expansion returns the widget value
    /// directly.
    pub(crate) ctx: Option<Ident>,
    pub(crate) root: FernElement,
}

/// An element: `Type[::ctor](args...) { body }`.
pub(crate) struct FernElement {
    /// The full callable path the user wrote. `Button("x")` stores
    /// `Button`; `Button::new_literal("x")` stores the whole
    /// `Button::new_literal` path. Lowering appends `::new` only when
    /// `has_explicit_ctor` is false.
    pub(crate) type_path: Path,
    /// True when the user named a constructor explicitly (a lowercase
    /// last path segment, per Rust naming convention). Lowering then
    /// calls the path as-is without appending `::new`.
    pub(crate) has_explicit_ctor: bool,
    /// Positional arguments between the parens after the type path.
    /// Empty when the user wrote `VStack` with no parens (equivalent to
    /// `VStack()`).
    pub(crate) args: Vec<Expr>,
    /// Body items in source order.
    pub(crate) body: Vec<BodyItem>,
    /// Span of the type path's first segment — used for error reporting
    /// on constructor typos.
    pub(crate) head_span: Span,
}

/// One item in an element's body block.
pub(crate) enum BodyItem {
    /// `name: arg1, arg2, ...` — builder method call with N args.
    /// A bare lowercase ident with no body is modeled as `args == []`.
    Property(FernProperty),
    /// A bare element at body position — attaches via `.child(...)`.
    Child(FernElement),
    /// `name = Element` — a binding that hoists `let name = ctx.add(...)`
    /// to the enclosing statement-forming block, then attaches via
    /// `.add_child(name)` on the parent (spec §3.3).
    Binding { name: Ident, element: FernElement },
    /// `#{ expr }` at body position — the expr is expected to evaluate
    /// to a `WidgetId` and attaches via `.add_child(expr)` (spec §6.1).
    /// Phase 2 keeps the semantics simple: always WidgetId. The full
    /// `IntoFernChild` routing (widget-or-id dispatch) is Phase 3.
    Escape { expr: Expr, span: Span },
    /// `let pat = expr;` at body position — spec §5.4. Introduces a
    /// local whose value is used by subsequent body items. Triggers
    /// statement-sequence lowering on the enclosing element.
    Let(Local),
    /// `rust { ... }` imperative escape — spec §5.6. Two forms:
    /// expression-producing (block tail has no semicolon, lowered as
    /// `.child(block)`) and side-effect (block tail ends in `;`,
    /// emitted inline as a side-effect statement).
    Rust { block: Block, span: Span, shape: RustShape },
    /// `if cond { Element } [else if cond { Element }]* [else { Element }]?`
    /// — spec §5.1. Lowers to `.child_opt(...)` (no-else) or
    /// `.child(FernBranch{N}::...)` (with else branches).
    If(FernIf),
    /// `match expr { pat => Element, ... }` — spec §5.3. Lowers to
    /// `.child(match ... { ... FernBranch{N}::... })` with arms
    /// dispatched by variant index.
    Match(FernMatch),
    /// `for pat in iter { Element }` — spec §5.2. Lowers to
    /// `.children(iter.map(|pat| Element))`.
    For(FernFor),
    /// `..expr` — spec §5.5. Inlines an iterator of `WidgetId`s as
    /// children. Forces statement-sequence lowering on the parent.
    Spread { expr: Expr, span: Span },
}

pub(crate) struct FernIf {
    pub(crate) cond: Expr,
    pub(crate) then: FernElement,
    pub(crate) else_branch: Option<Box<FernElse>>,
    pub(crate) span: Span,
}

pub(crate) enum FernElse {
    ElseIf(FernIf),
    Element(FernElement),
}

pub(crate) struct FernMatch {
    pub(crate) scrutinee: Expr,
    pub(crate) arms: Vec<FernMatchArm>,
    pub(crate) span: Span,
}

pub(crate) struct FernMatchArm {
    pub(crate) pat: Pat,
    pub(crate) guard: Option<(syn::Token![if], Expr)>,
    pub(crate) element: FernElement,
}

pub(crate) struct FernFor {
    pub(crate) pat: Pat,
    pub(crate) iter: Expr,
    pub(crate) lets: Vec<Local>,
    pub(crate) element: FernElement,
    pub(crate) span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RustShape {
    /// Block's last statement is an expression without trailing `;` —
    /// the block's value becomes a child via `.child(block)`.
    Expression,
    /// Block has a unit tail (last statement ends in `;` or is a
    /// `Stmt::Semi`) — emitted as a side-effect statement.
    SideEffect,
}

pub(crate) struct FernProperty {
    pub(crate) name: Ident,
    pub(crate) args: Vec<PropArg>,
}

/// A property argument. `Expr` and `Element` both emit `.prop_name(...)`;
/// `Escape` and `Binding` force the `_id` slot suffix per spec §A.3 and
/// hoist the binding when present.
pub(crate) enum PropArg {
    /// A plain Rust expression (scalars, closures, method calls).
    Expr(Expr),
    /// An embedded fern element — `tab_literal: "name", Card { ... }`.
    Element(FernElement),
    /// `#{ expr }` — a WidgetId expression that routes to `.prop_id`.
    Escape(Expr),
    /// `name = Element` — hoists `let name = ctx.add(...)` and routes
    /// to `.prop_id(name)`.
    Binding { name: Ident, element: FernElement },
}
