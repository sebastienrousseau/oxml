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

use alloc::collections::BTreeMap;
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
    /// The attribute's name, interned.
    ///
    /// A handle rather than an `ExpandedName` because attribute names
    /// repeat as heavily as element names do -- a catalogue with 2,000
    /// items and three attributes each has three distinct names, and
    /// storing them per attribute allocated thousands of strings to
    /// hold three values. Resolve it with [`Document::name`].
    pub name: NameId,
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
        /// The element's name, interned.
        ///
        /// Names repeat heavily — a 500,001-element document in the
        /// benchmark suite has **six** distinct ones — so storing an
        /// `ExpandedName` per element allocated half a million strings
        /// to hold six values. Resolve it with
        /// [`Document::element_name`].
        name: NameId,
        /// Where this element's attribute nodes live in the document's
        /// flat attribute arena, as `(start, len)`. Resolve it with
        /// [`Document::attribute_nodes`].
        ///
        /// Attributes are pushed together when the start tag is read,
        /// so they are already contiguous — no scratch stack needed,
        /// unlike children.
        attributes: (u32, u32),
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

/// A handle to an interned [`ExpandedName`].
///
/// Opaque: compare handles for equality to compare names, which is a
/// `u32` compare rather than two string compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameId(pub(crate) u32);

// Two ids being equal means the names were written identically, prefix
// included -- it does **not** mean they are the same name. `p:a` and
// `q:a` with both prefixes bound to one namespace are the same expanded
// name and get different ids. Compare `Document::name` values, not ids,
// wherever the question is whether two names refer to the same thing.

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) parent: Option<NodeId>,
    /// Where this node's children live in the document's flat child
    /// arena, as `(start, len)`.
    ///
    /// A `Vec` per node meant one allocation for every element that has
    /// children — about a million on the benchmark document. Children
    /// are contiguous in `child_ids` because they are copied there as a
    /// block when the element closes.
    pub(crate) children: (u32, u32),
}

/// A parsed XML document.
///
/// Construct one with [`crate::parse`].
#[derive(Debug, Clone)]
pub struct Document {
    pub(crate) nodes: Vec<Node>,
    /// Every distinct element name in the document, once.
    pub(crate) names: Vec<ExpandedName>,
    /// The prefix each interned name was written with, parallel to
    /// `names`.
    ///
    /// Kept separately rather than added to [`ExpandedName`] because a
    /// prefix is not part of a name's identity -- two prefixes bound to
    /// one namespace name the same thing -- but `name()` in `XPath` has
    /// to report a usable `QName`, and resolution discards the prefix.
    /// Namespace declarations are not retained as attributes (they are
    /// namespace nodes, not attributes), so there is nothing else left
    /// to reconstruct it from.
    pub(crate) name_prefixes: Vec<Option<String>>,
    /// All attribute lists, concatenated.
    pub(crate) attr_ids: Vec<NodeId>,
    /// All child lists, concatenated. Each node's slice is described by
    /// its `children` range.
    pub(crate) child_ids: Vec<NodeId>,
    /// Children of elements still open, innermost last.
    ///
    /// A single shared stack rather than one buffer per element: a
    /// node's children are only known to be complete when its end tag
    /// arrives, at which point they are moved into `child_ids` as a
    /// contiguous block.
    pub(crate) scratch: Vec<NodeId>,
    /// Elements carrying an `ID`-typed attribute, by that attribute's
    /// value.
    ///
    /// Built while parsing rather than searched for on demand, because
    /// what counts as an ID is a DTD declaration and the DTD is not
    /// kept past parsing. Empty for the overwhelming majority of
    /// documents, which declare no `ID` attribute at all.
    pub(crate) ids: BTreeMap<String, NodeId>,
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
            children: (0, 0),
        });
        Self {
            nodes: v,
            names: Vec::new(),
            name_prefixes: Vec::new(),
            attr_ids: Vec::new(),
            child_ids: Vec::with_capacity(nodes),
            scratch: Vec::new(),
            ids: BTreeMap::new(),
        }
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
            children: (0, 0),
        });
        // Held on the scratch stack until `parent` closes, so that its
        // children can be written to `child_ids` as one block.
        self.scratch.push(id);
        id
    }

    /// Where the scratch stack currently ends.
    ///
    /// Take this before parsing an element's content and pass it back
    /// to [`Document::finish_children`] afterwards.
    pub(crate) fn scratch_mark(&self) -> usize {
        self.scratch.len()
    }

    /// Move everything pushed since `mark` into `parent`'s child list.
    pub(crate) fn finish_children(&mut self, parent: NodeId, mark: usize) {
        let start = self.child_ids.len();
        self.child_ids.extend_from_slice(&self.scratch[mark..]);
        self.scratch.truncate(mark);
        if let Some(n) = self.nodes.get_mut(parent.0) {
            n.children = (
                u32::try_from(start).unwrap_or(u32::MAX),
                u32::try_from(self.child_ids.len() - start).unwrap_or(0),
            );
        }
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
            children: (0, 0),
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
        self.nodes.get(id.0).map_or(&[], |n| {
            let (start, len) = (n.children.0 as usize, n.children.1 as usize);
            // `saturating_add` because on a 32-bit target -- and this
            // crate builds for three bare-metal ones -- two `u32`
            // values can overflow a `usize`. No document that large
            // fits in such a machine's memory, so this is unreachable
            // rather than merely unlikely; it costs nothing to make
            // the totality obvious instead of argued.
            self.child_ids
                .get(start..start.saturating_add(len))
                .unwrap_or(&[])
        })
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
            Some(NodeKind::Element { name, .. }) => self.name(*name),
            _ => None,
        }
    }

    /// Resolve an interned name handle.
    #[must_use]
    pub fn name(&self, id: NameId) -> Option<&ExpandedName> {
        self.names.get(id.0 as usize)
    }

    /// The element whose `ID`-typed attribute has this value.
    ///
    /// An attribute is an ID because the DTD declared it as one, not
    /// because it is spelled `id`; a document with no DTD has no IDs
    /// and this always answers `None`.
    #[must_use]
    pub fn element_by_id(&self, value: &str) -> Option<NodeId> {
        self.ids.get(value).copied()
    }

    /// The prefix an interned name was written with, if it had one.
    ///
    /// `None` covers both an unprefixed name and an id from another
    /// document. Use it with [`Document::name`] to rebuild the `QName`
    /// as it appeared in the source.
    #[must_use]
    pub fn prefix(&self, id: NameId) -> Option<&str> {
        self.name_prefixes.get(id.0 as usize)?.as_deref()
    }

    /// The ids of an element's attribute nodes.
    #[must_use]
    pub fn attribute_nodes(&self, id: NodeId) -> &[NodeId] {
        match self.kind(id) {
            Some(NodeKind::Element { attributes, .. }) => {
                let (start, len) =
                    (attributes.0 as usize, attributes.1 as usize);
                self.attr_ids
                    .get(start..start.saturating_add(len))
                    .unwrap_or(&[])
            }
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
            .find(|a| self.name(a.name).is_some_and(|n| n.local == local))
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
