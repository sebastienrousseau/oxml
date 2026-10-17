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
//!
//! A document can be changed after it is built: see the methods under
//! [`Document::append_element`], and [`Builder`] for the chainable
//! form. The design -- generational slots, blocks that relocate with
//! spare capacity -- is recorded in
//! `doc/adr/0008-generational-slots-and-block-relocation.md`.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

mod attributes;
mod blocks;
mod builder;
mod compact;
mod error;
mod mutate;
#[cfg(test)]
mod mutation_tests;
mod names;

pub use builder::Builder;
pub use error::NodeError;

use blocks::Caps;

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
    /// An identifier that resolves to nothing, for the spare entries
    /// of a block.
    ///
    /// No slot has index `u32::MAX`, and the odd generation would be
    /// refused even if one did.
    pub(crate) const UNUSED: Self = Self {
        index: u32::MAX,
        generation: 1,
    };

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
    /// block when the element closes. A block that mutation has to
    /// grow is moved to the end of the arena with spare capacity; see
    /// `tree/blocks.rs`.
    pub(crate) children: (u32, u32),
}

/// The span of a node with no source: created by the mutation API, or
/// a recycled slot.
pub(crate) const NO_SPAN: (u32, u32) = (u32::MAX, u32::MAX);

/// A parsed XML document.
///
/// Construct one with [`crate::parse`], or build one from
/// [`Document::empty`].
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
    /// in the source as written. Empty for most documents. Everything
    /// the mutation API stores goes here too.
    pub(crate) expanded: Vec<String>,
    /// Entries of `expanded` released by a removal, for reuse.
    pub(crate) free_expanded: Vec<u32>,
    pub(crate) nodes: Vec<Node>,
    /// The current generation of each arena slot, parallel to `nodes`.
    ///
    /// **Even means live, odd means removed.** Removal bumps by one,
    /// so a slot's parity says whether anything occupies it without a
    /// second table. `resolve` refuses an odd slot even when the
    /// generation matches, which is what stops `nodes` handing out
    /// identifiers to removed nodes -- it mints them with the slot's
    /// *current* generation, so without the parity rule a removed
    /// node would look perfectly live.
    ///
    /// Bumped when the node in a slot is removed, so every `NodeId`
    /// minted for it beforehand stops resolving. Kept beside `nodes`
    /// rather than inside `Node` because it must outlive the node it
    /// describes -- the whole point is to answer questions about a slot
    /// whose node is gone.
    pub(crate) generations: Vec<u32>,
    /// Where each node came from in `input`: `(start, end)` byte
    /// offsets of the construct that produced it, indexed in lockstep
    /// with `nodes`. [`NO_SPAN`] for a node the mutation API created
    /// or a slot that was recycled, since neither has a source.
    pub(crate) spans: Vec<(u32, u32)>,
    /// Recycled arena slot indices from removed nodes and attributes.
    ///
    /// Each removed slot appears exactly once: `free_slot` refuses a
    /// slot that is already odd.
    pub(crate) free_slots: Vec<u32>,
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
    /// to reconstruct it from. A name created by the mutation API has
    /// no prefix; the serialiser chooses one.
    pub(crate) name_prefixes: Vec<Option<String>>,
    /// All attribute lists, concatenated.
    pub(crate) attr_ids: Vec<NodeId>,
    /// All child lists, concatenated. Each node's slice is described by
    /// its `children` range.
    pub(crate) child_ids: Vec<NodeId>,
    /// All namespace-node lists, concatenated.
    pub(crate) ns_ids: Vec<NodeId>,
    /// Spare capacity of child blocks that mutation has moved.
    pub(crate) child_caps: Caps,
    /// Spare capacity of attribute blocks that mutation has moved.
    pub(crate) attr_caps: Caps,
    /// Spare capacity of namespace blocks that mutation has moved.
    pub(crate) ns_caps: Caps,
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
    /// The pre-order position of each slot, parallel to `nodes`, or
    /// `u32::MAX` for a slot not reachable from the root.
    ///
    /// Empty until asked for. A document that has never been mutated
    /// is in arena order already, so it never pays for this.
    pub(crate) order: Vec<u32>,
    /// Whether a mutation has happened since `order` was computed.
    pub(crate) order_dirty: bool,
}

impl Document {
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
            free_expanded: Vec::new(),
            generations: Vec::new(),
            spans: Vec::new(),
            nodes: Vec::new(),
            free_slots: Vec::new(),
            names: Vec::new(),
            name_prefixes: Vec::new(),
            attr_ids: Vec::new(),
            child_ids: Vec::new(),
            ns_ids: Vec::new(),
            child_caps: Caps::new(),
            attr_caps: Caps::new(),
            ns_caps: Caps::new(),
            scratch: Vec::new(),
            ids: BTreeMap::new(),
            order: Vec::new(),
            order_dirty: false,
        }
    }

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
            data: NodeData::Root,
            parent: None,
            children: (0, 0),
        });
        // One entry for the root node pushed above. `generations` is
        // indexed in lockstep with `nodes`; starting it empty here
        // shifted every lookup by one, so slot 0 read slot 1's
        // generation and `root()` stopped resolving.
        //
        // Built rather than `vec![0]`: that macro resolves through the
        // `std` prelude, which is absent on the bare-metal targets
        // this crate builds for.
        let mut generations = Vec::with_capacity(nodes.saturating_add(1));
        generations.push(0);
        let mut spans = Vec::with_capacity(nodes.saturating_add(1));
        spans.push(NO_SPAN);
        Self {
            generations,
            spans,
            nodes: v,
            child_ids: Vec::with_capacity(nodes),
            ..Self::placeholder()
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

    /// Where `id` came from in the parsed input, as a byte range.
    ///
    /// An element's range runs from the `<` of its start tag to the
    /// `>` of its end tag (or of the self-closing tag); a text node's
    /// covers the character data as written, references included; a
    /// comment's and a processing instruction's cover the whole
    /// construct; the root's is the whole input. Offsets index the
    /// text the parser worked on, which is the caller's input with
    /// line endings normalised to `\n`, so they match
    /// [`Error::offset`](crate::Error) and
    /// [`Error::line_column`](crate::Error::line_column).
    ///
    /// `None` for a node the mutation API created, for an attribute or
    /// namespace node, and for a stale identifier. Tooling that maps
    /// nodes back to source text (an editor, a linter) is what this is
    /// for; nothing else in the crate reads it.
    #[must_use]
    pub fn range(&self, id: NodeId) -> Option<core::ops::Range<usize>> {
        let _live = self.resolve(id)?;
        let (start, end) = *self.spans.get(id.index as usize)?;
        if (start, end) == NO_SPAN {
            return None;
        }
        Some(start as usize..end as usize)
    }

    /// Record where `id` came from. Offsets past `u32::MAX` are not
    /// recorded, so a node that far in has no range rather than a
    /// wrong one.
    pub(crate) fn set_span(&mut self, id: NodeId, start: usize, end: usize) {
        let (Ok(s), Ok(e)) = (u32::try_from(start), u32::try_from(end)) else {
            return;
        };
        if let Some(slot) = self.spans.get_mut(id.index as usize) {
            *slot = (s, e);
        }
    }

    /// Extend `id`'s recorded range to end at `end`.
    pub(crate) fn set_span_end(&mut self, id: NodeId, end: usize) {
        let Ok(e) = u32::try_from(end) else { return };
        if let Some(slot) = self.spans.get_mut(id.index as usize)
            && *slot != NO_SPAN
        {
            slot.1 = e;
        }
    }

    /// Mint an identifier for the slot `push` is about to fill.
    ///
    /// A fresh slot starts at generation zero. Removal bumps the
    /// entry, so the table grows alongside `nodes` from the start
    /// rather than being migrated in when the first node goes.
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
        self.spans.push(NO_SPAN);
        NodeId {
            index,
            generation: 0,
        }
    }

    /// Allocate a node in the arena, reusing a freed slot if there is
    /// one.
    ///
    /// A reused slot's generation is bumped from odd back to even,
    /// which is what makes the new identifier differ from every one
    /// minted for the slot's previous occupant.
    pub(crate) fn alloc_node(
        &mut self,
        data: NodeData,
        parent: Option<NodeId>,
    ) -> NodeId {
        self.order_dirty = true;
        let Some(index) = self.free_slots.pop() else {
            let id = self.mint();
            self.nodes.push(Node {
                data,
                parent,
                children: (0, 0),
            });
            return id;
        };
        let slot = index as usize;
        self.spans[slot] = NO_SPAN;
        let slot_gen = &mut self.generations[slot];
        *slot_gen = slot_gen.wrapping_add(1);
        debug_assert_eq!(*slot_gen % 2, 0, "live slots have even generations");
        self.nodes[slot] = Node {
            data,
            parent,
            children: (0, 0),
        };
        NodeId {
            index,
            generation: *slot_gen,
        }
    }

    /// The node behind `id`, if the identifier is still live.
    ///
    /// Returns `None` for an identifier whose slot has since been
    /// removed, which is the whole reason the generation is carried.
    pub(crate) fn resolve(&self, id: NodeId) -> Option<&Node> {
        let index = id.index as usize;
        let generation = *self.generations.get(index)?;
        // Odd means the slot was removed. Checked before the match,
        // because an identifier minted from the current generation of
        // a removed slot would otherwise match itself.
        if generation % 2 == 1 || generation != id.generation {
            return None;
        }
        self.nodes.get(index)
    }

    pub(crate) fn resolve_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let index = id.index as usize;
        let generation = *self.generations.get(index)?;
        if generation % 2 == 1 || generation != id.generation {
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

    /// A node's parent, or `None` for the root and for a detached
    /// subtree's top.
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
    /// as it appeared in the source. A name created by the mutation
    /// API has none; the serialiser chooses a prefix for it.
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
    /// local part is what callers almost always mean. Use
    /// [`Document::attribute_ns`] when the namespace matters.
    #[must_use]
    pub fn attribute(&self, id: NodeId, local: &str) -> Option<&str> {
        self.attributes(id)
            .into_iter()
            .find(|a| self.name(a.name).is_some_and(|n| n.local == local))
            .map(|a| a.value)
    }

    /// Look up an attribute by expanded name.
    ///
    /// `None` as the namespace means no namespace, which is what an
    /// unprefixed attribute is in. Prefixes play no part: `p:a` and
    /// `q:a` with both prefixes bound to one namespace are one name.
    #[must_use]
    pub fn attribute_ns(
        &self,
        id: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> Option<&str> {
        let attr = self.find_attribute(id, namespace, local)?;
        match self.kind(attr) {
            Some(NodeKind::Attr(a)) => Some(a.value),
            _ => None,
        }
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

    /// The text of a node, borrowed, when it is a single run.
    ///
    /// For a text node, its content. For the root or an element,
    /// the content of its one text child, or `""` when it has no
    /// children at all. `None` otherwise: an element with element
    /// children, or mixed content, has no single text to borrow, and
    /// [`Document::text`] gathers it instead.
    #[must_use]
    pub fn text_content(&self, id: NodeId) -> Option<&str> {
        match self.kind(id)? {
            NodeKind::Text(t) => Some(t),
            NodeKind::Root | NodeKind::Element { .. } => {
                self.single_text_child(id)
            }
            _ => None,
        }
    }

    fn single_text_child(&self, id: NodeId) -> Option<&str> {
        match self.children(id) {
            [] => Some(""),
            [only] => match self.kind(*only) {
                Some(NodeKind::Text(t)) => Some(t),
                _ => None,
            },
            _ => None,
        }
    }

    /// Every live node in the document, in arena order.
    ///
    /// Arena order is the order slots were filled. For a document
    /// that has only been parsed it is document order, because the
    /// parser appends in pre-order; after a move or a slot reuse it
    /// is not. Attribute and namespace nodes are included. For the
    /// tree in document order use [`Document::descendants_of`]; for a
    /// position to sort by, [`Document::order_key`].
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        // Removed slots are skipped. Before this, a removed node was
        // still yielded -- it minted cleanly from its own current
        // generation -- so a walk after a removal visited nodes that
        // were gone, and their child lists named identifiers that no
        // longer resolved. Found by the mutation fuzz target.
        (0..self.nodes.len())
            .filter(|i| self.generations.get(*i).is_none_or(|g| g % 2 == 0))
            .map(|i| NodeId {
                index: u32::try_from(i).unwrap_or(u32::MAX),
                generation: self.generations.get(i).copied().unwrap_or(0),
            })
    }

    /// Every live node in the document, in arena order.
    ///
    /// The same walk as [`Document::nodes`], kept under the name it
    /// has had since 0.0.1. It is **not** a pre-order walk of the
    /// tree: a mutated document can yield a node before its parent.
    pub fn descendants(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes()
    }

    /// The nodes under `id`, in document order, not including `id`.
    ///
    /// A pre-order walk of the child lists: every element before its
    /// children, siblings in order. Attribute and namespace nodes are
    /// not children and are not yielded; reach them from their
    /// element with [`Document::attribute_nodes`] and
    /// [`Document::namespace_nodes`].
    #[must_use]
    pub fn descendants_of(&self, id: NodeId) -> Descendants<'_> {
        let mut stack = Vec::new();
        stack.extend(self.children(id).iter().rev());
        Descendants { doc: self, stack }
    }

    /// The pre-order position of every slot, recomputed if a mutation
    /// has happened since it was last asked for.
    ///
    /// Indexed by [`NodeId::index`]. The root is `0`; an element is
    /// followed by its namespace nodes, then its attributes, then its
    /// children, which is `XPath`'s document order. A slot that is
    /// removed or not reachable from the root holds `u32::MAX`.
    ///
    /// Computed in one walk and cached, so a node-set sort pays for
    /// it once per batch of mutations rather than once per
    /// comparison.
    pub fn document_order(&mut self) -> &[u32] {
        if self.order_dirty || self.order.len() != self.nodes.len() {
            self.refresh_order();
        }
        &self.order
    }

    /// The pre-order position of one node, from the cached order.
    ///
    /// `None` for an identifier that does not resolve or a node not
    /// reachable from the root. For a document that has never been
    /// mutated this is the arena index. After a mutation the cache is
    /// stale until [`Document::document_order`] refreshes it; check
    /// [`Document::is_order_dirty`].
    #[must_use]
    pub fn order_key(&self, id: NodeId) -> Option<u32> {
        let _ = self.resolve(id)?;
        if self.order.is_empty() && !self.order_dirty {
            return Some(id.index);
        }
        self.order
            .get(id.index())
            .copied()
            .filter(|key| *key != u32::MAX)
    }

    /// Whether a mutation has happened since the document order was
    /// last computed.
    #[must_use]
    pub const fn is_order_dirty(&self) -> bool {
        self.order_dirty
    }

    pub(crate) fn refresh_order(&mut self) {
        let mut order = Vec::new();
        order.resize(self.nodes.len(), u32::MAX);
        let mut next = 0u32;
        let mut stack = Vec::from([self.root()]);
        while let Some(id) = stack.pop() {
            place(&mut order, id, &mut next);
            for &ns in self.namespace_nodes(id) {
                place(&mut order, ns, &mut next);
            }
            for &attr in self.attribute_nodes(id) {
                place(&mut order, attr, &mut next);
            }
            stack.extend(self.children(id).iter().rev());
        }
        self.order = order;
        self.order_dirty = false;
    }

    /// The number of live nodes, including the document root.
    ///
    /// Removed nodes are not counted, though their slots remain in
    /// the arena until reused: a [`NodeId::index`] can therefore be
    /// greater than this.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len().saturating_sub(self.free_slots.len())
    }

    /// Whether the document holds nothing but its root node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() <= 1
    }
}

/// Give `id` the next pre-order position.
fn place(order: &mut [u32], id: NodeId, next: &mut u32) {
    if let Some(slot) = order.get_mut(id.index()) {
        *slot = *next;
        *next = next.saturating_add(1);
    }
}

/// A pre-order walk of a subtree, from [`Document::descendants_of`].
#[derive(Debug, Clone)]
pub struct Descendants<'a> {
    doc: &'a Document,
    /// Nodes still to visit, the next one last.
    stack: Vec<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.stack.pop()?;
        self.stack.extend(self.doc.children(id).iter().rev());
        Some(id)
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
        for id in doc.nodes() {
            assert!(
                doc.resolve(id).is_some(),
                "nodes() handed out an id it cannot resolve: {id:?}"
            );
        }
    }

    #[test]
    fn an_id_from_another_generation_does_not_resolve() {
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

    #[test]
    fn descendants_of_is_pre_order() {
        let doc = crate::parse("<a><b><c/></b><d/></a>").expect("well-formed");
        let names: Vec<&str> = doc
            .descendants_of(doc.root())
            .filter_map(|n| doc.element_name(n).map(|e| e.local.as_str()))
            .collect();
        assert_eq!(names, ["a", "b", "c", "d"]);
        let a = doc.root_element().expect("root element");
        assert_eq!(doc.descendants_of(a).count(), 3, "not including a");
    }

    #[test]
    fn document_order_puts_namespaces_and_attributes_before_children() {
        let mut doc =
            crate::parse("<a xmlns:p=\"u\" k=\"v\"><b/></a>").expect("valid");
        let order = doc.document_order().to_vec();
        let a = doc.root_element().expect("root element");
        let ns = doc.namespace_nodes(a)[0];
        let attr = doc.attribute_nodes(a)[0];
        let b = doc.children(a)[0];
        assert_eq!(order[doc.root().index()], 0);
        assert!(order[a.index()] < order[ns.index()]);
        assert!(order[ns.index()] < order[attr.index()]);
        assert!(order[attr.index()] < order[b.index()]);
        assert_eq!(doc.order_key(b), Some(order[b.index()]));
    }
}
