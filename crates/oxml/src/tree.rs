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
pub struct Attribute<'a> {
    /// The attribute's name, interned.
    ///
    /// A handle rather than an `ExpandedName` because attribute names
    /// repeat as heavily as element names do -- a catalogue with 2,000
    /// items and three attributes each has three distinct names, and
    /// storing them per attribute allocated thousands of strings to
    /// hold three values. Resolve it with [`Document::name`].
    pub name: NameId,
    /// The attribute's value, with entities already resolved.
    ///
    /// Borrowed from the document. Where the value appears verbatim in
    /// the source -- which is almost always -- this is a slice of the
    /// input the document owns and cost nothing to produce. Where
    /// entity expansion or attribute-value normalisation rewrote it,
    /// it is a slice of the document's side table of expanded values.
    pub value: &'a str,
}

/// What a node is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind<'a> {
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
        /// Where this element's namespace nodes live in the document's
        /// flat namespace arena, as `(start, len)`. Resolve it with
        /// [`Document::namespace_nodes`].
        ///
        /// Only the declarations written on *this* element. Everything
        /// else in scope belongs to an ancestor and is reached by
        /// walking up.
        namespaces: (u32, u32),
    },
    /// An attribute.
    ///
    /// Attributes are real nodes in the arena so that `XPath`'s
    /// `attribute::` axis can yield them and `string()` can return
    /// their value. They are deliberately *not* in their element's
    /// `children`, because `child::` must not see them.
    Attr(Attribute<'a>),
    /// Character data. Adjacent runs are merged during parsing, so a
    /// caller never sees two text siblings in a row.
    Text(&'a str),
    /// A comment's content, without the `<!--` and `-->`.
    Comment(&'a str),
    /// A namespace declaration in scope for an element.
    ///
    /// Like attributes, these are real nodes so `XPath`'s `namespace::`
    /// axis can yield them, and like attributes they are deliberately
    /// **not** in their element's `children`. One node exists per
    /// `xmlns` declaration in the source, not per element the
    /// declaration is in scope for -- the axis walks ancestors and
    /// applies shadowing, so a document does not pay for inheritance.
    Namespace {
        /// The prefix declared, or empty for the default namespace.
        ///
        /// This is the namespace node's name in `XPath`'s data model:
        /// `local-name()` returns it, and it is empty rather than
        /// absent for `xmlns="..."`.
        prefix: &'a str,
        /// The URI bound to it, which is the node's string-value.
        ///
        /// Empty for an *undeclaration* (`xmlns=""`, XML 1.1 only).
        /// Such a node exists so it can shadow the same prefix on an
        /// ancestor, but `namespace::` does not report it: the prefix
        /// is out of scope, not bound to the empty string.
        uri: &'a str,
    },
    /// A processing instruction.
    ProcessingInstruction {
        /// The PI target, e.g. `xml-stylesheet`.
        target: &'a str,
        /// Everything after the target, verbatim.
        data: &'a str,
    },
}

/// Character data belonging to the document, as stored.
///
/// The point of the whole arena: a text node or attribute value that
/// appears verbatim in the source is a range into the input the
/// document already owns, and costs no allocation at all.
///
/// Not everything can be. `&amp;` in a value, a `CDATA` section inside
/// a text run, or attribute-value normalisation collapsing a tab to a
/// space all produce characters that are nowhere in the input as
/// written. Those go in a side table, which is empty for the
/// overwhelming majority of documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chars {
    /// A range into [`Document::input`], as `(start, len)`.
    Span(u32, u32),
    /// An index into [`Document::expanded`], for text that the input
    /// does not contain verbatim.
    Expanded(u32),
}

impl Chars {
    /// The empty string, which every document can spell.
    pub(crate) const EMPTY: Self = Self::Span(0, 0);
}

/// A node as stored, with character data unresolved.
///
/// The counterpart of [`NodeKind`], which is the same information with
/// every [`Chars`] resolved against the document that owns it. They
/// are separate types because the stored form must not borrow -- the
/// document owns both the nodes and the text they point into, and a
/// node holding a `&str` into its own document would be
/// self-referential.
#[derive(Debug, Clone)]
pub(crate) enum NodeData {
    Root,
    Element {
        name: NameId,
        attributes: (u32, u32),
        namespaces: (u32, u32),
    },
    Attr {
        name: NameId,
        value: Chars,
    },
    Text(Chars),
    Comment(Chars),
    Namespace {
        prefix: Chars,
        uri: Chars,
    },
    ProcessingInstruction {
        target: Chars,
        data: Chars,
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
    pub(crate) data: NodeData,
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
    /// The document text, decoded and line-ending normalised.
    ///
    /// Owned rather than borrowed from the caller. `parse_bytes` on a
    /// UTF-16 or ISO-8859-1 document has nothing to borrow *from* --
    /// the decoded string is a temporary the parser made -- so a
    /// lifetime parameter would serve one entry point and not the
    /// other. See `doc/adr/0007-owned-strings-for-now.md`.
    ///
    /// Text nodes, comments and attribute values are ranges into this
    /// rather than strings of their own, which is what took the
    /// allocation count down.
    pub(crate) input: String,
    /// Character data the input does not contain verbatim.
    ///
    /// Entity expansion, `CDATA` merged into a text run, and
    /// attribute-value normalisation all produce text that is nowhere
    /// in the source as written. Empty for most documents.
    pub(crate) expanded: Vec<String>,
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
    /// All namespace-node lists, concatenated.
    pub(crate) ns_ids: Vec<NodeId>,
}

impl Document {
    /// A document sized for roughly `nodes` entries.
    ///
    /// The parser can estimate the node count in one cheap pass before
    /// parsing — every element, comment and processing instruction
    /// begins with `<`. Without this the arena reallocates and copies
    /// on the way to a million nodes, and each growth is an allocation
    /// plus a memcpy of everything so far.
    /// A document that owns nothing, for swapping out of a borrow.
    ///
    /// [`Document::with_capacity`] allocates even for zero nodes: it
    /// reserves room for the root and pushes it. The streaming reader
    /// swaps a placeholder in and out of its state twice per event,
    /// so that allocation was paid **per event** -- measured at 4.75
    /// allocations per event against 0.50 per node for a parse, which
    /// is why reading a document as events was twice as slow as
    /// parsing it into a tree.
    ///
    /// This one allocates nothing. It has no root node either, so it
    /// is only fit to be swapped straight back out again.
    pub(crate) const fn placeholder() -> Self {
        Self {
            input: String::new(),
            expanded: Vec::new(),
            nodes: Vec::new(),
            names: Vec::new(),
            name_prefixes: Vec::new(),
            attr_ids: Vec::new(),
            child_ids: Vec::new(),
            scratch: Vec::new(),
            ids: BTreeMap::new(),
            ns_ids: Vec::new(),
        }
    }

    pub(crate) fn with_capacity(nodes: usize) -> Self {
        let mut v = Vec::with_capacity(nodes.saturating_add(1));
        v.push(Node {
            data: NodeData::Root,
            parent: None,
            children: (0, 0),
        });
        Self {
            input: String::new(),
            expanded: Vec::new(),
            nodes: v,
            names: Vec::new(),
            name_prefixes: Vec::new(),
            attr_ids: Vec::new(),
            child_ids: Vec::with_capacity(nodes),
            scratch: Vec::new(),
            ids: BTreeMap::new(),
            ns_ids: Vec::new(),
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

    pub(crate) fn push(&mut self, data: NodeData, parent: NodeId) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            data,
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
        data: NodeData,
        parent: NodeId,
    ) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            data,
            parent: Some(parent),
            children: (0, 0),
        });
        id
    }

    /// Resolve stored character data against the document.
    ///
    /// A `Span` is a slice of the input; an `Expanded` is a slice of
    /// the side table. Either way the caller gets a `&str` borrowed
    /// from the document and no allocation happens.
    pub(crate) fn chars(&self, c: Chars) -> &str {
        match c {
            Chars::Span(start, len) => {
                let (start, len) = (start as usize, len as usize);
                // A span is written by the parser from positions it
                // scanned, so it is in range and on a character
                // boundary. `get` rather than indexing so that a bug
                // is an empty string and not a panic: this crate's
                // contract is that no input panics it.
                self.input.get(start..start + len).unwrap_or_default()
            }
            Chars::Expanded(i) => self
                .expanded
                .get(i as usize)
                .map_or("", alloc::string::String::as_str),
        }
    }

    /// The kind of a node, or `None` if the id is not from this
    /// document.
    ///
    /// Returns a *view*: character data is resolved against the
    /// document and borrowed from it, so this allocates nothing.
    #[must_use]
    pub fn kind(&self, id: NodeId) -> Option<NodeKind<'_>> {
        let node = self.nodes.get(id.0)?;
        Some(match &node.data {
            NodeData::Root => NodeKind::Root,
            NodeData::Element {
                name,
                attributes,
                namespaces,
            } => NodeKind::Element {
                name: *name,
                attributes: *attributes,
                namespaces: *namespaces,
            },
            NodeData::Attr { name, value } => NodeKind::Attr(Attribute {
                name: *name,
                value: self.chars(*value),
            }),
            NodeData::Text(c) => NodeKind::Text(self.chars(*c)),
            NodeData::Comment(c) => NodeKind::Comment(self.chars(*c)),
            NodeData::Namespace { prefix, uri } => NodeKind::Namespace {
                prefix: self.chars(*prefix),
                uri: self.chars(*uri),
            },
            NodeData::ProcessingInstruction { target, data } => {
                NodeKind::ProcessingInstruction {
                    target: self.chars(*target),
                    data: self.chars(*data),
                }
            }
        })
    }

    /// Add to the side table of text the input does not contain.
    pub(crate) fn push_expanded(&mut self, text: &str) -> Chars {
        let Ok(i) = u32::try_from(self.expanded.len()) else {
            return Chars::EMPTY;
        };
        self.expanded.push(text.into());
        Chars::Expanded(i)
    }

    /// The stored form of a node, for the parser to amend in place.
    ///
    /// Element attribute and namespace ranges are only known once the
    /// start tag has been fully read, so the node is pushed first and
    /// filled in after.
    pub(crate) fn data_mut(&mut self, id: NodeId) -> Option<&mut NodeData> {
        self.nodes.get_mut(id.0).map(|n| &mut n.data)
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
            Some(NodeKind::Element { name, .. }) => self.name(name),
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

    /// The namespace nodes an element *declares*.
    ///
    /// Not everything in scope: a prefix declared on an ancestor is in
    /// scope here but its node belongs to that ancestor. `XPath`'s
    /// `namespace::` axis walks up and applies shadowing; this is the
    /// raw declaration list.
    #[must_use]
    pub fn namespace_nodes(&self, id: NodeId) -> &[NodeId] {
        match self.kind(id) {
            Some(NodeKind::Element { namespaces, .. }) => {
                let (start, len) =
                    (namespaces.0 as usize, namespaces.1 as usize);
                self.ns_ids
                    .get(start..start.saturating_add(len))
                    .unwrap_or(&[])
            }
            _ => &[],
        }
    }

    /// An element's attributes, in document order.
    #[must_use]
    pub fn attributes(&self, id: NodeId) -> Vec<Attribute<'_>> {
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
            .map(|a| a.value)
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
            Some(NodeKind::Attr(a)) => out.push_str(a.value),
            // A namespace node's string-value is the URI, not the
            // prefix -- the prefix is its *name*.
            Some(NodeKind::Namespace { uri, .. }) => out.push_str(uri),
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
