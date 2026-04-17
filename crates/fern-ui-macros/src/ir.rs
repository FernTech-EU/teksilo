//! Intermediate representation for the `fern!` DSL.
//!
//! The parser builds an IR tree; the lowering walks the tree and emits
//! the final builder-call token stream. Keeping parse and lower separate
//! keeps span handling predictable: each IR node carries the span of the
//! user token it originated from.

use proc_macro2::Span;
use syn::{Expr, Ident, Path};

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
}

pub(crate) struct FernProperty {
    pub(crate) name: Ident,
    pub(crate) args: Vec<Expr>,
}
