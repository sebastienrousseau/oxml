// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Per-evaluation reservations, made before traversals and allocation.

use core::cell::Cell;

use super::{EvalError, EvalLimits, Value};
use crate::tree::{Document, NodeKind};

pub(super) struct Budget {
    limits: Option<EvalLimits>,
    work: Cell<usize>,
    strings: Cell<usize>,
    /// Upper bound for a complete document walk and its byte comparisons.
    pub(super) document_cost: usize,
    pub(super) name_bytes: usize,
}

impl Budget {
    pub(super) fn new(
        doc: &Document,
        limits: Option<EvalLimits>,
    ) -> Result<Self, EvalError> {
        let budget = Self {
            limits,
            work: Cell::new(limits.map_or(usize::MAX, |l| l.max_work)),
            strings: Cell::new(
                limits.map_or(usize::MAX, |l| l.max_string_bytes),
            ),
            document_cost: 0,
            name_bytes: 0,
        };
        if limits.is_none() {
            return Ok(budget);
        }
        budget.nodes(doc.nodes.len())?;
        let descriptors = doc
            .nodes
            .len()
            .saturating_add(doc.names.len())
            .saturating_add(doc.expanded.len())
            .saturating_add(doc.name_prefixes.len());
        budget.charge(descriptors)?;
        let (document_cost, name_bytes) = document_sizes(doc);
        Ok(Self {
            document_cost,
            name_bytes,
            ..budget
        })
    }

    pub(super) const fn bounded(&self) -> bool {
        self.limits.is_some()
    }

    pub(super) fn charge(&self, units: usize) -> Result<(), EvalError> {
        if self.bounded() {
            reserve(&self.work, units, "work")?;
        }
        Ok(())
    }

    pub(super) fn string(&self, bytes: usize) -> Result<(), EvalError> {
        if self.bounded() {
            self.charge(bytes)?;
            reserve(&self.strings, bytes, "string bytes")?;
        }
        Ok(())
    }

    pub(super) fn nodes(&self, count: usize) -> Result<(), EvalError> {
        if self.limits.is_some_and(|l| count > l.max_nodes) {
            return Err(EvalError::LimitExceeded("nodes"));
        }
        Ok(())
    }

    pub(super) fn value(&self, value: &Value) -> Result<(), EvalError> {
        match value {
            Value::NodeSet(nodes) => self.nodes(nodes.len()),
            _ => Ok(()),
        }
    }

    /// Reserve upper bounds for the comparison engine's temporary strings.
    pub(super) fn comparison(
        &self,
        left: &Value,
        right: &Value,
    ) -> Result<(), EvalError> {
        if !self.bounded() {
            return Ok(());
        }
        let copies =
            comparison_copies(left).saturating_add(comparison_copies(right));
        let bytes = scalar_bytes(left)
            .max(scalar_bytes(right))
            .max(self.document_cost);
        self.string(copies.saturating_mul(bytes))?;
        self.charge(
            copies
                .saturating_mul(bytes)
                .saturating_mul(sort_factor(copies)),
        )
    }
}

fn document_sizes(doc: &Document) -> (usize, usize) {
    let data = doc
        .expanded
        .iter()
        .fold(doc.input.len(), |n, s| n.saturating_add(s.len()));
    let names = doc
        .names
        .iter()
        .map(|n| {
            n.local.len().saturating_add(
                n.namespace.as_ref().map_or(0, alloc::string::String::len),
            )
        })
        .max()
        .unwrap_or(0);
    let prefixes = doc
        .name_prefixes
        .iter()
        .flatten()
        .map(alloc::string::String::len)
        .max()
        .unwrap_or(0);
    let node_names = doc
        .nodes()
        .filter_map(|id| super::names::name_parts(doc, id))
        .map(|(local, uri)| local.len().saturating_add(uri.map_or(0, str::len)))
        .max()
        .unwrap_or(0);
    let name_bytes = names
        .max(node_names)
        .saturating_add(prefixes)
        .saturating_add(1);
    let document_cost =
        data.saturating_add(doc.nodes.len().saturating_mul(name_bytes));
    (document_cost, name_bytes)
}

fn reserve(
    remaining: &Cell<usize>,
    amount: usize,
    resource: &'static str,
) -> Result<(), EvalError> {
    let next = remaining
        .get()
        .checked_sub(amount)
        .ok_or(EvalError::LimitExceeded(resource))?;
    remaining.set(next);
    Ok(())
}

fn comparison_copies(value: &Value) -> usize {
    value
        .nodes()
        .map_or(2, |nodes| nodes.len().saturating_mul(2))
}

fn scalar_bytes(value: &Value) -> usize {
    match value {
        Value::String(s) => s.len(),
        _ => 1024, // Includes the longest finite f64 decimal rendering.
    }
}

pub(super) fn sort_factor(count: usize) -> usize {
    (usize::BITS - count.leading_zeros()) as usize + 1
}

/// The exact output length without first allocating the string-value.
pub(super) fn string_len(doc: &Document, id: crate::NodeId) -> usize {
    match doc.kind(id) {
        Some(NodeKind::Attr(a)) => a.value.len(),
        Some(NodeKind::Text(s) | NodeKind::Comment(s)) => s.len(),
        Some(NodeKind::ProcessingInstruction { data, .. }) => data.len(),
        Some(NodeKind::Namespace { uri, .. }) => uri.len(),
        _ => doc
            .descendants_of(id)
            .filter_map(|n| match doc.kind(n) {
                Some(NodeKind::Text(s)) => Some(s.len()),
                _ => None,
            })
            .fold(0, usize::saturating_add),
    }
}
