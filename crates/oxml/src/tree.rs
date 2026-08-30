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
///
/// Carries the *generation* of the arena slot it was minted for as
/// well as the slot's index. A slot's generation changes when the node
/// in it is removed, so an identifier kept across that removal stops
/// resolving instead of silently addressing whatever occupies the slot
/// afterwards.
///
/// Without it, removal would be a correctness hazard rather than a
/// memory one: this crate forbids `unsafe`, so a stale identifier
/// could never corrupt memory, but it could return a different node
/// than the caller meant and nothing would report it.
///
/// Both halves are `u32` so the identifier stays eight bytes. Widening
/// it would double `child_ids` and `attr_ids`, which hold one entry per
/// parent-child and parent-attribute edge in the document -- millions
/// on a large one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl NodeId {
    /// The index of this node within its document's arena.
    ///
    /// Exposed because it is useful for building side tables keyed by
    /// node, which is how callers usually annotate a tree without
    /// mutating it.
    ///
    /// Two identifiers for the same slot in different generations
    /// share an index. A side table keyed on this is therefore keyed on
    /// the slot, not the node -- which is what a side table wants, but
    /// worth knowing if entries outlive a removal.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index as usize
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
    /// The current generation of each arena slot, parallel to `nodes`.
    ///
    /// Bumped when the node in a slot is removed, so every `NodeId`
    /// minted for it beforehand stops resolving. Kept beside `nodes`
    /// rather than inside `Node` because it must outlive the node it
    /// describes -- the whole point is to answer questions about a slot
    /// whose node is gone.
    pub(crate) generations: Vec<u32>,
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
            generations: Vec::new(),
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
            // One entry for the root node pushed above. `generations`
            // is indexed in lockstep with `nodes`; starting it empty
            // here shifted every lookup by one, so slot 0 read slot 1's
            // generation and `root()` stopped resolving.
            //
            // Built rather than `vec![0]`: that macro resolves through
            // the `std` prelude, which is absent on the bare-metal
            // targets this crate builds for.
            generations: {
                let mut g = Vec::with_capacity(nodes.saturating_add(1));
                g.push(0);
                g
            },
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
        NodeId {
            index: 0,
            generation: 0,
        }
    }

    pub(crate) fn push(&mut self, data: NodeData, parent: NodeId) -> NodeId {
        let id = self.mint();
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
        // The range is computed before taking the mutable borrow, so
        // that reading `child_ids` and writing the node do not overlap.
        let range = (
            u32::try_from(start).unwrap_or(u32::MAX),
            u32::try_from(self.child_ids.len() - start).unwrap_or(0),
        );
        if let Some(n) = self.resolve_mut(parent) {
            n.children = range;
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
        let id = self.mint();
        self.nodes.push(Node {
            data,
            parent: Some(parent),
            children: (0, 0),
        });
        id
    }

    /// Mint an identifier for the slot `push` is about to fill.
    ///
    /// Slots are never reused today -- nothing removes a node -- so the
    /// generation of a fresh slot is always zero. The generation table
    /// grows alongside `nodes` regardless, so that removal can bump an
    /// entry without a migration.
    ///
    /// A document with more than `u32::MAX` nodes cannot be addressed
    /// by a `NodeId`. At the smallest possible node that is over a
    /// hundred gigabytes of arena, so the panic is unreachable rather
    /// than merely unlikely -- but it is a panic and not a silent
    /// truncation, because a truncated index would address the wrong
    /// node.
    pub(crate) fn mint(&mut self) -> NodeId {
        let index = u32::try_from(self.nodes.len())
            .expect("document exceeds u32::MAX nodes");
        self.generations.push(0);
        NodeId {
            index,
            generation: 0,
        }
    }

    /// The node behind `id`, if the identifier is still live.
    ///
    /// Returns `None` for an identifier whose slot has since been
    /// removed, which is the whole reason the generation is carried.
    pub(crate) fn resolve(&self, id: NodeId) -> Option<&Node> {
        let index = id.index as usize;
        if *self.generations.get(index)? != id.generation {
            return None;
        }
        self.nodes.get(index)
    }

    pub(crate) fn resolve_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let index = id.index as usize;
        if *self.generations.get(index)? != id.generation {
            return None;
        }
        self.nodes.get_mut(index)
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
        let node = self.resolve(id)?;
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
        self.resolve_mut(id).map(|n| &mut n.data)
    }

    /// A node's parent, or `None` for the root.
    #[must_use]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.resolve(id).and_then(|n| n.parent)
    }

    /// A node's children, in document order.
    ///
    /// Returns an empty slice for a node with no children, and for an
    /// id that does not belong to this document.
    #[must_use]
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.resolve(id).map_or(&[], |n| {
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
        (0..self.nodes.len()).map(|i| NodeId {
            index: u32::try_from(i).unwrap_or(u32::MAX),
            generation: self.generations.get(i).copied().unwrap_or(0),
        })
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

#[cfg(test)]
mod generation_tests {
    use super::*;

    #[test]
    fn node_id_stays_eight_bytes() {
        // `child_ids` and `attr_ids` hold one `NodeId` per parent-child
        // and parent-attribute edge — millions on a large document.
        // Widening the identifier would double both.
        assert_eq!(core::mem::size_of::<NodeId>(), 8);
    }

    #[test]
    fn generations_stay_in_lockstep_with_nodes() {
        // They are indexed together. A length mismatch shifts every
        // lookup, which is how `root()` briefly stopped resolving.
        let doc = crate::parse("<a><b/><c>text</c></a>").expect("well-formed");
        assert_eq!(doc.generations.len(), doc.nodes.len());
    }

    #[test]
    fn a_fresh_document_resolves_every_id_it_hands_out() {
        let doc = crate::parse("<a><b/><c>text</c></a>").expect("well-formed");
        for id in doc.descendants() {
            assert!(
                doc.resolve(id).is_some(),
                "descendants() handed out an id it cannot resolve: {id:?}"
            );
        }
    }

    #[test]
    fn an_id_from_another_generation_does_not_resolve() {
        // Nothing removes a node yet, so the stale case is constructed
        // by hand. This is what a removal must produce.
        let doc = crate::parse("<a/>").expect("well-formed");
        let live = doc.root();
        let stale = NodeId {
            index: live.index,
            generation: live.generation + 1,
        };
        assert!(doc.resolve(live).is_some());
        assert!(
            doc.resolve(stale).is_none(),
            "a mismatched generation must not resolve to the live node"
        );
    }
}

/// Why a mutation could not be applied.
///
/// Every variant names a condition the caller can check for, rather
/// than a generic failure: a mutation API that returns one opaque
/// error teaches callers to `unwrap` it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeError {
    /// The node behind this identifier has been removed.
    ///
    /// Carries nothing that would let a caller retry with the same
    /// identifier, because retrying is never the right response.
    Stale,
    /// The operation needs an element and was given something else --
    /// a text node, a comment, or the document root.
    NotAnElement,
}

impl core::fmt::Display for NodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Stale => f.write_str("the node has been removed"),
            Self::NotAnElement => f.write_str("the node is not an element"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for NodeError {}

impl Document {
    /// An empty document, with a root and nothing under it.
    ///
    /// The counterpart to parsing. Until now a `Document` could only
    /// come from a parser, so building one meant writing XML to a
    /// string and parsing it back -- which is both slower and unable
    /// to express anything the serialiser would have escaped.
    #[must_use]
    pub fn empty() -> Self {
        Self::with_capacity(0)
    }

    /// Intern a name, reusing the entry if the document already has it.
    ///
    /// Names are interned because a document repeats a handful of them
    /// thousands of times. A linear scan is right here: mutation
    /// touches few names, and a map would cost every parsed document
    /// memory to speed up a case most documents never reach.
    fn intern(&mut self, namespace: Option<&str>, local: &str) -> NameId {
        // `String::from` rather than `.to_owned()`: the `ToOwned`
        // trait is not in the prelude without `std`, and this crate
        // builds for three bare-metal targets.
        let wanted = ExpandedName {
            namespace: namespace.map(String::from),
            local: String::from(local),
        };
        if let Some(i) = self.names.iter().position(|n| *n == wanted) {
            return NameId(u32::try_from(i).unwrap_or(u32::MAX));
        }
        let id = u32::try_from(self.names.len())
            .expect("document exceeds u32::MAX names");
        self.names.push(wanted);
        self.name_prefixes.push(None);
        NameId(id)
    }

    /// Store text the input does not contain, returning a handle.
    fn intern_text(&mut self, text: &str) -> Chars {
        if text.is_empty() {
            return Chars::EMPTY;
        }
        let id = u32::try_from(self.expanded.len())
            .expect("document exceeds u32::MAX texts");
        self.expanded.push(String::from(text));
        Chars::Expanded(id)
    }

    /// Append `child` to `parent`'s child list.
    ///
    /// A node's children are a contiguous `(start, len)` slice of a
    /// shared arena, written as one block when the parser closes the
    /// element. That layout is why reading is fast -- a `Vec` per node
    /// meant an allocation per element, about a million on the
    /// benchmark document -- and it is why appending cannot simply
    /// push.
    ///
    /// The block is copied to the end of the arena with the new child
    /// after it, and the old block is left behind as garbage. Reads
    /// keep their contiguous slice; the cost is arena growth
    /// proportional to how much a document is mutated, which a
    /// compaction pass can reclaim later. Moving to a linked list
    /// would make this O(1) and give up the locality the parser was
    /// built around.
    fn push_child(&mut self, parent: NodeId, child: NodeId) {
        let Some(node) = self.resolve(parent) else {
            return;
        };
        let (start, len) = (node.children.0 as usize, node.children.1 as usize);
        let new_start = self.child_ids.len();
        // `extend_from_within` copies in place without a temporary.
        self.child_ids
            .extend_from_within(start..start.saturating_add(len));
        self.child_ids.push(child);
        let range = (
            u32::try_from(new_start).unwrap_or(u32::MAX),
            u32::try_from(len + 1).unwrap_or(u32::MAX),
        );
        if let Some(n) = self.resolve_mut(parent) {
            n.children = range;
        }
    }

    /// Add an element as the last child of `parent`.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`] if `parent` has been removed.
    pub fn append_element(
        &mut self,
        parent: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> Result<NodeId, NodeError> {
        if self.resolve(parent).is_none() {
            return Err(NodeError::Stale);
        }
        let name = self.intern(namespace, local);
        let id = self.mint();
        self.nodes.push(Node {
            data: NodeData::Element {
                name,
                attributes: (0, 0),
                namespaces: (0, 0),
            },
            parent: Some(parent),
            children: (0, 0),
        });
        self.push_child(parent, id);
        Ok(id)
    }

    /// Add a text node as the last child of `parent`.
    ///
    /// The text is stored verbatim. Escaping happens at serialisation,
    /// so a caller passes the characters it means rather than the
    /// markup that spells them.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`] if `parent` has been removed.
    pub fn append_text(
        &mut self,
        parent: NodeId,
        text: &str,
    ) -> Result<NodeId, NodeError> {
        if self.resolve(parent).is_none() {
            return Err(NodeError::Stale);
        }
        let chars = self.intern_text(text);
        let id = self.mint();
        self.nodes.push(Node {
            data: NodeData::Text(chars),
            parent: Some(parent),
            children: (0, 0),
        });
        self.push_child(parent, id);
        Ok(id)
    }

    /// Remove a node and everything under it.
    ///
    /// The slots are not reused; their generations are bumped, so every
    /// identifier minted for them stops resolving. That is the whole
    /// reason [`NodeId`] carries a generation: without it a caller
    /// holding an identifier across this call would address whatever
    /// occupied the slot next, and get a wrong answer nothing reports.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`] if `id` has already been removed.
    pub fn remove(&mut self, id: NodeId) -> Result<(), NodeError> {
        if self.resolve(id).is_none() {
            return Err(NodeError::Stale);
        }

        // Unlink from the parent first, so the tree is never walkable
        // into a removed subtree even briefly.
        if let Some(parent) = self.resolve(id).and_then(|n| n.parent) {
            if let Some(p) = self.resolve(parent) {
                let (start, len) =
                    (p.children.0 as usize, p.children.1 as usize);
                let kept: Vec<NodeId> = self
                    .child_ids
                    .get(start..start.saturating_add(len))
                    .unwrap_or(&[])
                    .iter()
                    .copied()
                    .filter(|c| *c != id)
                    .collect();
                let new_start = self.child_ids.len();
                self.child_ids.extend_from_slice(&kept);
                let range = (
                    u32::try_from(new_start).unwrap_or(u32::MAX),
                    u32::try_from(kept.len()).unwrap_or(u32::MAX),
                );
                if let Some(p) = self.resolve_mut(parent) {
                    p.children = range;
                }
            }
        }

        // Then the subtree, depth first. Collected before bumping, so
        // the walk is not reading generations it is in the middle of
        // changing.
        let mut doomed = Vec::new();
        let mut stack = alloc::vec::Vec::from([id]);
        while let Some(current) = stack.pop() {
            doomed.push(current);
            stack.extend_from_slice(self.children(current));
        }
        for node in doomed {
            if let Some(g) = self.generations.get_mut(node.index as usize) {
                *g = g.wrapping_add(1);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod mutation_tests {
    use super::*;

    #[test]
    fn a_document_can_be_built_from_nothing() {
        let mut doc = Document::empty();
        let root = doc.root();
        let a = doc.append_element(root, None, "a").expect("root is live");
        let b = doc.append_element(a, None, "b").expect("a is live");
        let _ = doc.append_text(b, "hello").expect("b is live");

        assert_eq!(doc.children(root), [a]);
        assert_eq!(doc.children(a), [b]);
        assert_eq!(doc.text(root), "hello");
    }

    #[test]
    fn children_stay_contiguous_and_ordered_as_they_are_appended() {
        // The arena copies each block to the end on append. Order and
        // contents must survive that, or reads silently see a stale
        // block.
        let mut doc = Document::empty();
        let root = doc.root();
        let parent = doc.append_element(root, None, "p").expect("live");
        let kids: Vec<NodeId> = (0..8)
            .map(|i| {
                if i % 2 == 0 {
                    doc.append_element(parent, None, "e").expect("live")
                } else {
                    doc.append_text(parent, "t").expect("live")
                }
            })
            .collect();
        assert_eq!(doc.children(parent), kids.as_slice());
    }

    #[test]
    fn a_removed_node_leaves_its_identifier_stale() {
        // The reason NodeId carries a generation at all.
        let mut doc = Document::empty();
        let root = doc.root();
        let a = doc.append_element(root, None, "a").expect("live");
        assert!(doc.resolve(a).is_some());

        doc.remove(a).expect("a is live");

        assert!(doc.resolve(a).is_none(), "the identifier must not resolve");
        assert_eq!(
            doc.remove(a),
            Err(NodeError::Stale),
            "and removing twice is an error"
        );
        assert!(
            doc.children(root).is_empty(),
            "and the parent must not still list it"
        );
    }

    #[test]
    fn removing_a_subtree_invalidates_every_descendant() {
        // Removing only the top of a subtree would leave descendants
        // resolving through identifiers whose parent is gone.
        let mut doc = Document::empty();
        let root = doc.root();
        let a = doc.append_element(root, None, "a").expect("live");
        let b = doc.append_element(a, None, "b").expect("live");
        let t = doc.append_text(b, "deep").expect("live");

        doc.remove(a).expect("live");

        for (name, id) in [("a", a), ("b", b), ("text", t)] {
            assert!(doc.resolve(id).is_none(), "{name} should be stale");
        }
    }

    #[test]
    fn removing_one_child_leaves_its_siblings_alone() {
        let mut doc = Document::empty();
        let root = doc.root();
        let p = doc.append_element(root, None, "p").expect("live");
        let one = doc.append_element(p, None, "one").expect("live");
        let two = doc.append_element(p, None, "two").expect("live");
        let three = doc.append_element(p, None, "three").expect("live");

        doc.remove(two).expect("live");

        assert_eq!(doc.children(p), [one, three]);
        assert!(doc.resolve(one).is_some());
        assert!(doc.resolve(three).is_some());
    }

    #[test]
    fn appending_to_a_removed_parent_is_an_error_not_a_panic() {
        let mut doc = Document::empty();
        let root = doc.root();
        let a = doc.append_element(root, None, "a").expect("live");
        doc.remove(a).expect("live");

        assert_eq!(doc.append_element(a, None, "b"), Err(NodeError::Stale));
        assert_eq!(doc.append_text(a, "t"), Err(NodeError::Stale));
    }

    #[test]
    fn a_built_document_serialises_and_reparses_to_the_same_tree() {
        // The join between the new mutation path and the existing
        // serialiser: a document built by hand must be a fixed point
        // just as a parsed one is.
        let mut doc = Document::empty();
        let root = doc.root();
        let a = doc.append_element(root, None, "a").expect("live");
        let b = doc.append_element(a, None, "b").expect("live");
        let _ = doc.append_text(b, "p & q").expect("live");

        let once = doc.to_xml();
        let reparsed = crate::parse(&once).expect("what we built must parse");
        assert_eq!(reparsed.to_xml(), once, "built: {once}");
        assert_eq!(reparsed.text(reparsed.root()), "p & q");
    }

    #[test]
    fn text_is_stored_verbatim_and_escaped_only_on_the_way_out() {
        // A caller passes characters, not markup. If `&` were stored
        // as written and emitted as written, the reparse would read an
        // entity reference.
        let mut doc = Document::empty();
        let root = doc.root();
        let a = doc.append_element(root, None, "a").expect("live");
        let _ = doc.append_text(a, "x & y < z").expect("live");

        assert_eq!(doc.text(root), "x & y < z");
        let out = doc.to_xml();
        assert!(out.contains("&amp;"), "{out}");
        assert!(out.contains("&lt;"), "{out}");
        assert_eq!(
            crate::parse(&out).expect("reparses").text(doc.root()),
            "x & y < z"
        );
    }

    #[test]
    fn names_are_interned_rather_than_repeated() {
        let mut doc = Document::empty();
        let root = doc.root();
        let before = doc.names.len();
        for _ in 0..10 {
            let _ = doc.append_element(root, None, "same").expect("live");
        }
        assert_eq!(doc.names.len(), before + 1, "one name, ten elements");
    }
}
