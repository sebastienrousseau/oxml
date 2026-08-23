// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The document tree.
//!
//! Nodes live in a single arena and are addressed by [`NodeId`], a
//! plain index. That choice is what lets the tree be cyclic-by-shape
//! (every node knows its parent) without `Rc`, `RefCell`, or `unsafe`:
//! the parent link is an index, not a pointer, so there is no ownership
//! cycle for the borrow checker to reject.
//!
//! The trade is that a [`NodeId`] is only meaningful against the
//! [`Document`] that issued it. Using one against a different document
//! is a logic error; the accessors return `None` rather than panicking.

use alloc::string::String;
use alloc::vec::Vec;

/// A handle to a node within a [`Document`].
///
/// Cheap to copy. Only valid for the document that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub(crate) usize);

impl NodeId {
    /// The index of this node within its document's arena.
    ///
    /// Exposed because it is useful for building side tables keyed by
    /// node, which is how callers usually annotate a tree without
    /// mutating it.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// An expanded name: a local part plus an optional namespace URI.
///
/// XML names compare by *namespace URI and local name*, never by
/// prefix — `<a:x xmlns:a="u"/>` and `<b:x xmlns:b="u"/>` are the same
/// element. Keeping the URI here rather than the prefix is what makes
/// that comparison correct by construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExpandedName {
    /// The namespace URI, if the name is in a namespace.
    pub namespace: Option<String>,
    /// The local part of the name, without any prefix.
    pub local: String,
}

impl ExpandedName {
    /// Build a name in no namespace.
    #[must_use]
    pub fn local(local: impl Into<String>) -> Self {
        Self {
            namespace: None,
            local: local.into(),
        }
    }

    /// Build a namespaced name.
    #[must_use]
    pub fn qualified(
        namespace: impl Into<String>,
        local: impl Into<String>,
    ) -> Self {
        Self {
            namespace: Some(namespace.into()),
            local: local.into(),
        }
    }
}

/// An attribute on an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// The attribute's expanded name.
    pub name: ExpandedName,
    /// The attribute's value, with entities already resolved.
    pub value: String,
}

/// What a node is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// The document root. Exactly one per [`Document`], and it is not
    /// the same thing as the root *element*.
    Root,
    /// An element.
    Element {
        /// The element's expanded name.
        name: ExpandedName,
        /// Ids of this element's attribute nodes, in document order.
        attributes: Vec<NodeId>,
    },
    /// An attribute.
    ///
    /// Attributes are real nodes in the arena so that `XPath`'s
    /// `attribute::` axis can yield them and `string()` can return
    /// their value. They are deliberately *not* in their element's
    /// `children`, because `child::` must not see them.
    Attr(Attribute),
    /// Character data. Adjacent runs are merged during parsing, so a
    /// caller never sees two text siblings in a row.
    Text(String),
    /// A comment's content, without the `<!--` and `-->`.
    Comment(String),
    /// A processing instruction.
    ProcessingInstruction {
        /// The PI target, e.g. `xml-stylesheet`.
        target: String,
        /// Everything after the target, verbatim.
        data: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
}

/// A parsed XML document.
///
/// Construct one with [`crate::parse`].
#[derive(Debug, Clone)]
pub struct Document {
    pub(crate) nodes: Vec<Node>,
}

impl Document {
    /// A document sized for roughly `nodes` entries.
    ///
    /// The parser can estimate the node count in one cheap pass before
    /// parsing — every element, comment and processing instruction
    /// begins with `<`. Without this the arena reallocates and copies
    /// on the way to a million nodes, and each growth is an allocation
    /// plus a memcpy of everything so far.
    pub(crate) fn with_capacity(nodes: usize) -> Self {
        let mut v = Vec::with_capacity(nodes.saturating_add(1));
        v.push(Node {
            kind: NodeKind::Root,
            parent: None,
            children: Vec::new(),
        });
        Self { nodes: v }
    }

    /// The document root.
    ///
    /// This is the node *containing* the root element, mirroring
    /// `XPath`'s document node, not the root element itself. Use
    /// [`Document::root_element`] for that.
    #[must_use]
    pub const fn root(&self) -> NodeId {
        NodeId(0)
    }

    pub(crate) fn push(&mut self, kind: NodeKind, parent: NodeId) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            kind,
            parent: Some(parent),
            children: Vec::new(),
        });
        self.nodes[parent.0].children.push(id);
        id
    }

    /// Append a node without linking it into its parent's children.
    ///
    /// Attributes need a parent (so `parent::` works from them) but
    /// must not appear on the `child::` axis.
    pub(crate) fn push_detached(
        &mut self,
        kind: NodeKind,
        parent: NodeId,
    ) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            kind,
            parent: Some(parent),
            children: Vec::new(),
        });
        id
    }

    /// The kind of a node, or `None` if the id is not from this
    /// document.
    #[must_use]
    pub fn kind(&self, id: NodeId) -> Option<&NodeKind> {
        self.nodes.get(id.0).map(|n| &n.kind)
    }

    pub(crate) fn kind_mut(&mut self, id: NodeId) -> Option<&mut NodeKind> {
        self.nodes.get_mut(id.0).map(|n| &mut n.kind)
    }

    /// A node's parent, or `None` for the root.
    #[must_use]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id.0).and_then(|n| n.parent)
    }

    /// A node's children, in document order.
    ///
    /// Returns an empty slice for a node with no children, and for an
    /// id that does not belong to this document.
    #[must_use]
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.nodes.get(id.0).map_or(&[], |n| n.children.as_slice())
    }

    /// The document's root element, if it has one.
    ///
    /// A document is only well-formed with exactly one, but a
    /// [`Document`] can also be inspected mid-construction, so this
    /// returns an `Option` rather than asserting.
    #[must_use]
    pub fn root_element(&self) -> Option<NodeId> {
        self.children(self.root())
            .iter()
            .copied()
            .find(|id| self.is_element(*id))
    }

    /// Whether a node is an element.
    #[must_use]
    pub fn is_element(&self, id: NodeId) -> bool {
        matches!(self.kind(id), Some(NodeKind::Element { .. }))
    }

    /// An element's expanded name, or `None` for other node kinds.
    #[must_use]
    pub fn element_name(&self, id: NodeId) -> Option<&ExpandedName> {
        match self.kind(id) {
            Some(NodeKind::Element { name, .. }) => Some(name),
            _ => None,
        }
    }

    /// The ids of an element's attribute nodes.
    #[must_use]
    pub fn attribute_nodes(&self, id: NodeId) -> &[NodeId] {
        match self.kind(id) {
            Some(NodeKind::Element { attributes, .. }) => attributes,
            _ => &[],
        }
    }

    /// An element's attributes, in document order.
    #[must_use]
    pub fn attributes(&self, id: NodeId) -> Vec<&Attribute> {
        self.attribute_nodes(id)
            .iter()
            .filter_map(|a| match self.kind(*a) {
                Some(NodeKind::Attr(at)) => Some(at),
                _ => None,
            })
            .collect()
    }

    /// Look up an attribute by local name, ignoring namespaces.
    ///
    /// This is the common case: unprefixed attributes are in *no*
    /// namespace, not the element's namespace, so matching on the
    /// local part is what callers almost always mean.
    #[must_use]
    pub fn attribute(&self, id: NodeId, local: &str) -> Option<&str> {
        self.attributes(id)
            .into_iter()
            .find(|a| a.name.local == local)
            .map(|a| a.value.as_str())
    }

    /// The concatenated text of a node and its descendants.
    ///
    /// This is `XPath`'s `string-value`: comments and processing
    /// instructions contribute nothing, which is why it is not simply
    /// "all the text in the subtree".
    #[must_use]
    pub fn text(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        match self.kind(id) {
            Some(NodeKind::Attr(a)) => out.push_str(&a.value),
            Some(NodeKind::Text(t)) => out.push_str(t),
            Some(NodeKind::Root | NodeKind::Element { .. }) => {
                for child in self.children(id) {
                    self.collect_text(*child, out);
                }
            }
            _ => {}
        }
    }

    /// Every node in the document, in document order.
    ///
    /// Document order is the order nodes' start tags appear in the
    /// source, which for this arena is simply ascending index — the
    /// parser only ever appends.
    pub fn descendants(&self) -> impl Iterator<Item = NodeId> + '_ {
        (0..self.nodes.len()).map(NodeId)
    }

    /// The number of nodes, including the document root.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the document holds nothing but its root node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }
}
