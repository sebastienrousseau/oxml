// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The thirteen axes, each as a walk of the tree, and the node tests.
//!
//! An axis is a visitor over the candidates it selects from one
//! context node, yielded in *axis order*: document order on a forward
//! axis, reverse document order on a reverse one. That is the order
//! section 2.4 counts proximity position in, so the evaluator applies
//! predicates straight to what it is handed.
//!
//! None of the walks recurses. A document built through the tree API
//! can be deeper than any parsed one -- `Limits::max_depth` only
//! bounds parsing -- and a recursive descent of it would overflow the
//! stack on the first `//` step.

use alloc::vec::Vec;

use super::ast::{Axis, NodeTest};
use crate::tree::{Document, NodeId, NodeKind};

/// Call `f` with every node on `axis` from `node`, in axis order.
pub(crate) fn for_each_on_axis(
    doc: &Document,
    node: NodeId,
    axis: Axis,
    f: &mut dyn FnMut(NodeId),
) {
    match axis {
        Axis::Child => doc.children(node).iter().copied().for_each(f),
        Axis::SelfAxis => f(node),
        Axis::Parent => {
            if let Some(parent) = doc.parent(node) {
                f(parent);
            }
        }
        Axis::Attribute => {
            doc.attribute_nodes(node).iter().copied().for_each(f);
        }
        Axis::Namespace => visit_in_scope_namespaces(doc, node, f),
        _ => for_each_on_walking_axis(doc, node, axis, f),
    }
}

/// The axes that walk more than one edge.
fn for_each_on_walking_axis(
    doc: &Document,
    node: NodeId,
    axis: Axis,
    f: &mut dyn FnMut(NodeId),
) {
    match axis {
        Axis::Descendant | Axis::DescendantOrSelf => {
            visit_descendants(doc, node, axis == Axis::DescendantOrSelf, f);
        }
        Axis::Ancestor | Axis::AncestorOrSelf => {
            visit_ancestors(doc, node, axis == Axis::AncestorOrSelf, f);
        }
        Axis::FollowingSibling | Axis::PrecedingSibling => {
            visit_siblings(doc, node, axis.is_reverse(), f);
        }
        Axis::Following => visit_following(doc, node, f),
        Axis::Preceding => visit_preceding(doc, node, f),
        _ => {}
    }
}

/// Pre-order, iteratively: [`Document::descendants_of`] keeps its own
/// stack.
fn visit_descendants(
    doc: &Document,
    node: NodeId,
    include_self: bool,
    f: &mut dyn FnMut(NodeId),
) {
    if include_self {
        f(node);
    }
    doc.descendants_of(node).for_each(f);
}

/// Nearest first. An attribute's parent is its element, so the
/// element is among the attribute's ancestors, as section 2.2
/// requires.
fn visit_ancestors(
    doc: &Document,
    node: NodeId,
    include_self: bool,
    f: &mut dyn FnMut(NodeId),
) {
    if include_self {
        f(node);
    }
    let mut at = doc.parent(node);
    while let Some(parent) = at {
        f(parent);
        at = doc.parent(parent);
    }
}

/// The siblings after `node`, or before it nearest first. An
/// attribute or namespace node is not in its element's child list and
/// so has no siblings, which is what the specification says.
fn visit_siblings(
    doc: &Document,
    node: NodeId,
    reverse: bool,
    f: &mut dyn FnMut(NodeId),
) {
    let Some((siblings, index)) = position_among_siblings(doc, node) else {
        return;
    };
    if reverse {
        siblings[..index].iter().rev().copied().for_each(f);
    } else {
        siblings[index + 1..].iter().copied().for_each(f);
    }
}

/// The child list `node` sits in, and where.
fn position_among_siblings(
    doc: &Document,
    node: NodeId,
) -> Option<(&[NodeId], usize)> {
    let siblings = doc.children(doc.parent(node)?);
    let index = siblings.iter().position(|&s| s == node)?;
    Some((siblings, index))
}

/// Everything after `node` in document order that is not beneath it:
/// for each ancestor-or-self, the subtrees of its following siblings.
/// O(result + depth), not a scan of the document.
///
/// An attribute or namespace node is followed by its element's
/// content first, since that content is after it in document order
/// and is not its descendant.
fn visit_following(doc: &Document, node: NodeId, f: &mut dyn FnMut(NodeId)) {
    let mut at = node;
    if is_attached(doc, node) {
        let Some(owner) = doc.parent(node) else {
            return;
        };
        doc.descendants_of(owner).for_each(&mut *f);
        at = owner;
    }
    while let Some((siblings, index)) = position_among_siblings(doc, at) {
        for &sibling in &siblings[index + 1..] {
            f(sibling);
            doc.descendants_of(sibling).for_each(&mut *f);
        }
        at = doc.parent(at).unwrap_or(at);
    }
}

/// The mirror of [`visit_following`], nearest first: for each
/// ancestor-or-self, the subtrees of its preceding siblings, each in
/// reverse document order.
///
/// An attribute or namespace node's element is its parent and so an
/// ancestor, excluded by definition; what precedes the node is what
/// precedes its element.
fn visit_preceding(doc: &Document, node: NodeId, f: &mut dyn FnMut(NodeId)) {
    let mut at = node;
    if is_attached(doc, node) {
        let Some(owner) = doc.parent(node) else {
            return;
        };
        at = owner;
    }
    // One stack for every subtree, rather than an allocation per
    // preceding sibling.
    let mut stack = Vec::new();
    while let Some((siblings, index)) = position_among_siblings(doc, at) {
        for &sibling in siblings[..index].iter().rev() {
            visit_subtree_reversed(doc, sibling, &mut stack, f);
        }
        at = doc.parent(at).unwrap_or(at);
    }
}

/// `root` and everything beneath it in reverse document order: the
/// last descendant first, `root` last. An explicit stack of
/// (node, children already pushed) pairs, so depth costs heap.
fn visit_subtree_reversed(
    doc: &Document,
    root: NodeId,
    stack: &mut Vec<(NodeId, bool)>,
    f: &mut dyn FnMut(NodeId),
) {
    stack.push((root, false));
    while let Some((node, expanded)) = stack.pop() {
        if expanded {
            f(node);
            continue;
        }
        stack.push((node, true));
        // Pushed in document order, so the last child is popped --
        // and therefore visited -- first.
        stack.extend(doc.children(node).iter().map(|&child| (child, false)));
    }
}

/// Every namespace in scope for an element: its own declarations and
/// those inherited from its ancestors, nearest first. A prefix
/// declared nearer shadows the same prefix declared further up, so
/// the walk keeps the first node it sees for each prefix.
///
/// Section 5.4: only an element has namespace nodes. From any other
/// kind of node the axis is empty, however many declarations are in
/// scope around it.
fn visit_in_scope_namespaces(
    doc: &Document,
    node: NodeId,
    f: &mut dyn FnMut(NodeId),
) {
    if !doc.is_element(node) {
        return;
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut at = Some(node);
    while let Some(current) = at {
        for &ns in doc.namespace_nodes(current) {
            let Some(NodeKind::Namespace { prefix, uri }) = doc.kind(ns) else {
                continue;
            };
            if seen.contains(&prefix) {
                continue;
            }
            seen.push(prefix);
            // An undeclaration shadows the ancestor binding and then
            // contributes nothing itself: the prefix is out of scope,
            // not bound to the empty string.
            if !uri.is_empty() {
                f(ns);
            }
        }
        at = doc.parent(current);
    }
}

/// The union of `following::` over several context nodes, without
/// predicates, in one pre-order walk.
///
/// Section 2.4 defines a step over a node-set as the union of the
/// step over each node, and from `n` context nodes `following::`
/// yields O(n × N) candidates before the union collapses them -- a
/// quadratic cost for `//*/following::*`, whose answer is at most the
/// document. The union has a closed form: a node is after *some*
/// context node that is not its ancestor exactly when more context
/// nodes started before it than are among its ancestors. An attribute
/// or namespace context node counts as started at its element and is
/// nobody's ancestor, since its following is its element's content
/// and beyond. The walk yields document order, so no sort follows.
pub(crate) fn following_union(
    doc: &Document,
    context: &[NodeId],
    test: &NodeTest,
) -> Vec<NodeId> {
    let marks = Marks::new(context);
    let mut out = Vec::new();
    let mut started = 0_usize;
    let mut among_ancestors = 0_usize;
    let mut stack = alloc::vec![(doc.root(), false)];
    while let Some((node, leaving)) = stack.pop() {
        let is_context = usize::from(marks.has(node));
        if leaving {
            among_ancestors -= is_context;
            continue;
        }
        if started > among_ancestors
            && test_matches(doc, node, test, Axis::Following)
        {
            out.push(node);
        }
        started += is_context + marks.attached_to(doc, node);
        among_ancestors += is_context;
        stack.push((node, true));
        stack.extend(doc.children(node).iter().rev().map(|&c| (c, false)));
    }
    out
}

/// The union of `preceding::` over several context nodes, the mirror
/// of [`following_union`]: a node is before some context node that it
/// is not an ancestor of exactly when a context node starts after its
/// subtree ends. Decided on leaving each node, so the result is in
/// post-order and the caller sorts it.
pub(crate) fn preceding_union(
    doc: &Document,
    context: &[NodeId],
    test: &NodeTest,
) -> Vec<NodeId> {
    let marks = Marks::new(context);
    let mut out = Vec::new();
    let mut started = 0_usize;
    let mut stack = alloc::vec![(doc.root(), false)];
    while let Some((node, leaving)) = stack.pop() {
        if leaving {
            if context.len() > started
                && test_matches(doc, node, test, Axis::Preceding)
            {
                out.push(node);
            }
            continue;
        }
        started += usize::from(marks.has(node)) + marks.attached_to(doc, node);
        stack.push((node, true));
        stack.extend(doc.children(node).iter().rev().map(|&c| (c, false)));
    }
    out
}

/// Which arena slots hold a context node.
struct Marks {
    slots: Vec<bool>,
}

impl Marks {
    fn new(context: &[NodeId]) -> Self {
        let len = context.iter().map(|id| id.index() + 1).max().unwrap_or(0);
        let mut slots = alloc::vec![false; len];
        for id in context {
            slots[id.index()] = true;
        }
        Self { slots }
    }

    fn has(&self, id: NodeId) -> bool {
        self.slots.get(id.index()).copied().unwrap_or(false)
    }

    /// How many of `node`'s attribute and namespace nodes are marked.
    fn attached_to(&self, doc: &Document, node: NodeId) -> usize {
        doc.attribute_nodes(node)
            .iter()
            .chain(doc.namespace_nodes(node))
            .filter(|&&id| self.has(id))
            .count()
    }
}

/// Whether `node` hangs off an element without being its child: an
/// attribute or a namespace node.
fn is_attached(doc: &Document, node: NodeId) -> bool {
    matches!(
        doc.kind(node),
        Some(NodeKind::Attr(_) | NodeKind::Namespace { .. })
    )
}

/// Whether `node`, a candidate from `axis`, passes `test`.
///
/// The principal node type of the attribute axis is attribute, of the
/// namespace axis namespace, and of every other axis element: `*`
/// and a name test match that type.
pub(crate) fn test_matches(
    doc: &Document,
    node: NodeId,
    test: &NodeTest,
    axis: Axis,
) -> bool {
    match axis {
        Axis::Attribute => attribute_test_matches(doc, node, test),
        Axis::Namespace => namespace_test_matches(doc, node, test),
        _ => element_test_matches(doc, node, test),
    }
}

/// On the attribute axis the candidates are attribute nodes, so `*`
/// means "any attribute" and a name test matches the attribute's
/// expanded name.
fn attribute_test_matches(
    doc: &Document,
    node: NodeId,
    test: &NodeTest,
) -> bool {
    let Some(NodeKind::Attr(attribute)) = doc.kind(node) else {
        return false;
    };
    match test {
        NodeTest::Wildcard | NodeTest::Any => true,
        NodeTest::Name { namespace, local } => {
            doc.name(attribute.name).is_some_and(|name| {
                &name.local == local
                    && name.namespace.as_deref() == namespace.as_deref()
            })
        }
        // `@*:local` -- the local name, in whatever namespace.
        NodeTest::AnyNamespace { local } => doc
            .name(attribute.name)
            .is_some_and(|name| &name.local == local),
        _ => false,
    }
}

/// On the namespace axis the candidates are namespace nodes, whose
/// name is the declared prefix. A name test matches that prefix, so
/// `namespace::m` selects the node declaring `m`. The test's own
/// namespace part is never consulted: a namespace node is in no
/// namespace, and a prefixed test could not match one.
fn namespace_test_matches(
    doc: &Document,
    node: NodeId,
    test: &NodeTest,
) -> bool {
    let Some(NodeKind::Namespace { prefix, .. }) = doc.kind(node) else {
        return false;
    };
    match test {
        NodeTest::Wildcard | NodeTest::Any => true,
        NodeTest::Name { namespace, local } => {
            namespace.is_none() && local == prefix
        }
        // A namespace node is in no namespace, so "any namespace"
        // degenerates to the same match as an unprefixed test.
        NodeTest::AnyNamespace { local } => local == prefix,
        _ => false,
    }
}

/// On every other axis the principal node type is element.
fn element_test_matches(doc: &Document, node: NodeId, test: &NodeTest) -> bool {
    match test {
        NodeTest::Any => true,
        NodeTest::Wildcard => doc.is_element(node),
        NodeTest::Name { namespace, local } => doc
            .element_name(node)
            .is_some_and(|e| e.local == *local && e.namespace == *namespace),
        // `*:local` -- the local name, in whatever namespace,
        // including none. The whole point is not having to bind a
        // prefix per namespace to ask a structural question.
        NodeTest::AnyNamespace { local } => {
            doc.element_name(node).is_some_and(|e| e.local == *local)
        }
        kind => kind_test_matches(doc.kind(node), kind),
    }
}

/// `text()`, `comment()` and `processing-instruction()`, the last
/// optionally narrowed to one target.
fn kind_test_matches(kind: Option<NodeKind<'_>>, test: &NodeTest) -> bool {
    match (test, kind) {
        (NodeTest::Text, Some(NodeKind::Text(_)))
        | (NodeTest::Comment, Some(NodeKind::Comment(_))) => true,
        // With a target, only instructions with that target match;
        // without one, every processing instruction does.
        (
            NodeTest::ProcessingInstruction(want),
            Some(NodeKind::ProcessingInstruction { target, .. }),
        ) => want.as_ref().is_none_or(|w| w == target),
        _ => false,
    }
}
