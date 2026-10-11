// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Comparison (section 3.4) and the number grammar (section 3.5).
//!
//! Every comparison involving a node-set is existential: it holds if
//! *some* node satisfies it, and two node-sets compare by any pair.
//! That is as true of `<` as of `=`, and `!=` is existential in its
//! own right rather than the negation of `=`: `//a != 'x'` asks
//! whether some node differs, which `not(//a = 'x')` does not.
//!
//! Numbers compare by IEEE 754 equality, which is exact. An epsilon
//! would make `1 div 1e17` equal to `2 div 1e17` and infinity unequal
//! to itself.

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use super::ast::BinaryOp;
use super::float;
use super::value::{Value, string_value};
use crate::tree::{Document, NodeId};

/// IEEE 754 equality, spelled through `partial_cmp` so that the
/// intent -- exact, not approximate -- is explicit at every call.
/// `NaN` is equal to nothing, and `-0` is equal to `0`.
pub(crate) fn ieee_eq(a: f64, b: f64) -> bool {
    a.partial_cmp(&b) == Some(Ordering::Equal)
}

/// The four characters the XML `S` production allows: space, tab,
/// carriage return, line feed. `XPath` trims and splits on these and
/// on nothing else; a no-break space is a character like any other.
pub(crate) fn is_xml_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// `number()` of a string: the `XPath` 1.0 `Number` production,
/// optionally negated and surrounded by XML whitespace, and `NaN`
/// for anything else.
///
/// Rust's `str::parse::<f64>` accepts a superset -- `1e5`, `+1`,
/// `inf`, `NaN` -- and `str::trim` strips Unicode whitespace, so
/// neither can stand in for the grammar. Every string-to-number
/// conversion in the evaluator goes through here.
pub(crate) fn xpath_number(s: &str) -> f64 {
    let trimmed = s.trim_matches(is_xml_whitespace);
    if !is_xpath_number(trimmed) {
        return f64::NAN;
    }
    trimmed.parse().unwrap_or(f64::NAN)
}

/// `Number ::= Digits ('.' Digits?)? | '.' Digits`, with an optional
/// leading `-`.
fn is_xpath_number(s: &str) -> bool {
    let digits = |part: &str| {
        !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
    };
    let unsigned = s.strip_prefix('-').unwrap_or(s);
    match unsigned.split_once('.') {
        None => digits(unsigned),
        Some((int, frac)) => {
            (digits(int) && (frac.is_empty() || digits(frac)))
                || (int.is_empty() && digits(frac))
        }
    }
}

/// `round()`: the closest integer, and on a tie the one closer to
/// positive infinity.
///
/// Not `f64::round`, which breaks ties away from zero and gives `-2`
/// for `round(-1.5)` where the specification requires `-1`. Not
/// `floor(n + 0.5)` either: the addition rounds, so
/// `0.49999999999999994` becomes `1` and `2^52 + 1` becomes
/// `2^52 + 2`. Floor first, then compare the fraction, which is
/// exact. A negative argument that rounds to zero gives negative
/// zero, as section 4.4 specifies; it is observable only through
/// division.
pub(crate) fn xpath_round(n: f64) -> f64 {
    if !n.is_finite() {
        return n;
    }
    let down = float::floor(n);
    let rounded = if n - down >= 0.5 { down + 1.0 } else { down };
    if ieee_eq(rounded, 0.0) && n.is_sign_negative() {
        return -0.0;
    }
    rounded
}

/// Section 3.4, for the six comparison operators.
pub(crate) fn compare(
    doc: &Document,
    op: BinaryOp,
    l: &Value,
    r: &Value,
) -> bool {
    match (l, r) {
        (Value::NodeSet(a), Value::NodeSet(b)) => compare_sets(doc, op, a, b),
        (Value::NodeSet(a), other) => compare_set_with(doc, op, a, other),
        (other, Value::NodeSet(b)) => compare_set_with(doc, flip(op), b, other),
        _ => compare_scalars(doc, op, l, r),
    }
}

/// The operator with its operands swapped: `a < b` is `b > a`.
fn flip(op: BinaryOp) -> BinaryOp {
    match op {
        BinaryOp::Lt => BinaryOp::Gt,
        BinaryOp::Le => BinaryOp::Ge,
        BinaryOp::Gt => BinaryOp::Lt,
        BinaryOp::Ge => BinaryOp::Le,
        other => other,
    }
}

/// Neither operand is a node-set. For `=` and `!=`, a boolean on
/// either side compares booleans, otherwise a number on either side
/// compares numbers, otherwise strings. The four relational operators
/// always compare numbers.
fn compare_scalars(doc: &Document, op: BinaryOp, l: &Value, r: &Value) -> bool {
    let equal = match (l, r) {
        (Value::Boolean(_), _) | (_, Value::Boolean(_)) => {
            l.to_boolean() == r.to_boolean()
        }
        (Value::Number(_), _) | (_, Value::Number(_)) => {
            ieee_eq(l.to_number(doc), r.to_number(doc))
        }
        _ => l.to_str(doc) == r.to_str(doc),
    };
    match op {
        BinaryOp::Eq => equal,
        BinaryOp::Ne => !equal,
        _ => compare_numbers(op, l.to_number(doc), r.to_number(doc)),
    }
}

/// One operand is a node-set and the other is not.
///
/// Against a boolean the node-set's truth is compared. Against a
/// number, some node's number must satisfy the comparison. Against a
/// string, `=` and `!=` compare some node's string-value and the
/// relational operators compare numbers.
fn compare_set_with(
    doc: &Document,
    op: BinaryOp,
    set: &[NodeId],
    other: &Value,
) -> bool {
    match other {
        Value::Boolean(_) => {
            compare_scalars(doc, op, &Value::Boolean(!set.is_empty()), other)
        }
        Value::String(s) if matches!(op, BinaryOp::Eq | BinaryOp::Ne) => {
            let want = op == BinaryOp::Eq;
            set.iter().any(|&id| (string_value(doc, id) == *s) == want)
        }
        _ => {
            let n = other.to_number(doc);
            set.iter().any(|&id| {
                compare_numbers(op, xpath_number(&string_value(doc, id)), n)
            })
        }
    }
}

/// Two node-sets: true if some pair of nodes satisfies the
/// comparison. Two empty sets, or one, satisfy nothing.
fn compare_sets(
    doc: &Document,
    op: BinaryOp,
    a: &[NodeId],
    b: &[NodeId],
) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    match op {
        BinaryOp::Eq => sets_share_a_value(doc, a, b),
        BinaryOp::Ne => sets_differ_somewhere(doc, a, b),
        _ => sets_compare_at_the_extremes(doc, op, a, b),
    }
}

/// `=`: the smaller set's string-values are hoisted into a set and
/// the larger is scanned against it, O((n + m) log n) rather than
/// O(nm).
fn sets_share_a_value(doc: &Document, a: &[NodeId], b: &[NodeId]) -> bool {
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let values: BTreeSet<String> =
        small.iter().map(|&id| string_value(doc, id)).collect();
    large
        .iter()
        .any(|&id| values.contains(&string_value(doc, id)))
}

/// `!=`: some pair differs unless both sets hold exactly one distinct
/// value and it is the same one.
fn sets_differ_somewhere(doc: &Document, a: &[NodeId], b: &[NodeId]) -> bool {
    let va: BTreeSet<String> =
        a.iter().map(|&id| string_value(doc, id)).collect();
    let vb: BTreeSet<String> =
        b.iter().map(|&id| string_value(doc, id)).collect();
    va.len() > 1 || vb.len() > 1 || va != vb
}

/// The relational operators: some pair satisfies `<` exactly when
/// the smallest of one side is below the largest of the other, so
/// only the extremes are compared.
fn sets_compare_at_the_extremes(
    doc: &Document,
    op: BinaryOp,
    a: &[NodeId],
    b: &[NodeId],
) -> bool {
    let (min_a, max_a) = extremes(doc, a);
    let (min_b, max_b) = extremes(doc, b);
    match op {
        BinaryOp::Lt => min_a < max_b,
        BinaryOp::Le => min_a <= max_b,
        BinaryOp::Gt => max_a > min_b,
        _ => max_a >= min_b,
    }
}

/// The smallest and largest numbers in a node-set, ignoring `NaN`,
/// which compares false against everything. An all-`NaN` set
/// yields `NaN` for both, so every comparison with it is false.
fn extremes(doc: &Document, set: &[NodeId]) -> (f64, f64) {
    let numbers: Vec<f64> = set
        .iter()
        .map(|&id| xpath_number(&string_value(doc, id)))
        .filter(|n| !n.is_nan())
        .collect();
    let min = numbers.iter().copied().fold(f64::NAN, f64::min);
    let max = numbers.iter().copied().fold(f64::NAN, f64::max);
    (min, max)
}

/// The six operators on two numbers.
pub(crate) fn compare_numbers(op: BinaryOp, a: f64, b: f64) -> bool {
    match op {
        BinaryOp::Eq => ieee_eq(a, b),
        BinaryOp::Ne => !ieee_eq(a, b),
        BinaryOp::Lt => a < b,
        BinaryOp::Le => a <= b,
        BinaryOp::Gt => a > b,
        BinaryOp::Ge => a >= b,
        _ => unreachable!("not a comparison operator"),
    }
}
