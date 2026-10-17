// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The `XPath` 1.0 function library, section 4.
//!
//! Dispatch is on [`Func`], resolved at compile time, in five
//! families that mirror the specification's own sections: node-set,
//! string, the string functions that take positions, boolean and
//! number. A function that requires a node-set and is given something
//! else is a type error through the fallible entry point and the
//! documented silent value through the infallible one; `id()` is the
//! exception, since the specification converts its argument.

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::EvalError;
use super::ast::{Expr, Func};
use super::compare::{is_xml_whitespace, xpath_number, xpath_round};
use super::eval::{Ctx, Ev, eval, type_error};
use super::float;
use super::names::{lang_matches, name_parts, qualified_name};
use super::value::Value;
use crate::tree::{Document, NodeId};

pub(crate) fn eval_function(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    match func {
        Func::Last
        | Func::Position
        | Func::Count
        | Func::Id
        | Func::LocalName
        | Func::NamespaceUri
        | Func::Name => node_set_function(ev, func, args, c),
        Func::String
        | Func::Concat
        | Func::StartsWith
        | Func::Contains
        | Func::SubstringBefore
        | Func::SubstringAfter => string_function(ev, func, args, c),
        Func::Substring
        | Func::StringLength
        | Func::NormalizeSpace
        | Func::Translate => positional_string_function(ev, func, args, c),
        Func::Boolean | Func::Not | Func::True | Func::False | Func::Lang => {
            boolean_function(ev, func, args, c)
        }
        Func::Number
        | Func::Sum
        | Func::Floor
        | Func::Ceiling
        | Func::Round => number_function(ev, func, args, c),
    }
}

/// Argument `i`, evaluated, if there is one.
fn arg(
    ev: &Ev<'_>,
    args: &[Expr],
    i: usize,
    c: Ctx,
) -> Result<Option<Value>, EvalError> {
    args.get(i).map(|a| eval(ev, a, c)).transpose()
}

/// Argument `i` as a string, or the empty string when absent.
fn string_arg(
    ev: &Ev<'_>,
    args: &[Expr],
    i: usize,
    c: Ctx,
) -> Result<String, EvalError> {
    match arg(ev, args, i, c)? {
        Some(value) => ev.stringify(&value),
        None => Ok(String::new()),
    }
}

/// Argument `i` as a string, or the context node's string-value when
/// absent: the default the string functions share.
fn string_arg_or_context(
    ev: &Ev<'_>,
    args: &[Expr],
    i: usize,
    c: Ctx,
) -> Result<String, EvalError> {
    match arg(ev, args, i, c)? {
        Some(value) => ev.stringify(&value),
        None => ev.node_string(c.node),
    }
}

/// Argument `i` as a number, or `NaN` when absent.
fn number_arg(
    ev: &Ev<'_>,
    args: &[Expr],
    i: usize,
    c: Ctx,
) -> Result<f64, EvalError> {
    match arg(ev, args, i, c)? {
        Some(value) => ev.number(&value),
        None => Ok(f64::NAN),
    }
}

/// Argument `i`, which `func` requires to be a node-set. `None` when
/// it is absent, or -- outside strict mode -- when it is something
/// else.
fn node_set_arg(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    i: usize,
    c: Ctx,
) -> Result<Option<Vec<NodeId>>, EvalError> {
    match arg(ev, args, i, c)? {
        Some(Value::NodeSet(nodes)) => Ok(Some(nodes)),
        Some(other) => {
            let message = format!(
                "{}() takes a node-set, not a {}",
                func.name(),
                other.type_name()
            );
            type_error(ev, None, message)
        }
        None => Ok(None),
    }
}

/// The node a node-describing function should report on: the first
/// node of its argument node-set, or the context node when it takes no
/// argument. `None` when an argument was supplied but selected
/// nothing, which is not the same as having no argument at all.
fn node_argument(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    c: Ctx,
) -> Result<Option<NodeId>, EvalError> {
    if args.is_empty() {
        return Ok(Some(c.node));
    }
    Ok(node_set_arg(ev, func, args, 0, c)?
        .and_then(|nodes| nodes.first().copied()))
}

fn node_set_function(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    match func {
        Func::Last => Ok(Value::Number(c.size as f64)),
        Func::Position => Ok(Value::Number(c.position as f64)),
        Func::Count => Ok(Value::Number(
            node_set_arg(ev, func, args, 0, c)?.map_or(0, |n| n.len()) as f64,
        )),
        Func::Id => id_function(ev, args, c),
        // The three name functions take an optional node-set: with
        // one, they describe its *first* node; without one, the
        // context node. Reading the context node unconditionally made
        // `local-name(//x)` answer about whatever the expression
        // happened to be evaluated from -- usually the document root,
        // which has no name, so the answer was always the empty
        // string.
        Func::LocalName => describe_node(ev, func, args, c, local_name),
        Func::NamespaceUri => describe_node(ev, func, args, c, namespace_uri),
        _ => describe_node(ev, func, args, c, qualified_name),
    }
}

/// A name function: `describe` applied to the node the argument (or
/// its absence) selects, and the empty string for a node without a
/// name or an argument that selects nothing.
fn describe_node(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    c: Ctx,
    describe: fn(&Document, NodeId) -> Option<String>,
) -> Result<Value, EvalError> {
    let node = node_argument(ev, func, args, c)?;
    if node.is_some() {
        ev.budget.string(ev.budget.name_bytes)?;
    }
    Ok(Value::String(
        node.and_then(|n| describe(ev.doc, n)).unwrap_or_default(),
    ))
}

fn local_name(doc: &Document, id: NodeId) -> Option<String> {
    name_parts(doc, id).map(|(local, _)| local.to_owned())
}

fn namespace_uri(doc: &Document, id: NodeId) -> Option<String> {
    name_parts(doc, id).and_then(|(_, namespace)| namespace.map(str::to_owned))
}

/// `id()` takes a whitespace-separated list of IDs, not one ID. Given
/// a node-set it uses each node's string-value as such a list; given
/// anything else, its string-value. The result is a node-set, so it
/// comes back in document order and without duplicates.
fn id_function(ev: &Ev<'_>, args: &[Expr], c: Ctx) -> Result<Value, EvalError> {
    let mut out = Vec::new();
    match arg(ev, args, 0, c)? {
        Some(Value::NodeSet(nodes)) => {
            collect_node_ids(ev, &nodes, &mut out)?;
        }
        Some(other) => collect_ids(ev, &ev.stringify(&other)?, &mut out)?,
        None => {}
    }
    ev.sort(&mut out)?;
    Ok(Value::NodeSet(out))
}

fn collect_node_ids(
    ev: &Ev<'_>,
    nodes: &[NodeId],
    out: &mut Vec<NodeId>,
) -> Result<(), EvalError> {
    for &node in nodes {
        collect_ids(ev, &ev.node_string(node)?, out)?;
    }
    Ok(())
}

/// The elements whose ID is one of the XML-whitespace-separated
/// tokens of `list`.
fn collect_ids(
    ev: &Ev<'_>,
    list: &str,
    out: &mut Vec<NodeId>,
) -> Result<(), EvalError> {
    ev.budget.charge(
        list.len()
            .saturating_mul(super::budget::sort_factor(ev.doc.len())),
    )?;
    for token in list.split(is_xml_whitespace).filter(|t| !t.is_empty()) {
        if let Some(found) = ev.doc.element_by_id(token) {
            ev.budget.nodes(out.len().saturating_add(1))?;
            out.push(found);
        }
    }
    Ok(())
}

fn string_function(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    match func {
        Func::String => {
            Ok(Value::String(string_arg_or_context(ev, args, 0, c)?))
        }
        Func::Concat => concat(ev, args, c),
        _ => string_pair_function(ev, func, args, c),
    }
}

fn concat(ev: &Ev<'_>, args: &[Expr], c: Ctx) -> Result<Value, EvalError> {
    let mut s = String::new();
    for i in 0..args.len() {
        let part = string_arg(ev, args, i, c)?;
        ev.budget.string(part.len())?;
        s.push_str(&part);
    }
    Ok(Value::String(s))
}

/// The functions of two strings.
fn string_pair_function(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    let a = string_arg(ev, args, 0, c)?;
    let b = string_arg(ev, args, 1, c)?;
    if matches!(func, Func::SubstringBefore | Func::SubstringAfter) {
        ev.budget.string(a.len())?;
    }
    Ok(match func {
        Func::StartsWith => Value::Boolean(a.starts_with(&b)),
        Func::Contains => Value::Boolean(a.contains(&b)),
        Func::SubstringBefore => Value::String(substring_before(&a, &b)),
        _ => Value::String(substring_after(&a, &b)),
    })
}

/// Both are defined over *characters*, not bytes, but `find` returns
/// a byte offset and slicing at it is therefore safe: a match of
/// `needle` inside `haystack` always begins and ends on a character
/// boundary. No match is the empty string, not the whole.
fn substring_before(haystack: &str, needle: &str) -> String {
    haystack
        .find(needle)
        .map(|at| haystack[..at].to_owned())
        .unwrap_or_default()
}

fn substring_after(haystack: &str, needle: &str) -> String {
    haystack
        .find(needle)
        .map(|at| haystack[at + needle.len()..].to_owned())
        .unwrap_or_default()
}

fn positional_string_function(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    match func {
        Func::Substring => substring_call(ev, args, c),
        Func::StringLength => Ok(Value::Number(
            string_arg_or_context(ev, args, 0, c)?.chars().count() as f64,
        )),
        Func::NormalizeSpace => {
            let text = string_arg_or_context(ev, args, 0, c)?;
            ev.budget.string(text.len())?;
            ev.budget.charge(text.len().saturating_mul(3))?;
            Ok(Value::String(normalize_space(&text)))
        }
        _ => translate_call(ev, args, c),
    }
}

fn substring_call(
    ev: &Ev<'_>,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    let s = string_arg(ev, args, 0, c)?;
    let start = number_arg(ev, args, 1, c)?;
    let length = arg(ev, args, 2, c)?.map(|v| ev.number(&v)).transpose()?;
    ev.budget.string(s.len())?;
    Ok(substring(&s, start, length))
}

fn translate_call(
    ev: &Ev<'_>,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    let s = string_arg(ev, args, 0, c)?;
    let from = string_arg(ev, args, 1, c)?;
    let to = string_arg(ev, args, 2, c)?;
    ev.budget.string(s.len().saturating_mul(4))?;
    let bytes = s.len().saturating_add(from.len()).saturating_add(to.len());
    ev.budget
        .charge(bytes.saturating_mul(super::budget::sort_factor(from.len())))?;
    Ok(Value::String(translate(&s, &from, &to)))
}

/// Leading and trailing XML whitespace removed, and every run of it
/// inside collapsed to one space.
fn normalize_space(s: &str) -> String {
    s.split(is_xml_whitespace)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `translate(s, from, to)`: each character of `s` found in `from`
/// is replaced by the character at the same position in `to`, or
/// removed when `to` is shorter. A character repeated in `from` takes
/// its *first* position. The map is built once rather than searching
/// `from` per character.
fn translate(s: &str, from: &str, to: &str) -> String {
    let mut map: BTreeMap<char, Option<char>> = BTreeMap::new();
    let mut replacements = to.chars();
    for f in from.chars() {
        let replacement = replacements.next();
        let _ = map.entry(f).or_insert(replacement);
    }
    s.chars()
        .filter_map(|ch| map.get(&ch).map_or(Some(ch), |r| *r))
        .collect()
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

fn boolean_function(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    Ok(Value::Boolean(match func {
        Func::True => true,
        Func::False => false,
        Func::Not => !truth_arg(ev, args, c)?,
        Func::Boolean => truth_arg(ev, args, c)?,
        _ => {
            ev.budget.charge(ev.budget.document_cost)?;
            lang_matches(ev.doc, c.node, &string_arg(ev, args, 0, c)?)
        }
    }))
}

/// The first argument's truth; false when absent.
fn truth_arg(ev: &Ev<'_>, args: &[Expr], c: Ctx) -> Result<bool, EvalError> {
    Ok(arg(ev, args, 0, c)?.is_some_and(|v| v.to_boolean()))
}

fn number_function(
    ev: &Ev<'_>,
    func: Func,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    if func == Func::Sum {
        // Each node converts as `number()` does, so one node that is
        // not a number makes the sum `NaN` rather than being skipped.
        return sum_function(ev, args, c);
    }
    let n = if args.is_empty() {
        // `number()` with no argument converts the context node.
        xpath_number(&ev.node_string(c.node)?)
    } else {
        number_arg(ev, args, 0, c)?
    };
    Ok(Value::Number(match func {
        Func::Floor => float::floor(n),
        Func::Ceiling => float::ceil(n),
        Func::Round => xpath_round(n),
        _ => n,
    }))
}

fn sum_function(
    ev: &Ev<'_>,
    args: &[Expr],
    c: Ctx,
) -> Result<Value, EvalError> {
    let mut total = 0.0;
    for node in node_set_arg(ev, Func::Sum, args, 0, c)?.unwrap_or_default() {
        total += xpath_number(&ev.node_string(node)?);
    }
    Ok(Value::Number(total))
}
