// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Which XML version the output declares.

use super::escape::is_c0_control;
use crate::tree::{Document, NameId, NodeId, NodeKind};

impl Document {
    /// Whether any content can only be expressed in XML 1.1.
    ///
    /// Two things require it, and the second is not about characters
    /// at all:
    ///
    /// - a C0 control character, which 1.0 forbids outright and 1.1
    ///   permits as a reference;
    /// - **unbinding a prefix** with `xmlns:p=""`, which Namespaces 1.1
    ///   allows and Namespaces 1.0 rejects as a reserved use.
    ///
    /// The C1 range `U+007F..U+009F` does **not**: XML 1.0 permits it
    /// literally and as a reference. Sharing one predicate with
    /// [`needs_reference`] declared every 1.0 document containing
    /// `U+0085` as 1.1, which is a different document.
    ///
    /// The corpus found the unbinding case after the first was fixed,
    /// which is the argument for running a property over 1,910 real
    /// documents rather than over the cases one thinks of.
    ///
    /// A namespace URI is written as an attribute value too, whether
    /// declared or given only in a name, so a control character in one
    /// needs 1.1 as much as one in a value does.
    ///
    /// Only what is written counts. A detached subtree is still live,
    /// so walking every node declared 1.1 for a control character that
    /// never reached the output; reparsing that output found nothing
    /// needing 1.1, and the second pass dropped the declaration.
    pub(super) fn needs_xml_11(&self) -> bool {
        self.written().any(|id| match self.kind(id) {
            Some(NodeKind::Text(t)) => t.chars().any(is_c0_control),
            Some(NodeKind::Attr(a)) => {
                a.value.chars().any(is_c0_control) || self.c0_in_uri(a.name)
            }
            Some(NodeKind::Element { name, .. }) => self.c0_in_uri(name),
            Some(NodeKind::Namespace { prefix, uri }) => {
                (!prefix.is_empty() && uri.is_empty())
                    || uri.chars().any(is_c0_control)
            }
            _ => false,
        })
    }

    /// Every node the writer visits: the root's descendants, each
    /// followed by its attribute and namespace nodes.
    fn written(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.descendants_of(self.root()).flat_map(move |id| {
            core::iter::once(id)
                .chain(self.attribute_nodes(id).iter().copied())
                .chain(self.namespace_nodes(id).iter().copied())
        })
    }

    /// Whether the namespace URI of `name` holds a C0 control.
    fn c0_in_uri(&self, name: NameId) -> bool {
        self.name(name)
            .and_then(|n| n.namespace.as_deref())
            .is_some_and(|uri| uri.chars().any(is_c0_control))
    }
}
