// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The four `XPath` 1.0 value types and the conversions between them.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::compare::{ieee_eq, xpath_number};
use super::float;
use crate::tree::{Document, NodeId, NodeKind};

/// A value produced by evaluating an expression.
///
/// `XPath` 1.0 has exactly four types, and the conversions between them
/// are specified rather than intuitive — `boolean(node-set)` is
/// "non-empty", `number("abc")` is `NaN`. Modelling them explicitly
/// keeps those rules in one place.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A set of nodes, in document order, without duplicates.
    NodeSet(Vec<NodeId>),
    /// A string.
    String(String),
    /// A number. May be `NaN`.
    Number(f64),
    /// A boolean.
    Boolean(bool),
}

impl Value {
    /// Convert to a boolean, per `XPath`'s `boolean()`.
    #[must_use]
    pub fn to_boolean(&self) -> bool {
        match self {
            Self::Boolean(b) => *b,
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::String(s) => !s.is_empty(),
            Self::NodeSet(n) => !n.is_empty(),
        }
    }

    /// Convert to a number, per `XPath`'s `number()`.
    ///
    /// A string that is not a number becomes `NaN` rather than an
    /// error — that is what the specification requires, and callers
    /// relying on an error would silently get different results from
    /// every other `XPath` engine. "A number" means the `XPath` 1.0
    /// `Number` production, optionally negated and surrounded by XML
    /// whitespace: `'1e5'`, `'+1'` and `'Infinity'` are all `NaN`.
    #[must_use]
    pub fn to_number(&self, doc: &Document) -> f64 {
        match self {
            Self::Number(n) => *n,
            Self::Boolean(b) => f64::from(u8::from(*b)),
            Self::String(s) => xpath_number(s),
            Self::NodeSet(_) => xpath_number(&self.to_str(doc)),
        }
    }

    /// Convert to a string, per `XPath`'s `string()`.
    ///
    /// For a node-set this is the string-value of the *first* node in
    /// document order, not a concatenation of all of them.
    #[must_use]
    pub fn to_str(&self, doc: &Document) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Boolean(b) => {
                if *b {
                    "true".to_owned()
                } else {
                    "false".to_owned()
                }
            }
            Self::Number(n) => format_number(*n),
            Self::NodeSet(nodes) => nodes
                .first()
                .map(|id| string_value(doc, *id))
                .unwrap_or_default(),
        }
    }

    /// The node-set, if this is one.
    #[must_use]
    pub fn nodes(&self) -> Option<&[NodeId]> {
        match self {
            Self::NodeSet(n) => Some(n),
            _ => None,
        }
    }

    /// The type's name as the specification writes it, for messages.
    pub(crate) const fn type_name(&self) -> &'static str {
        match self {
            Self::NodeSet(_) => "node-set",
            Self::String(_) => "string",
            Self::Number(_) => "number",
            Self::Boolean(_) => "boolean",
        }
    }
}

/// Format a number the way `XPath`'s `string()` does.
///
/// Three things this gets right that Rust's default does not:
///
/// - `1.0` prints as `1`, never `1.0`.
/// - There is no exponent form.
/// - Fractions are rendered at 15 significant digits, then trimmed.
///
/// That last point is a deliberate departure from Rust's shortest
/// round-trip formatting. `sum()` over `9.99` and `7.50` produces the
/// f64 nearest `17.490000000000002`, and printing every digit needed
/// to distinguish that value is what the specification's wording
/// literally asks for. No other engine does it: libxml2, Xalan and
/// Saxon all print `17.49`, because 15 significant digits is the
/// point past which IEEE 754 noise starts showing. Matching them
/// matters more than matching the letter of a sentence written before
/// shortest-round-trip printing existed.
fn format_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    // `n == trunc(n)` is an exact integrality test, which is the one
    // case where comparing floats for equality is right rather than
    // sloppy: a value either is its own truncation or it is not, and
    // an epsilon here would misclassify values near an integer.
    if ieee_eq(n, float::trunc(n)) && n.abs() < 1e21 {
        return format!("{}", n as i64);
    }
    // Round to 15 significant figures, then let Rust print the
    // shortest form of *that*. That is what drops the trailing IEEE 754
    // noise; formatting to a fixed number of *decimal places* would
    // not, because the noise sits at a different decimal position
    // depending on magnitude.
    //
    // This is done through scientific notation rather than by scaling
    // with `log10`/`powf`. Rust does not specify the precision of those
    // two — Miri's implementation and `libm`'s disagree with the host's
    // by a few ULP on values as ordinary as `17.49`. Since `magnitude`
    // is fed to `floor`, a 1-ULP difference near an exact power of ten
    // flips it to the next integer, changing the digit position that
    // gets rounded and therefore the printed result. A `no_std` build
    // would then print a different number from a `std` build.
    //
    // `{:.14e}` is exact decimal conversion: 15 significant digits,
    // identical on every platform, and it needs no transcendental
    // functions at all.
    let rounded: f64 = format!("{n:.14e}").parse().unwrap_or(n);
    let mut s = rounded.to_string();
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    s
}

/// The string-value of a node.
pub(crate) fn string_value(doc: &Document, id: NodeId) -> String {
    match doc.kind(id) {
        Some(NodeKind::Attr(a)) => a.value.to_owned(),
        Some(NodeKind::Comment(t) | NodeKind::Text(t)) => t.to_owned(),
        Some(NodeKind::ProcessingInstruction { data, .. }) => data.to_owned(),
        Some(NodeKind::Namespace { uri, .. }) => uri.to_owned(),
        _ => {
            let mut text = String::new();
            for node in doc.descendants_of(id) {
                if let Some(NodeKind::Text(t)) = doc.kind(node) {
                    text.push_str(t);
                }
            }
            text
        }
    }
}
