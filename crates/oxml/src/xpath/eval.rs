// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Evaluating a compiled `XPath` expression against a [`Document`].

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::ast::{Axis, BinaryOp, Expr, NodeTest, Step};
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
    /// every other `XPath` engine.
    #[must_use]
    pub fn to_number(&self, doc: &Document) -> f64 {
        match self {
            Self::Number(n) => *n,
            Self::Boolean(b) => f64::from(u8::from(*b)),
            Self::String(s) => s.trim().parse().unwrap_or(f64::NAN),
            Self::NodeSet(_) => {
                self.to_str(doc).trim().parse().unwrap_or(f64::NAN)
            }
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
    #[allow(clippy::float_cmp)]
    let is_integral = n == float::trunc(n);
    if is_integral && n.abs() < 1e21 {
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
fn string_value(doc: &Document, id: NodeId) -> String {
    match doc.kind(id) {
        Some(NodeKind::Attr(a)) => a.value.clone(),
        Some(NodeKind::Comment(t)) => t.clone(),
        Some(NodeKind::ProcessingInstruction { data, .. }) => data.clone(),
        _ => doc.text(id),
    }
}

/// Evaluate a compiled expression against a document.
#[must_use]
pub fn evaluate(doc: &Document, expr: &Expr, context: NodeId) -> Value {
    eval(doc, expr, context, 1, 1)
}

fn eval(
    doc: &Document,
    expr: &Expr,
    ctx: NodeId,
    position: usize,
    size: usize,
) -> Value {
    match expr {
        Expr::Literal(s) => Value::String(s.clone()),
        Expr::Number(n) => Value::Number(*n),
        Expr::Negate(inner) => {
            Value::Number(-eval(doc, inner, ctx, position, size).to_number(doc))
        }
        Expr::Path { absolute, steps } => {
            let start = if *absolute { doc.root() } else { ctx };
            Value::NodeSet(eval_path(doc, steps, start))
        }
        Expr::Binary { op, lhs, rhs } => {
            eval_binary(doc, *op, lhs, rhs, ctx, position, size)
        }
        Expr::Function { name, args } => {
            eval_function(doc, name, args, ctx, position, size)
        }
    }
}

fn eval_path(doc: &Document, steps: &[Step], start: NodeId) -> Vec<NodeId> {
    let mut current = alloc::vec![start];
    for step in steps {
        let mut next: Vec<NodeId> = Vec::new();
        for &node in &current {
            next.extend(
                axis_nodes(doc, node, step.axis)
                    .into_iter()
                    .filter(|&c| test_matches(doc, c, &step.test, step.axis)),
            );
        }
        // Deduplicate by sorting rather than scanning. A `contains`
        // check inside the loop above is O(n^2), which on `//title`
        // over a 2000-element document meant ~10ms for a query that
        // should take microseconds: the descendant axis produces tens
        // of thousands of candidates and each one rescanned the whole
        // accumulated set. Sorting is O(n log n), and node ids are
        // already document order, so this also establishes the order
        // XPath requires.
        next.sort_unstable();
        next.dedup();
        // Predicates see the node's position within this step's
        // result, which is why they are applied after the whole set
        // is gathered rather than inside the filter above.
        for pred in &step.predicates {
            let size = next.len();
            let mut kept = Vec::with_capacity(next.len());
            for (idx, &node) in next.iter().enumerate() {
                let v = eval(doc, pred, node, idx + 1, size);
                let keep = match v {
                    // A bare number predicate is a position test:
                    // `foo[1]`, not `foo[true()]`.
                    Value::Number(n) => {
                        (n - (idx + 1) as f64).abs() < f64::EPSILON
                    }
                    other => other.to_boolean(),
                };
                if keep {
                    kept.push(node);
                }
            }
            next = kept;
        }
        current = next;
    }
    current.sort_unstable();
    current.dedup();
    current
}

fn axis_nodes(doc: &Document, node: NodeId, axis: Axis) -> Vec<NodeId> {
    match axis {
        Axis::Child => doc.children(node).to_vec(),
        Axis::SelfAxis => alloc::vec![node],
        Axis::Parent => doc.parent(node).into_iter().collect(),
        Axis::Attribute => doc.attribute_nodes(node).to_vec(),
        Axis::Descendant => {
            let mut out = Vec::new();
            collect_descendants(doc, node, &mut out);
            out
        }
        Axis::DescendantOrSelf => {
            let mut out = alloc::vec![node];
            collect_descendants(doc, node, &mut out);
            out
        }
        Axis::Ancestor => {
            let mut out = Vec::new();
            let mut cur = doc.parent(node);
            while let Some(p) = cur {
                out.push(p);
                cur = doc.parent(p);
            }
            out
        }
        Axis::AncestorOrSelf => {
            let mut out = alloc::vec![node];
            let mut cur = doc.parent(node);
            while let Some(p) = cur {
                out.push(p);
                cur = doc.parent(p);
            }
            out
        }
        Axis::FollowingSibling | Axis::PrecedingSibling => {
            let Some(parent) = doc.parent(node) else {
                return Vec::new();
            };
            let sibs = doc.children(parent);
            let Some(idx) = sibs.iter().position(|&s| s == node) else {
                return Vec::new();
            };
            if axis == Axis::FollowingSibling {
                sibs[idx + 1..].to_vec()
            } else {
                sibs[..idx].to_vec()
            }
        }
    }
}

fn collect_descendants(doc: &Document, node: NodeId, out: &mut Vec<NodeId>) {
    for &child in doc.children(node) {
        out.push(child);
        collect_descendants(doc, child, out);
    }
}

fn test_matches(
    doc: &Document,
    node: NodeId,
    test: &NodeTest,
    axis: Axis,
) -> bool {
    // On the attribute axis the candidates are attribute nodes, so
    // `*` means "any attribute" and a name test matches the
    // attribute's local name.
    if axis == Axis::Attribute {
        return match (test, doc.kind(node)) {
            (NodeTest::Wildcard | NodeTest::Any, Some(NodeKind::Attr(_))) => {
                true
            }
            (NodeTest::Name { namespace, local }, Some(NodeKind::Attr(a))) => {
                doc.name(a.name).is_some_and(|name| {
                    &name.local == local
                        && name.namespace.as_deref() == namespace.as_deref()
                })
            }
            _ => false,
        };
    }
    match test {
        NodeTest::Any => true,
        NodeTest::Wildcard => doc.is_element(node),
        NodeTest::Name { namespace, local } => {
            doc.element_name(node).is_some_and(|e| {
                &e.local == local
                    && e.namespace.as_deref() == namespace.as_deref()
            })
        }
        NodeTest::Text => {
            matches!(doc.kind(node), Some(NodeKind::Text(_)))
        }
        NodeTest::Comment => {
            matches!(doc.kind(node), Some(NodeKind::Comment(_)))
        }
        // With a target, only instructions with that target match;
        // without one, every processing instruction does.
        NodeTest::ProcessingInstruction(want) => matches!(
            doc.kind(node),
            Some(NodeKind::ProcessingInstruction { target, .. })
                if want.as_ref().is_none_or(|w| w == target)
        ),
    }
}

fn eval_binary(
    doc: &Document,
    op: BinaryOp,
    lhs: &Expr,
    rhs: &Expr,
    ctx: NodeId,
    position: usize,
    size: usize,
) -> Value {
    // `and`/`or` short-circuit, so they must not evaluate the right
    // side eagerly.
    match op {
        BinaryOp::And => {
            let l = eval(doc, lhs, ctx, position, size);
            if !l.to_boolean() {
                return Value::Boolean(false);
            }
            return Value::Boolean(
                eval(doc, rhs, ctx, position, size).to_boolean(),
            );
        }
        BinaryOp::Or => {
            let l = eval(doc, lhs, ctx, position, size);
            if l.to_boolean() {
                return Value::Boolean(true);
            }
            return Value::Boolean(
                eval(doc, rhs, ctx, position, size).to_boolean(),
            );
        }
        _ => {}
    }

    let l = eval(doc, lhs, ctx, position, size);
    let r = eval(doc, rhs, ctx, position, size);

    match op {
        BinaryOp::Union => {
            let mut out = l.nodes().unwrap_or(&[]).to_vec();
            out.extend_from_slice(r.nodes().unwrap_or(&[]));
            out.sort_unstable();
            out.dedup();
            Value::NodeSet(out)
        }
        BinaryOp::Eq | BinaryOp::Ne => {
            let eq = compare_equality(doc, &l, &r);
            Value::Boolean(if op == BinaryOp::Eq { eq } else { !eq })
        }
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            let a = l.to_number(doc);
            let b = r.to_number(doc);
            Value::Boolean(match op {
                BinaryOp::Lt => a < b,
                BinaryOp::Le => a <= b,
                BinaryOp::Gt => a > b,
                _ => a >= b,
            })
        }
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Mod => {
            let a = l.to_number(doc);
            let b = r.to_number(doc);
            Value::Number(match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => a / b,
                _ => a % b,
            })
        }
        BinaryOp::And | BinaryOp::Or => unreachable!("handled above"),
    }
}

/// `XPath` equality against a node-set is existential.
///
/// `//book/@lang = 'en'` is true if *any* matching attribute equals
/// `'en'`, not if all do. Getting this wrong is a silent correctness
/// bug rather than an error, so it is spelled out here.
fn compare_equality(doc: &Document, l: &Value, r: &Value) -> bool {
    match (l, r) {
        (Value::NodeSet(a), Value::NodeSet(b)) => a.iter().any(|x| {
            b.iter()
                .any(|y| string_value(doc, *x) == string_value(doc, *y))
        }),
        (Value::NodeSet(a), other) | (other, Value::NodeSet(a)) => {
            match other {
                Value::Number(n) => a.iter().any(|x| {
                    string_value(doc, *x)
                        .trim()
                        .parse::<f64>()
                        .is_ok_and(|v| (v - n).abs() < f64::EPSILON)
                }),
                Value::Boolean(b) => a.is_empty() != *b,
                _ => {
                    let s = other.to_str(doc);
                    a.iter().any(|x| string_value(doc, *x) == s)
                }
            }
        }
        (Value::Boolean(_), _) | (_, Value::Boolean(_)) => {
            l.to_boolean() == r.to_boolean()
        }
        (Value::Number(_), _) | (_, Value::Number(_)) => {
            let a = l.to_number(doc);
            let b = r.to_number(doc);
            (a - b).abs() < f64::EPSILON
        }
        _ => l.to_str(doc) == r.to_str(doc),
    }
}

/// `substring(string, start, length?)`.
///
/// The specification defines the result by *position*, not by a start
/// and a count: it keeps every character whose 1-based position `p`
/// satisfies
///
/// ```text
/// p >= round(start)  and  p < round(start) + round(length)
/// ```
///
/// Clamping the start to 1 and then taking `length` characters is a
/// different function -- it gives `"123"` for the specification's own
/// example, `substring("12345", 0, 3)`, which must be `"12"`, because
/// positions 0 and below still consume part of the window.
fn substring(s: &str, start: f64, length: Option<f64>) -> Value {
    let start = xpath_round(start);
    let end = match length {
        Some(length) => {
            let length = xpath_round(length);
            if length.is_nan() || start.is_nan() {
                f64::NAN
            } else {
                start + length
            }
        }
        None => f64::INFINITY,
    };
    Value::String(
        s.chars()
            .enumerate()
            .filter(|(i, _)| {
                let p = *i as f64 + 1.0;
                p >= start && p < end
            })
            .map(|(_, c)| c)
            .collect(),
    )
}

/// Dispatch a function call.
///
/// Split across two functions purely for length: the node and numeric
/// families are separated from the boolean and string ones so neither
/// grows past a readable size.
fn eval_function(
    doc: &Document,
    name: &str,
    args: &[Expr],
    ctx: NodeId,
    position: usize,
    size: usize,
) -> Value {
    let arg = |i: usize| -> Option<Value> {
        args.get(i).map(|a| eval(doc, a, ctx, position, size))
    };
    match name {
        "true" => Value::Boolean(true),
        "false" => Value::Boolean(false),
        "not" => Value::Boolean(!arg(0).is_some_and(|v| v.to_boolean())),
        "position" => Value::Number(position as f64),
        "last" => Value::Number(size as f64),
        "count" => Value::Number(
            arg(0)
                .and_then(|v| v.nodes().map(<[NodeId]>::len))
                .unwrap_or(0) as f64,
        ),
        "string" => Value::String(
            arg(0).map_or_else(|| string_value(doc, ctx), |v| v.to_str(doc)),
        ),
        "number" => {
            Value::Number(arg(0).map_or(f64::NAN, |v| v.to_number(doc)))
        }
        "boolean" => Value::Boolean(arg(0).is_some_and(|v| v.to_boolean())),
        _ => eval_string_function(doc, name, args, ctx, position, size),
    }
}

/// The string half of the function library.
///
/// Split out of [`eval_function`] purely for length, as the node and
/// numeric families are.
fn eval_string_function(
    doc: &Document,
    name: &str,
    args: &[Expr],
    ctx: NodeId,
    position: usize,
    size: usize,
) -> Value {
    let arg = |i: usize| -> Option<Value> {
        args.get(i).map(|a| eval(doc, a, ctx, position, size))
    };
    match name {
        "concat" => {
            let mut s = String::new();
            for a in args {
                s.push_str(&eval(doc, a, ctx, position, size).to_str(doc));
            }
            Value::String(s)
        }
        "string-length" => Value::Number(
            arg(0)
                .map_or_else(|| string_value(doc, ctx), |v| v.to_str(doc))
                .chars()
                .count() as f64,
        ),
        "starts-with" => {
            let a = arg(0).map(|v| v.to_str(doc)).unwrap_or_default();
            let b = arg(1).map(|v| v.to_str(doc)).unwrap_or_default();
            Value::Boolean(a.starts_with(&b))
        }
        "contains" => {
            let a = arg(0).map(|v| v.to_str(doc)).unwrap_or_default();
            let b = arg(1).map(|v| v.to_str(doc)).unwrap_or_default();
            Value::Boolean(a.contains(&b))
        }
        "normalize-space" => {
            let s = arg(0)
                .map_or_else(|| string_value(doc, ctx), |v| v.to_str(doc));
            Value::String(s.split_whitespace().collect::<Vec<_>>().join(" "))
        }
        "substring" => substring(
            &arg(0).map(|v| v.to_str(doc)).unwrap_or_default(),
            arg(1).map_or(1.0, |v| v.to_number(doc)),
            arg(2).map(|v| v.to_number(doc)),
        ),
        // All three are defined over *characters*, not bytes, but
        // `find` returns a byte offset and slicing at it is therefore
        // safe: a match of `needle` inside `haystack` always begins and
        // ends on a character boundary.
        "substring-before" => {
            let haystack = arg(0).map(|v| v.to_str(doc)).unwrap_or_default();
            let needle = arg(1).map(|v| v.to_str(doc)).unwrap_or_default();
            Value::String(
                haystack
                    .find(&needle)
                    .map(|at| haystack[..at].to_owned())
                    .unwrap_or_default(),
            )
        }
        "substring-after" => {
            let haystack = arg(0).map(|v| v.to_str(doc)).unwrap_or_default();
            let needle = arg(1).map(|v| v.to_str(doc)).unwrap_or_default();
            Value::String(
                haystack
                    .find(&needle)
                    .map(|at| haystack[at + needle.len()..].to_owned())
                    .unwrap_or_default(),
            )
        }
        "translate" => {
            let s = arg(0).map(|v| v.to_str(doc)).unwrap_or_default();
            let from: Vec<char> = arg(1)
                .map(|v| v.to_str(doc))
                .unwrap_or_default()
                .chars()
                .collect();
            let to: Vec<char> = arg(2)
                .map(|v| v.to_str(doc))
                .unwrap_or_default()
                .chars()
                .collect();
            let mut out = String::with_capacity(s.len());
            for c in s.chars() {
                match from.iter().position(|&f| f == c) {
                    // Shorter replacement string than search string:
                    // the character is *removed*, not left alone. A
                    // repeated character in the search string takes its
                    // first position, which `position` already gives.
                    Some(i) => {
                        if let Some(&r) = to.get(i) {
                            out.push(r);
                        }
                    }
                    None => out.push(c),
                }
            }
            Value::String(out)
        }
        _ => eval_node_function(doc, name, args, ctx, position, size),
    }
}

/// The node a node-describing function should report on: the first
/// node of its argument node-set, or the context node when it takes no
/// argument. Returns `None` when an argument was supplied but selected
/// nothing, which is not the same as having no argument at all.
/// The expanded name of a node, as `local-name` and `namespace-uri`
/// define it.
///
/// `XPath` 1.0 gives an expanded-name to elements **and attributes**;
/// reading only `Document::element_name` here meant both functions
/// answered the empty string for every attribute, which silently broke
/// the one workaround available for selecting by namespace. A
/// processing instruction has a local part -- its target -- and no
/// namespace. Everything else has neither.
fn name_parts(doc: &Document, id: NodeId) -> Option<(&str, Option<&str>)> {
    match doc.kind(id)? {
        NodeKind::Element { .. } => doc
            .element_name(id)
            .map(|n| (n.local.as_str(), n.namespace.as_deref())),
        NodeKind::Attr(attribute) => doc
            .name(attribute.name)
            .map(|n| (n.local.as_str(), n.namespace.as_deref())),
        NodeKind::ProcessingInstruction { target, .. } => {
            Some((target.as_str(), None))
        }
        NodeKind::Root | NodeKind::Text(_) | NodeKind::Comment(_) => None,
    }
}

/// The namespace `xml:` is bound to by specification, and so the one
/// `xml:lang` carries once names are resolved.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// The value of an attribute on a node, by expanded name, without
/// building the intermediate `Vec` that [`Document::attributes`]
/// returns.
fn attribute_by_name<'d>(
    doc: &'d Document,
    id: NodeId,
    namespace: Option<&str>,
    local: &str,
) -> Option<&'d str> {
    doc.attribute_nodes(id).iter().find_map(|&a| {
        let NodeKind::Attr(attribute) = doc.kind(a)? else {
            return None;
        };
        let name = doc.name(attribute.name)?;
        (name.local == local && name.namespace.as_deref() == namespace)
            .then_some(attribute.value.as_str())
    })
}

/// The `xml:lang` in scope for a node: the one on the node itself, or
/// failing that the one on its nearest ancestor that carries it.
///
/// `xml:lang` is defined to be inherited, so reading only the context
/// node would answer `false` for every element below the one that
/// declares the language -- which is nearly always where the text is.
fn in_scope_lang(doc: &Document, id: NodeId) -> Option<&str> {
    let mut at = id;
    loop {
        if let Some(value) =
            attribute_by_name(doc, at, Some(XML_NAMESPACE), "lang")
        {
            return Some(value);
        }
        at = doc.parent(at)?;
    }
}

/// `lang(s)`: whether the in-scope language is `s`, or is `s` followed
/// by `-` and a subtag, so `lang("en")` is true for `en-GB`.
///
/// Language tags are ASCII and compared without regard to case.
fn lang_matches(doc: &Document, ctx: NodeId, want: &str) -> bool {
    let Some(have) = in_scope_lang(doc, ctx) else {
        return false;
    };
    if have.eq_ignore_ascii_case(want) {
        return true;
    }
    // A suffix only counts when a `-` separates it, so `lang("e")` is
    // false for `en`. Taking the prefix with `get` rather than slicing
    // keeps a multi-byte character straddling the boundary from
    // panicking, though the `-` test already implies a boundary.
    have.as_bytes().get(want.len()) == Some(&b'-')
        && have
            .get(..want.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(want))
}

/// The `QName` `name()` reports: the prefix the name was written
/// with, a colon, then the local part -- or just the local part when
/// the name had no prefix.
///
/// A processing instruction's name is its target, which never has a
/// prefix. Everything without an expanded name answers the empty
/// string, as `local-name` does.
fn qualified_name(doc: &Document, id: NodeId) -> Option<String> {
    let name_id = match doc.kind(id)? {
        NodeKind::Element { name, .. } => *name,
        NodeKind::Attr(attribute) => attribute.name,
        NodeKind::ProcessingInstruction { target, .. } => {
            return Some(target.clone());
        }
        NodeKind::Root | NodeKind::Text(_) | NodeKind::Comment(_) => {
            return None;
        }
    };
    let local = &doc.name(name_id)?.local;
    Some(match doc.prefix(name_id) {
        Some(prefix) => {
            let mut out = String::with_capacity(prefix.len() + 1 + local.len());
            out.push_str(prefix);
            out.push(':');
            out.push_str(local);
            out
        }
        None => local.clone(),
    })
}

fn node_argument(
    doc: &Document,
    args: &[Expr],
    ctx: NodeId,
    position: usize,
    size: usize,
) -> Option<NodeId> {
    match args.first() {
        None => Some(ctx),
        Some(a) => {
            match eval(doc, a, ctx, position, size) {
                Value::NodeSet(nodes) => nodes.first().copied(),
                // A non-node-set argument names no node.
                _ => None,
            }
        }
    }
}

/// `XPath` 1.0 rounding: the nearest integer, and on a tie the one
/// closer to positive infinity.
///
/// This is not `f64::round`, which breaks ties away from zero and so
/// gives `-2` for `round(-1.5)` where the specification requires `-1`.
fn xpath_round(n: f64) -> f64 {
    if n.is_nan() || n.is_infinite() {
        return n;
    }
    float::floor(n + 0.5)
}

/// The node-oriented and numeric half of the function library.
fn eval_node_function(
    doc: &Document,
    name: &str,
    args: &[Expr],
    ctx: NodeId,
    position: usize,
    size: usize,
) -> Value {
    let arg = |i: usize| -> Option<Value> {
        args.get(i).map(|a| eval(doc, a, ctx, position, size))
    };
    match name {
        // Both take an optional node-set: with one, they describe its
        // *first* node; without one, the context node. Reading `ctx`
        // unconditionally made `local-name(//x)` answer about whatever
        // the expression happened to be evaluated from — usually the
        // document root, which has no name, so the answer was always
        // the empty string.
        "local-name" => Value::String(
            node_argument(doc, args, ctx, position, size)
                .and_then(|n| name_parts(doc, n))
                .map(|(local, _)| local.to_owned())
                .unwrap_or_default(),
        ),
        "namespace-uri" => Value::String(
            node_argument(doc, args, ctx, position, size)
                .and_then(|n| name_parts(doc, n))
                .and_then(|(_, namespace)| namespace)
                .map(str::to_owned)
                .unwrap_or_default(),
        ),
        "lang" => Value::Boolean(lang_matches(
            doc,
            ctx,
            &arg(0).map(|v| v.to_str(doc)).unwrap_or_default(),
        )),
        "name" => Value::String(
            node_argument(doc, args, ctx, position, size)
                .and_then(|n| qualified_name(doc, n))
                .unwrap_or_default(),
        ),
        // `id()` takes a whitespace-separated list of IDs, not one
        // ID. Given a node-set it uses each node's string-value as such
        // a list; given anything else, its string-value. The result is
        // a node-set, so it must come back in document order and
        // without duplicates -- ids are arena indices assigned in
        // document order, so sorting by index gives exactly that.
        "id" => {
            let mut out = Vec::new();
            let collect = |list: &str, out: &mut Vec<NodeId>| {
                for token in list.split_whitespace() {
                    if let Some(found) = doc.element_by_id(token) {
                        out.push(found);
                    }
                }
            };
            match arg(0) {
                Some(Value::NodeSet(nodes)) => {
                    for node in nodes {
                        collect(&string_value(doc, node), &mut out);
                    }
                }
                Some(other) => collect(&other.to_str(doc), &mut out),
                None => {}
            }
            out.sort_unstable();
            out.dedup();
            Value::NodeSet(out)
        }
        "sum" => {
            let total = arg(0)
                .and_then(|v| v.nodes().map(<[NodeId]>::to_vec))
                .unwrap_or_default()
                .iter()
                .filter_map(|id| {
                    string_value(doc, *id).trim().parse::<f64>().ok()
                })
                .sum();
            Value::Number(total)
        }
        "floor" => Value::Number(float::floor(
            arg(0).map_or(f64::NAN, |v| v.to_number(doc)),
        )),
        "ceiling" => Value::Number(float::ceil(
            arg(0).map_or(f64::NAN, |v| v.to_number(doc)),
        )),
        "round" => Value::Number(xpath_round(
            arg(0).map_or(f64::NAN, |v| v.to_number(doc)),
        )),
        // Unreachable: the parser checks every function name against
        // `FUNCTIONS` and refuses to compile an expression naming
        // anything else, so no unknown name survives to be evaluated.
        // An empty node-set rather than a panic keeps that a
        // total function even if the two lists ever drift.
        _ => Value::NodeSet(Vec::new()),
    }
}
