// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Attributes and namespace declarations on an element.
//!
//! Both live in blocks like children do, and both are matched by
//! expanded name: two prefixes bound to one namespace intern as two
//! ids for one name, so ids are never compared here.

use super::blocks::BlockEdit;
use super::mutate::Arena;
use super::names::{check_prefix, check_text, namespace_of};
use super::{Document, NodeData, NodeError, NodeId};

impl Document {
    /// Whether attribute node `attr` has this expanded name.
    fn attribute_named(
        &self,
        attr: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> bool {
        match self.resolve(attr).map(|n| &n.data) {
            Some(NodeData::Attr { name, .. }) => {
                self.name(*name).is_some_and(|n| {
                    n.local == local && n.namespace.as_deref() == namespace
                })
            }
            _ => false,
        }
    }

    /// The attribute node of `element` with this expanded name.
    ///
    /// Compared by namespace and local part, never by interned id: two
    /// prefixes bound to one namespace intern as two ids for one name.
    pub(crate) fn find_attribute(
        &self,
        element: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> Option<NodeId> {
        let namespace = namespace_of(namespace);
        self.attribute_nodes(element)
            .iter()
            .copied()
            .find(|a| self.attribute_named(*a, namespace, local))
    }

    /// Set an attribute on an element, replacing any it already has
    /// with that expanded name.
    ///
    /// Replacement overwrites the stored value in place; a new
    /// attribute is appended to the element's attribute block.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`] if `element` has been removed,
    /// [`NodeError::NotAnElement`] if it is not an element -- only
    /// elements carry attributes -- [`NodeError::InvalidName`] or
    /// [`NodeError::InvalidText`].
    pub fn set_attribute(
        &mut self,
        element: NodeId,
        namespace: Option<&str>,
        local: &str,
        value: &str,
    ) -> Result<(), NodeError> {
        self.element(element)?;
        check_text(value)?;
        if let Some(attr) = self.find_attribute(element, namespace, local) {
            self.replace_attribute_value(attr, value);
            return Ok(());
        }
        let name = self.intern(namespace, local)?;
        let value = self.store_text(value);
        let attr =
            self.alloc_node(NodeData::Attr { name, value }, Some(element));
        let _ = self.edit_block(
            element,
            Arena::Attributes,
            BlockEdit::Append(attr),
        );
        Ok(())
    }

    fn replace_attribute_value(&mut self, attr: NodeId, value: &str) {
        let Some(NodeData::Attr { name, value: old }) =
            self.resolve(attr).map(|n| n.data.clone())
        else {
            return;
        };
        let value = self.replace_text(old, value);
        if let Some(n) = self.resolve_mut(attr) {
            n.data = NodeData::Attr { name, value };
        }
    }

    /// Remove an attribute by expanded name, reporting whether there
    /// was one.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`] if `element` has been removed, or
    /// [`NodeError::NotAnElement`] if it is not an element.
    pub fn remove_attribute(
        &mut self,
        element: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> Result<bool, NodeError> {
        self.element(element)?;
        match self.find_attribute(element, namespace, local) {
            Some(attr) => self.remove(attr).map(|()| true),
            None => Ok(false),
        }
    }

    // --- namespaces ----------------------------------------------------

    /// The namespace node `element` declares for `prefix`, if any.
    fn find_namespace_node(
        &self,
        element: NodeId,
        prefix: &str,
    ) -> Option<NodeId> {
        self.namespace_nodes(element).iter().copied().find(|ns| {
            matches!(
                self.resolve(*ns).map(|n| &n.data),
                Some(NodeData::Namespace { prefix: p, .. }) if self.chars(*p) == prefix
            )
        })
    }

    /// Declare a namespace on an element, returning the namespace
    /// node.
    ///
    /// `None` declares the default namespace (`xmlns="..."`). A prefix
    /// the element already declares is rebound rather than declared
    /// twice. An empty `uri` with a prefix undeclares it, which only
    /// XML 1.1 can write; the serialiser declares the version when
    /// that is so.
    ///
    /// Declaring is optional: a name in a namespace nothing declares
    /// is given a declaration when the document is written. This is
    /// for choosing the prefix and where it is declared.
    ///
    /// # Errors
    ///
    /// [`NodeError::Stale`], [`NodeError::NotAnElement`],
    /// [`NodeError::InvalidName`] if the prefix is not an `NCName` or
    /// is `xml` or `xmlns`, or [`NodeError::InvalidText`].
    pub fn declare_namespace(
        &mut self,
        element: NodeId,
        prefix: Option<&str>,
        uri: &str,
    ) -> Result<NodeId, NodeError> {
        self.element(element)?;
        let prefix = prefix.unwrap_or("");
        check_prefix(prefix)?;
        check_text(uri)?;
        if let Some(existing) = self.find_namespace_node(element, prefix) {
            self.rebind_namespace(existing, uri);
            return Ok(existing);
        }
        let prefix = self.store_text(prefix);
        let uri = self.store_text(uri);
        let id =
            self.alloc_node(NodeData::Namespace { prefix, uri }, Some(element));
        let _ =
            self.edit_block(element, Arena::Namespaces, BlockEdit::Append(id));
        Ok(id)
    }

    fn rebind_namespace(&mut self, ns: NodeId, uri: &str) {
        let Some(NodeData::Namespace { prefix, uri: old }) =
            self.resolve(ns).map(|n| n.data.clone())
        else {
            return;
        };
        let uri = self.replace_text(old, uri);
        if let Some(n) = self.resolve_mut(ns) {
            n.data = NodeData::Namespace { prefix, uri };
        }
    }
}
