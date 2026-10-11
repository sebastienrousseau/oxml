// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Evaluating a compiled `XPath` expression against a [`Document`].
//!
//! This file holds the expression walk and the two
//! things every location path needs: a step applied per context node
//! (section 2.4) and a node-set put into document order. The value
//! type lives in `value`, the axes in `axes`, the comparisons and
//! number grammar in `compare`, and the function library in
//! `functions`.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::ast::{Axis, BinaryOp, Expr, Step};
use super::axes::{
    following_union, for_each_on_axis, preceding_union, test_matches,
};
use super::compare::{compare, ieee_eq};
use super::float;
use super::functions::eval_function;
use super::value::Value;
use super::{EvalContext, EvalError};
use crate::tree::{Document, ExpandedName, NodeId};

/// Everything one evaluation carries besides the context node.
pub(crate) struct Ev<'a> {
    pub(crate) doc: &'a Document,
    /// The bound variables.
    pub(crate) vars: &'a EvalContext,
    /// Whether a type error is reported, or answered with the
    /// documented silent value as the infallible entry points do.
    pub(crate) strict: bool,
    /// Where document order comes from.
    pub(crate) order: Order<'a>,
}

/// The context of one sub-expression: the node, and its proximity
/// position within the node-set a predicate is filtering.
#[derive(Clone, Copy)]
pub(crate) struct Ctx {
    pub(crate) node: NodeId,
    pub(crate) position: usize,
    pub(crate) size: usize,
}

/// Evaluate a compiled expression against a document.
///
/// # Errors
///
/// An unbound variable, or in strict mode a type error, as
/// [`EvalError`] describes.
pub(crate) fn evaluate(
    doc: &Document,
    expr: &Expr,
    context: NodeId,
    vars: &EvalContext,
    strict: bool,
) -> Result<Value, EvalError> {
    let ev = Ev {
        doc,
        vars,
        strict,
        order: Order::for_document(doc),
    };
    let ctx = Ctx {
        node: context,
        position: 1,
        size: 1,
    };
    eval(&ev, expr, ctx)
}

/// A type error in strict mode; the documented silent value otherwise.
pub(crate) fn type_error<T>(
    ev: &Ev<'_>,
    fallback: T,
    message: String,
) -> Result<T, EvalError> {
    if ev.strict {
        return Err(EvalError::TypeError(message));
    }
    Ok(fallback)
}

pub(crate) fn eval(
    ev: &Ev<'_>,
    expr: &Expr,
    c: Ctx,
) -> Result<Value, EvalError> {
    match expr {
        Expr::Literal(s) => Ok(Value::String(s.clone())),
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::Negate(inner) => {
            Ok(Value::Number(-eval(ev, inner, c)?.to_number(ev.doc)))
        }
        Expr::Variable(name) => lookup_variable(ev, name),
        Expr::Path { absolute, steps } => {
            let start = if *absolute { ev.doc.root() } else { c.node };
            Ok(Value::NodeSet(eval_steps(ev, steps, alloc::vec![start])?))
        }
        compound => eval_compound(ev, compound, c),
    }
}

/// The expressions with sub-expressions.
fn eval_compound(ev: &Ev<'_>, expr: &Expr, c: Ctx) -> Result<Value, EvalError> {
    match expr {
        Expr::Binary { op, lhs, rhs } => eval_binary(ev, *op, lhs, rhs, c),
        Expr::Chain { first, rest } => eval_chain(ev, first, rest, c),
        Expr::Function { func, args } => eval_function(ev, *func, args, c),
        Expr::Filter(filter) => {
            eval_filter(ev, &filter.base, &filter.predicates, &filter.steps, c)
        }
        _ => unreachable!("not a compound expression"),
    }
}

/// The value bound to a variable, named for the error as the local
/// part or `{uri}local`.
fn lookup_variable(
    ev: &Ev<'_>,
    name: &ExpandedName,
) -> Result<Value, EvalError> {
    ev.vars.variable(name).cloned().ok_or_else(|| {
        EvalError::UnboundVariable(match &name.namespace {
            Some(uri) => format!("{{{uri}}}{}", name.local),
            None => name.local.clone(),
        })
    })
}

/// `(primary)[pred].../step...`: the predicates filter the primary's
/// node-set in document order, counted forwards, and the steps
/// continue from whatever is left.
fn eval_filter(
    ev: &Ev<'_>,
    base: &Expr,
    predicates: &[Expr],
    steps: &[Step],
    c: Ctx,
) -> Result<Value, EvalError> {
    let mut nodes = match eval(ev, base, c)? {
        Value::NodeSet(nodes) => nodes,
        other => {
            let message = format!(
                "a predicate or path step applies to a node-set, not a {}",
                other.type_name()
            );
            return type_error(ev, Value::NodeSet(Vec::new()), message);
        }
    };
    // Every node-set the evaluator builds is already in document
    // order; one bound to a variable came from the caller.
    if matches!(base, Expr::Variable(_)) {
        sort_document_order(&ev.order, &mut nodes);
    }
    let kept = apply_predicates(ev, nodes, predicates)?;
    Ok(Value::NodeSet(eval_steps(ev, steps, kept)?))
}

/// Walk `steps` from every node in `start`.
fn eval_steps(
    ev: &Ev<'_>,
    steps: &[Step],
    start: Vec<NodeId>,
) -> Result<Vec<NodeId>, EvalError> {
    let mut current = start;
    for step in steps {
        current = eval_step(ev, step, &current)?;
    }
    Ok(current)
}

/// One step over every context node, merged into document order.
///
/// A single context node yields its result already ordered and
/// without duplicates, so the merge is skipped. `following::` and
/// `preceding::` without predicates have a closed form for the whole
/// union, one walk of the document rather than one per context node.
/// Otherwise the per-node results are deduplicated as they are
/// gathered rather than after, so the merge holds at most the
/// document even when the candidates are quadratic in it.
fn eval_step(
    ev: &Ev<'_>,
    step: &Step,
    context: &[NodeId],
) -> Result<Vec<NodeId>, EvalError> {
    if let [only] = context {
        return step_from(ev, step, *only);
    }
    if step.predicates.is_empty() {
        if step.axis == Axis::Following {
            return Ok(following_union(ev.doc, context, &step.test));
        }
        if step.axis == Axis::Preceding {
            let mut out = preceding_union(ev.doc, context, &step.test);
            sort_document_order(&ev.order, &mut out);
            return Ok(out);
        }
    }
    let mut seen = Seen::default();
    let mut out = Vec::new();
    for &node in context {
        for found in step_from(ev, step, node)? {
            if seen.insert(found) {
                out.push(found);
            }
        }
    }
    sort_document_order(&ev.order, &mut out);
    Ok(out)
}

/// Section 2.4: the axis from one context node, filtered by the node
/// test, then by each predicate in turn with the proximity position
/// counted *within that node-set* -- in reverse document order on a
/// reverse axis, so `preceding-sibling::*[1]` is the nearest sibling.
/// The result is handed back in document order whatever the axis.
fn step_from(
    ev: &Ev<'_>,
    step: &Step,
    node: NodeId,
) -> Result<Vec<NodeId>, EvalError> {
    let mut candidates = Vec::new();
    for_each_on_axis(ev.doc, node, step.axis, &mut |found| {
        if test_matches(ev.doc, found, &step.test, step.axis) {
            candidates.push(found);
        }
    });
    let mut kept = apply_predicates(ev, candidates, &step.predicates)?;
    if step.axis.is_reverse() {
        kept.reverse();
    } else if step.axis == Axis::Namespace {
        // In-scope namespaces are gathered nearest declaration first,
        // which is not where the nodes sit in the document.
        sort_document_order(&ev.order, &mut kept);
    }
    Ok(kept)
}

/// Filter `nodes`, which are in proximity order, by each predicate in
/// turn. Each predicate sees the node-set the previous one left.
pub(crate) fn apply_predicates(
    ev: &Ev<'_>,
    mut nodes: Vec<NodeId>,
    predicates: &[Expr],
) -> Result<Vec<NodeId>, EvalError> {
    for pred in predicates {
        nodes = filter_by_predicate(ev, &nodes, pred)?;
    }
    Ok(nodes)
}

fn filter_by_predicate(
    ev: &Ev<'_>,
    nodes: &[NodeId],
    pred: &Expr,
) -> Result<Vec<NodeId>, EvalError> {
    // A constant number is a position test that needs no evaluation:
    // `[1]` is the first node, `[0]`, `[1.5]` and `[-1]` are nothing.
    if let Expr::Number(n) = pred {
        return Ok(constant_index(nodes, *n));
    }
    let size = nodes.len();
    let mut kept = Vec::new();
    for (i, &node) in nodes.iter().enumerate() {
        let c = Ctx {
            node,
            position: i + 1,
            size,
        };
        if predicate_holds(ev, pred, c)? {
            kept.push(node);
        }
    }
    Ok(kept)
}

/// A predicate whose value is a number is true when it equals the
/// proximity position -- by IEEE equality, as every numeric
/// comparison is -- and otherwise when its boolean value is.
fn predicate_holds(
    ev: &Ev<'_>,
    pred: &Expr,
    c: Ctx,
) -> Result<bool, EvalError> {
    Ok(match eval(ev, pred, c)? {
        Value::Number(n) => ieee_eq(n, c.position as f64),
        other => other.to_boolean(),
    })
}

/// The node at position `n`, when `n` is an integer from one upwards,
/// and nothing otherwise. The cast saturates, so a position beyond
/// any `usize` selects nothing rather than wrapping.
fn constant_index(nodes: &[NodeId], n: f64) -> Vec<NodeId> {
    if n < 1.0 || !ieee_eq(n, float::trunc(n)) {
        return Vec::new();
    }
    let position = n as usize;
    nodes
        .get(position - 1)
        .map(|&id| alloc::vec![id])
        .unwrap_or_default()
}

/// Which arena slots a step has already produced, for merging the
/// per-context results of a step without a quadratic scan. A live
/// node has one slot, so the slot index identifies it.
#[derive(Default)]
struct Seen {
    slots: Vec<bool>,
}

impl Seen {
    /// Mark `id`; `true` if it was not marked before.
    fn insert(&mut self, id: NodeId) -> bool {
        let index = id.index();
        if index >= self.slots.len() {
            self.slots.resize(index + 1, false);
        }
        let fresh = !self.slots[index];
        self.slots[index] = true;
        fresh
    }
}

/// Where document order comes from for one evaluation.
///
/// A document that has never been mutated has its arena in document
/// order and [`Document::order_key`] answers from that for free. One
/// that has been mutated since its order cache was last refreshed
/// reports [`Document::is_order_dirty`]; evaluation takes `&Document`
/// and cannot refresh the cache, so it computes its own pre-order
/// table once, at O(N), and sorts by that. Calling
/// [`Document::document_order`] after a batch of mutations avoids
/// that cost on every evaluation.
pub(crate) struct Order<'a> {
    doc: &'a Document,
    /// The local table, indexed by slot, when the document's own
    /// cache is stale.
    local: Option<Vec<u32>>,
}

impl<'a> Order<'a> {
    fn for_document(doc: &'a Document) -> Self {
        let local = doc.is_order_dirty().then(|| pre_order(doc));
        Self { doc, local }
    }

    /// The sort key of a node: its pre-order position, then its slot
    /// to keep the order total for anything the walk did not reach.
    fn key(&self, id: NodeId) -> (u32, usize) {
        let position = match &self.local {
            Some(table) => table.get(id.index()).copied(),
            None => self.doc.order_key(id),
        };
        (position.unwrap_or(u32::MAX), id.index())
    }
}

/// `XPath` 1.0 section 5: the root, then for each element the element
/// itself, its namespace nodes, its attributes, and its children in
/// order. Iterative, so a deep document costs heap rather than stack.
fn pre_order(doc: &Document) -> Vec<u32> {
    let mut table = Vec::new();
    let mut next = 0_u32;
    let mut place = |id: NodeId, table: &mut Vec<u32>| {
        let index = id.index();
        if index >= table.len() {
            table.resize(index + 1, u32::MAX);
        }
        table[index] = next;
        next = next.saturating_add(1);
    };
    let mut stack = alloc::vec![doc.root()];
    while let Some(id) = stack.pop() {
        place(id, &mut table);
        for &ns in doc.namespace_nodes(id) {
            place(ns, &mut table);
        }
        for &attr in doc.attribute_nodes(id) {
            place(attr, &mut table);
        }
        stack.extend(doc.children(id).iter().rev());
    }
    table
}

/// Put a node-set into document order and drop duplicates. Every
/// node-set the evaluator hands on goes through here.
pub(crate) fn sort_document_order(order: &Order<'_>, nodes: &mut Vec<NodeId>) {
    nodes.sort_unstable_by_key(|&id| order.key(id));
    nodes.dedup();
}

fn eval_binary(
    ev: &Ev<'_>,
    op: BinaryOp,
    lhs: &Expr,
    rhs: &Expr,
    c: Ctx,
) -> Result<Value, EvalError> {
    let l = eval(ev, lhs, c)?;
    // `and`/`or` short-circuit, so they must not evaluate the right
    // side eagerly.
    if let Some(decided) = short_circuit(op, &l) {
        return Ok(decided);
    }
    let r = eval(ev, rhs, c)?;
    Ok(apply_binary(ev, op, &l, &r))
}

/// A flat chain, folded from the left: the same answer the nested
/// `Binary` form gives, in constant stack rather than one frame per
/// term. An `and`/`or` chain stops at the first operand that decides
/// it, exactly as the nested form stops at the first deciding pair.
fn eval_chain(
    ev: &Ev<'_>,
    first: &Expr,
    rest: &[(BinaryOp, Expr)],
    c: Ctx,
) -> Result<Value, EvalError> {
    let mut acc = eval(ev, first, c)?;
    for (op, rhs) in rest {
        if let Some(decided) = short_circuit(*op, &acc) {
            return Ok(decided);
        }
        let r = eval(ev, rhs, c)?;
        acc = apply_binary(ev, *op, &acc, &r);
    }
    Ok(acc)
}

/// The answer `and`/`or` give from the left operand alone, when it
/// settles the question: a false `and`, a true `or`.
fn short_circuit(op: BinaryOp, l: &Value) -> Option<Value> {
    match op {
        BinaryOp::And if !l.to_boolean() => Some(Value::Boolean(false)),
        BinaryOp::Or if l.to_boolean() => Some(Value::Boolean(true)),
        _ => None,
    }
}

/// Combine two evaluated operands.
///
/// For `and`/`or` the left side has already been through
/// [`short_circuit`] and did not decide, so the answer is the right
/// side's truth.
fn apply_binary(ev: &Ev<'_>, op: BinaryOp, l: &Value, r: &Value) -> Value {
    match op {
        BinaryOp::And | BinaryOp::Or => Value::Boolean(r.to_boolean()),
        BinaryOp::Union => {
            let mut out = l.nodes().unwrap_or(&[]).to_vec();
            out.extend_from_slice(r.nodes().unwrap_or(&[]));
            sort_document_order(&ev.order, &mut out);
            Value::NodeSet(out)
        }
        BinaryOp::Eq
        | BinaryOp::Ne
        | BinaryOp::Lt
        | BinaryOp::Le
        | BinaryOp::Gt
        | BinaryOp::Ge => Value::Boolean(compare(ev.doc, op, l, r)),
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Mod => Value::Number(arithmetic(
            op,
            l.to_number(ev.doc),
            r.to_number(ev.doc),
        )),
    }
}

fn arithmetic(op: BinaryOp, a: f64, b: f64) -> f64 {
    match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => a / b,
        BinaryOp::Mod => a % b,
        _ => unreachable!("not an arithmetic operator"),
    }
}
