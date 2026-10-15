// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Changing a [`Document`] after it is built.
//!
//! Every operation here goes through three gates before it touches
//! the arena: the identifiers resolve (or [`NodeError::Stale`]), the
//! node kinds fit the operation (a child goes under a container, an
//! attribute on an element), and the content is XML the serialiser
//! can write. The first mutation API checked only the first, so a
//! caller could build a document that serialised to something the
//! parser refused and find out on the round trip. See
//! `doc/adr/0008-generational-slots-and-block-relocation.md`.

use alloc::vec::Vec;

use super::blocks::BlockEdit;
use super::names::{check_comment, check_pi, check_text};
use super::{Document, NodeData, NodeError, NodeId};

/// Which of the three block arenas an edit addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arena {
    Children,
    Attributes,
    Namespaces,
}

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

    // --- gates ---------------------------------------------------------

    /// `Ok` for a live element.
    pub(super) fn element(&self, id: NodeId) -> Result<(), NodeError> {
        match self.resolve(id).map(|n| &n.data) {
            None => Err(NodeError::Stale),
            Some(NodeData::Element { .. }) => Ok(()),
            Some(_) => Err(NodeError::NotAnElement),
        }
    }

    /// `Ok` for a live node that can hold children.
    pub(super) fn container(&self, id: NodeId) -> Result<(), NodeError> {
        match self.resolve(id).map(|n| &n.data) {
            None => Err(NodeError::Stale),
            Some(NodeData::Root | NodeData::Element { .. }) => Ok(()),
            Some(_) => Err(NodeError::NotAContainer),
        }
    }

    /// `Ok` for a live node that lives in a child list.
    pub(super) fn child_kind(&self, id: NodeId) -> Result<(), NodeError> {
        match self.resolve(id).map(|n| &n.data) {
            None => Err(NodeError::Stale),
            Some(NodeData::Root) => Err(NodeError::NotRemovable),
            Some(NodeData::Attr { .. } | NodeData::Namespace { .. }) => {
                Err(NodeError::NotAChild)
            }
            Some(_) => Ok(()),
        }
    }

    /// XML has exactly one root element. Allowing a second would
    /// build a document that serialises and then fails to reparse
    /// with `TrailingContent` -- a fault the caller meets long after
    /// the call that caused it. `except` is the element being moved,
    /// which may already be the root element.
    pub(super) fn refuse_second_root(
        &self,
        parent: NodeId,
        is_element: bool,
        except: Option<NodeId>,
    ) -> Result<(), NodeError> {
        let taken = parent == self.root()
            && is_element
            && self.root_element().is_some_and(|r| Some(r) != except);
        if taken {
            Err(NodeError::RootElementExists)
        } else {
            Ok(())
        }
    }

    /// Walk up from the destination: if `id` is on that path, the
    /// move would put a node inside its own subtree.
    pub(super) fn check_no_cycle(
        &self,
        id: NodeId,
        new_parent: NodeId,
    ) -> Result<(), NodeError> {
        let mut cursor = Some(new_parent);
        while let Some(node) = cursor {
            if node == id {
                return Err(NodeError::WouldCycle);
            }
            cursor = self.parent(node);
        }
        Ok(())
    }

    // --- appending -----------------------------------------------------

    /// Add an element as the last child of `parent`.
    ///
    /// A `namespace` is the namespace *name* (the URI). No prefix is
    /// chosen here: the serialiser reuses one in scope for that
    /// namespace, or declares one on the element.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`] if `parent` has been removed,
    /// [`NodeError::NotAContainer`] if it is not the root or an element,
    /// [`NodeError::RootElementExists`] if `parent` is the root and
    /// already has one, or [`NodeError::InvalidName`] if `local` is
    /// not an `NCName`.
    pub fn append_element(
        &mut self,
        parent: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> Result<NodeId, NodeError> {
        self.container(parent)?;
        self.refuse_second_root(parent, true, None)?;
        let name = self.intern(namespace, local)?;
        let data = NodeData::Element {
            name,
            attributes: (0, 0),
            namespaces: (0, 0),
        };
        Ok(self.append_node(parent, data))
    }

    /// Add a text node as the last child of `parent`.
    ///
    /// The text is stored verbatim. Escaping happens at serialisation,
    /// so a caller passes the characters it means rather than the
    /// markup that spells them.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`] if `parent` has been removed,
    /// [`NodeError::NotAContainer`] if it cannot hold children, or
    /// [`NodeError::InvalidText`] if the text contains a character no
    /// XML version can write.
    pub fn append_text(
        &mut self,
        parent: NodeId,
        text: &str,
    ) -> Result<NodeId, NodeError> {
        self.container(parent)?;
        check_text(text)?;
        let chars = self.store_text(text);
        Ok(self.append_node(parent, NodeData::Text(chars)))
    }

    /// Add a comment as the last child of `parent`.
    ///
    /// # Errors
    ///
    /// As [`Document::append_text`]; a comment also may not contain
    /// `--` or end with `-`, and is written verbatim so it may not
    /// contain a control character or a carriage return.
    pub fn append_comment(
        &mut self,
        parent: NodeId,
        text: &str,
    ) -> Result<NodeId, NodeError> {
        self.container(parent)?;
        check_comment(text)?;
        let chars = self.store_text(text);
        Ok(self.append_node(parent, NodeData::Comment(chars)))
    }

    /// Add a processing instruction as the last child of `parent`.
    ///
    /// # Errors
    ///
    /// As [`Document::append_text`]; [`NodeError::InvalidName`] if the
    /// target is not an `NCName` or is `xml` in any case, and
    /// [`NodeError::InvalidText`] if the data contains `?>`, a control
    /// character or a carriage return.
    pub fn append_pi(
        &mut self,
        parent: NodeId,
        target: &str,
        data: &str,
    ) -> Result<NodeId, NodeError> {
        self.container(parent)?;
        check_pi(target, data)?;
        let target = self.store_text(target);
        let data = self.store_text(data);
        Ok(self.append_node(
            parent,
            NodeData::ProcessingInstruction { target, data },
        ))
    }

    /// Replace the content of a text node.
    ///
    /// Overwrites the stored text in place, so repeated replacement
    /// retains only the latest value.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`], [`NodeError::NotText`] for any other kind
    /// of node, or [`NodeError::InvalidText`].
    pub fn set_text(
        &mut self,
        id: NodeId,
        text: &str,
    ) -> Result<(), NodeError> {
        let old = match self.resolve(id).map(|n| &n.data) {
            None => return Err(NodeError::Stale),
            Some(NodeData::Text(c)) => *c,
            Some(_) => return Err(NodeError::NotText),
        };
        check_text(text)?;
        let chars = self.replace_text(old, text);
        if let Some(n) = self.resolve_mut(id) {
            n.data = NodeData::Text(chars);
        }
        Ok(())
    }

    // --- removing and moving -------------------------------------------

    /// Free one arena slot, once.
    ///
    /// Bumping an even generation to odd is what makes every
    /// identifier minted for the slot stop resolving. A slot already
    /// odd is left alone: freeing it again would make it even -- live
    /// -- and list it twice, so the next two allocations would share
    /// it. That is how `remove(attr)` followed by `remove(element)`
    /// once made a node vanish.
    pub(super) fn free_slot(&mut self, id: NodeId) {
        let index = id.index as usize;
        let Some(g) = self.generations.get_mut(index) else {
            return;
        };
        if *g % 2 == 1 {
            return;
        }
        *g = g.wrapping_add(1);
        self.free_slots.push(id.index);
        self.order_dirty = true;
        self.release_node_text(index);
    }

    /// Free `id` and everything it owns: children, attributes and
    /// namespace nodes, to any depth.
    pub(super) fn free_subtree(&mut self, id: NodeId) {
        let mut stack = Vec::from([id]);
        while let Some(current) = stack.pop() {
            // Attribute and namespace nodes have no children, so they
            // can ride the same stack as the subtree.
            stack.extend_from_slice(self.children(current));
            stack.extend_from_slice(self.attribute_nodes(current));
            stack.extend_from_slice(self.namespace_nodes(current));
            self.free_slot(current);
        }
    }

    /// Remove a node and everything under it.
    ///
    /// An attribute or namespace node is taken off its element; any
    /// other node is unlinked from its parent and freed with its
    /// subtree. The slots are recycled for future allocations once
    /// their generations are bumped, so every identifier minted for
    /// them beforehand stops resolving. That is the whole reason
    /// [`NodeId`] carries a generation: without it a caller holding an
    /// identifier across this call would address whatever occupied the
    /// slot next, and get a wrong answer nothing reports.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`] if `id` has already been removed, or
    /// [`NodeError::NotRemovable`] for the document root.
    pub fn remove(&mut self, id: NodeId) -> Result<(), NodeError> {
        let owned_by = match self.resolve(id).map(|n| &n.data) {
            None => return Err(NodeError::Stale),
            Some(NodeData::Root) => return Err(NodeError::NotRemovable),
            Some(NodeData::Attr { .. }) => Some(Arena::Attributes),
            Some(NodeData::Namespace { .. }) => Some(Arena::Namespaces),
            Some(_) => None,
        };
        // Unlink first, so the tree is never walkable into a removed
        // subtree even briefly.
        match (owned_by, self.parent(id)) {
            (Some(arena), Some(owner)) => {
                let _ = self.edit_block(owner, arena, BlockEdit::Without(id));
            }
            (None, _) => self.unlink_from_parent(id),
            (Some(_), None) => {}
        }
        self.free_subtree(id);
        Ok(())
    }

    /// Unlink a node from its parent without freeing it.
    ///
    /// The subtree stays live and can be put back with
    /// [`Document::reparent`] or [`Document::insert_at`]. Until then it
    /// is not written and has no document position.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`], [`NodeError::NotRemovable`] for the root,
    /// or [`NodeError::NotAChild`] for an attribute or namespace node.
    pub fn detach(&mut self, id: NodeId) -> Result<(), NodeError> {
        self.child_kind(id)?;
        self.unlink_from_parent(id);
        if let Some(n) = self.resolve_mut(id) {
            n.parent = None;
        }
        self.order_dirty = true;
        Ok(())
    }

    /// Move a subtree to a new parent, appending it there.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`] if either identifier has been removed,
    /// [`NodeError::NotAContainer`] if `new_parent` cannot hold
    /// children, [`NodeError::NotAChild`] for an attribute or namespace
    /// node, [`NodeError::NotRemovable`] for the root,
    /// [`NodeError::RootElementExists`] if the move would put a second
    /// element under the root, or [`NodeError::WouldCycle`] if `id` is
    /// `new_parent` or an ancestor of it. That last check is not
    /// optional: the result would be a "tree" containing a loop, and
    /// every walker in this crate -- `descendants_of`, `text`, the
    /// serialiser, `XPath` -- follows children until they run out. None
    /// of them would ever run out.
    pub fn reparent(
        &mut self,
        id: NodeId,
        new_parent: NodeId,
    ) -> Result<(), NodeError> {
        self.move_node(id, new_parent, None)
    }

    /// Put `child` at position `index` among `parent`'s children.
    ///
    /// `child` is unlinked from wherever it was first, so the index
    /// counts the children that remain; an index past the end appends.
    ///
    /// # Errors
    ///
    /// As [`Document::reparent`].
    pub fn insert_at(
        &mut self,
        parent: NodeId,
        index: usize,
        child: NodeId,
    ) -> Result<(), NodeError> {
        self.move_node(child, parent, Some(index))
    }

    pub(super) fn move_node(
        &mut self,
        id: NodeId,
        new_parent: NodeId,
        at: Option<usize>,
    ) -> Result<(), NodeError> {
        self.child_kind(id)?;
        self.container(new_parent)?;
        self.check_no_cycle(id, new_parent)?;
        self.refuse_second_root(new_parent, self.is_element(id), Some(id))?;
        self.unlink_from_parent(id);
        if let Some(n) = self.resolve_mut(id) {
            n.parent = Some(new_parent);
        }
        let edit =
            at.map_or(BlockEdit::Append(id), |i| BlockEdit::InsertAt(i, id));
        let _ = self.edit_block(new_parent, Arena::Children, edit);
        Ok(())
    }
}
