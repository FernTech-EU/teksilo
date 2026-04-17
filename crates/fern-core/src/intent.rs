//! Runtime intents dispatched by shortcuts and programmatic callers.
//!
//! An [`Intent`] is the unit of "something wants to happen" in the
//! action system. It carries a stable name (the intent string) and an
//! optional DTO of parameters. Intents are produced by
//! [`Shortcut`](crate::shortcut::Shortcut)s at activation time, by
//! widgets via `ctx.send_intent`, or by programmatic callers.
//!
//! They are dispatched through the widget tree by walking
//! **source-widget → root**: each ancestor's [`Action`](crate::action::Action)
//! whose `intent` name matches gets a chance to consume the intent or
//! propagate it.

use std::borrow::Cow;

/// A single parameter slot carried by [`IntentParams`].
///
/// Small closed value enum inspired by GTK's `GVariant`. Keeps
/// parameters stack-allocated and serializable without the type
/// erasure that a trait-object payload would require.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ActionArg {
    #[default]
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Cow<'static, str>),
}

impl ActionArg {
    pub fn as_bool(&self) -> Option<bool> {
        if let ActionArg::Bool(v) = self { Some(*v) } else { None }
    }

    pub fn as_int(&self) -> Option<i64> {
        if let ActionArg::Int(v) = self { Some(*v) } else { None }
    }

    pub fn as_float(&self) -> Option<f64> {
        if let ActionArg::Float(v) = self { Some(*v) } else { None }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let ActionArg::Str(v) = self { Some(v.as_ref()) } else { None }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, ActionArg::None)
    }
}

impl From<bool> for ActionArg {
    fn from(v: bool) -> Self {
        ActionArg::Bool(v)
    }
}

impl From<i64> for ActionArg {
    fn from(v: i64) -> Self {
        ActionArg::Int(v)
    }
}

impl From<i32> for ActionArg {
    fn from(v: i32) -> Self {
        ActionArg::Int(v as i64)
    }
}

impl From<f64> for ActionArg {
    fn from(v: f64) -> Self {
        ActionArg::Float(v)
    }
}

impl From<&'static str> for ActionArg {
    fn from(v: &'static str) -> Self {
        ActionArg::Str(Cow::Borrowed(v))
    }
}

impl From<String> for ActionArg {
    fn from(v: String) -> Self {
        ActionArg::Str(Cow::Owned(v))
    }
}

/// Fixed-arity positional parameter DTO for [`Intent`].
///
/// Four slots is an arbitrary but pragmatic upper bound: it covers
/// every parametric shortcut the reference apps (Qt, VSCode) bind in
/// practice without pushing to the heap. Unused slots default to
/// [`ActionArg::None`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IntentParams {
    pub p1: ActionArg,
    pub p2: ActionArg,
    pub p3: ActionArg,
    pub p4: ActionArg,
}

impl IntentParams {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with1(p1: impl Into<ActionArg>) -> Self {
        Self {
            p1: p1.into(),
            ..Self::default()
        }
    }

    pub fn with2(p1: impl Into<ActionArg>, p2: impl Into<ActionArg>) -> Self {
        Self {
            p1: p1.into(),
            p2: p2.into(),
            ..Self::default()
        }
    }

    pub fn with3(
        p1: impl Into<ActionArg>,
        p2: impl Into<ActionArg>,
        p3: impl Into<ActionArg>,
    ) -> Self {
        Self {
            p1: p1.into(),
            p2: p2.into(),
            p3: p3.into(),
            ..Self::default()
        }
    }

    pub fn with4(
        p1: impl Into<ActionArg>,
        p2: impl Into<ActionArg>,
        p3: impl Into<ActionArg>,
        p4: impl Into<ActionArg>,
    ) -> Self {
        Self {
            p1: p1.into(),
            p2: p2.into(),
            p3: p3.into(),
            p4: p4.into(),
        }
    }
}

/// A runtime intent dispatched through the widget tree.
///
/// See the module docs for the dispatch model.
#[derive(Debug, Clone, PartialEq)]
pub struct Intent {
    /// Stable intent name. Usually matches the originating
    /// [`Shortcut`](crate::shortcut::Shortcut)'s `intent_name()`.
    pub name: &'static str,
    /// Parameters. Empty for most intents (save, undo, copy, …).
    pub params: IntentParams,
}

impl Intent {
    /// A parameter-less intent.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            params: IntentParams::empty(),
        }
    }

    /// An intent with a pre-built parameter DTO.
    pub fn with_params(name: &'static str, params: IntentParams) -> Self {
        Self { name, params }
    }

    /// Shorthand for `Intent::with_params(name, IntentParams::with1(v))`.
    pub fn with1(name: &'static str, p1: impl Into<ActionArg>) -> Self {
        Self::with_params(name, IntentParams::with1(p1))
    }
}

/// Return value of an [`Action`](crate::action::Action) handler. Controls
/// whether the intent keeps bubbling up to ancestor widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IntentResponse {
    /// Intent was consumed; stop walking up the focus chain.
    #[default]
    Handled,
    /// Intent was observed but not consumed; continue walking up so
    /// ancestor widgets can also react.
    Propagated,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_arg_accessors() {
        assert_eq!(ActionArg::Bool(true).as_bool(), Some(true));
        assert_eq!(ActionArg::Int(42).as_int(), Some(42));
        assert_eq!(ActionArg::Float(1.5).as_float(), Some(1.5));
        assert_eq!(ActionArg::Str("hi".into()).as_str(), Some("hi"));
        assert!(ActionArg::None.is_none());
        assert!(ActionArg::Bool(true).as_int().is_none());
    }

    #[test]
    fn action_arg_from_impls() {
        let a: ActionArg = true.into();
        assert_eq!(a, ActionArg::Bool(true));
        let b: ActionArg = 7_i32.into();
        assert_eq!(b, ActionArg::Int(7));
        let c: ActionArg = 2.5_f64.into();
        assert_eq!(c, ActionArg::Float(2.5));
        let d: ActionArg = "hi".into();
        assert_eq!(d, ActionArg::Str(Cow::Borrowed("hi")));
    }

    #[test]
    fn intent_params_builders_fill_slots() {
        let p = IntentParams::with2(3_i32, "name");
        assert_eq!(p.p1, ActionArg::Int(3));
        assert_eq!(p.p2, ActionArg::Str(Cow::Borrowed("name")));
        assert_eq!(p.p3, ActionArg::None);
        assert_eq!(p.p4, ActionArg::None);
    }

    #[test]
    fn intent_shorthand() {
        let i = Intent::with1("tab.switch", 3_i32);
        assert_eq!(i.name, "tab.switch");
        assert_eq!(i.params.p1, ActionArg::Int(3));
    }

    #[test]
    fn default_response_is_handled() {
        assert_eq!(IntentResponse::default(), IntentResponse::Handled);
    }
}
