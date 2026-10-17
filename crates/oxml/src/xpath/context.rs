// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! What an evaluation is given beyond the document: variable
//! bindings, and the errors the fallible entry points report.

use alloc::string::String;
use alloc::vec::Vec;

use super::Value;
use crate::tree::ExpandedName;

/// Resource bounds for one fallible evaluation.
///
/// Work units are conservative charges for expression operations,
/// traversals and byte processing, not elapsed time. String bytes are
/// cumulative reservations across temporary values; some operations
/// reserve upper bounds before running. Node limits include arena slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct EvalLimits {
    /// Maximum work units spent by an evaluation.
    pub max_work: usize,
    /// Maximum arena slots or nodes in an intermediate node-set.
    pub max_nodes: usize,
    /// Maximum total UTF-8 bytes materialized as strings.
    pub max_string_bytes: usize,
}

impl Default for EvalLimits {
    fn default() -> Self {
        Self {
            max_work: 10_000_000,
            max_nodes: 100_000,
            max_string_bytes: 1024 * 1024,
        }
    }
}

/// Variable bindings for [`XPath::evaluate_with`](super::XPath::evaluate_with).
///
/// `XPath` 1.0 has variable references, `$name`, bound by the
/// evaluation context rather than by the expression. A binding is a
/// [`Value`] of any of the four types; a node-set bound here takes
/// predicates and steps like any other primary expression.
///
/// ```
/// use oxml::xpath::{EvalContext, Value};
/// use oxml::{ExpandedName, XPath, parse};
///
/// let doc = parse("<r><a>1</a><a>2</a></r>").unwrap();
/// let xp = XPath::compile("count(//a[. > $min])").unwrap();
/// let ctx = EvalContext::new()
///     .bind(ExpandedName::local("min"), Value::Number(1.0));
/// assert_eq!(xp.evaluate_with(&doc, &ctx).unwrap().to_str(&doc), "1");
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EvalContext {
    variables: Vec<(ExpandedName, Value)>,
    pub(crate) limits: Option<EvalLimits>,
}

impl EvalContext {
    /// A context with nothing bound.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bound work and allocation during [`XPath::evaluate_with`](super::XPath::evaluate_with).
    ///
    /// Each evaluation starts with fresh budgets. Existing contexts
    /// remain unbounded unless this method is called.
    #[must_use]
    pub fn with_limits(mut self, limits: EvalLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Bind `name` to `value`, replacing any earlier binding of it.
    ///
    /// A prefixed variable in an expression, `$p:x`, resolves its
    /// prefix against the bindings the expression was compiled with,
    /// so the name to bind here is the expanded one:
    /// `ExpandedName::qualified("urn:u", "x")`.
    #[must_use]
    pub fn bind(mut self, name: ExpandedName, value: Value) -> Self {
        match self.variables.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => self.variables.push((name, value)),
        }
        self
    }

    /// Number of names inspected by a variable lookup in the worst case.
    pub(crate) fn binding_count(&self) -> usize {
        self.variables.len()
    }

    /// The value bound to `name`, if any.
    #[must_use]
    pub fn variable(&self, name: &ExpandedName) -> Option<&Value> {
        self.variables
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }
}

/// Why [`XPath::evaluate_with`](super::XPath::evaluate_with) could not produce a value.
///
/// `XPath` 1.0 has no exceptions, and the infallible entry points
/// honour that: [`XPath::evaluate`](super::XPath::evaluate) always answers. These are the
/// conditions under which the answer is not meaningful, reported by
/// the fallible entry points instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// The expression references a variable the context does not
    /// bind. The name is the local part, or `{uri}local` for a
    /// namespaced variable.
    UnboundVariable(String),
    /// A function or operation that requires a node-set was given
    /// something else: `count('x')`, `sum(1)`, `(1)[1]`. The
    /// specification calls this an error; [`XPath::evaluate`](super::XPath::evaluate) answers
    /// `0`, the empty string or the empty node-set instead.
    TypeError(String),
    /// The context node handed to [`XPath::evaluate_from_with`](super::XPath::evaluate_from_with) no
    /// longer belongs to the document: it was removed, and its slot
    /// may since have been reused.
    Stale,
    /// An evaluation exhausted a work, node or string-byte budget.
    LimitExceeded(&'static str),
}

impl core::fmt::Display for EvalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnboundVariable(name) => {
                write!(f, "unbound variable `${name}`")
            }
            Self::TypeError(message) => f.write_str(message),
            Self::Stale => f.write_str("the context node has been removed"),
            Self::LimitExceeded(resource) => {
                write!(f, "XPath evaluation exceeds the {resource} limit")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for EvalError {}
