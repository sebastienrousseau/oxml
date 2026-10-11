// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Rebuilding the block arenas without garbage.

use alloc::vec::Vec;

use super::blocks::pack;
use super::mutate::Arena;
use super::{Document, NodeId};

impl Document {
    /// Rebuild the block arenas with no garbage, in document order.
    ///
    /// A block that has moved leaves its old entries behind, and a
    /// block with spare capacity holds entries it does not use. Both
    /// are bounded -- a constant per append, amortised -- but a
    /// document that is mutated heavily and then kept, or cloned, is
    /// better off without them. Identifiers are unchanged; detached
    /// subtrees are kept; the document-order cache is refreshed.
    pub fn compact(&mut self) {
        let mut fresh = Arenas::default();
        let tops: Vec<NodeId> =
            self.nodes().filter(|n| self.parent(*n).is_none()).collect();
        for top in tops {
            self.rebuild_subtree(top, &mut fresh);
        }
        self.child_ids = fresh.children;
        self.attr_ids = fresh.attributes;
        self.ns_ids = fresh.namespaces;
        self.child_caps.clear();
        self.attr_caps.clear();
        self.ns_caps.clear();
        self.scratch = Vec::new();
        self.refresh_order();
    }

    /// Copy every block under `top` into `out`, pre-order, and point
    /// the nodes at the copies.
    fn rebuild_subtree(&mut self, top: NodeId, out: &mut Arenas) {
        let mut stack = Vec::from([top]);
        while let Some(id) = stack.pop() {
            let start = out.children.len();
            out.children.extend_from_slice(self.children(id));
            let children = pack(start, out.children.len() - start);
            let attributes =
                copy_block(&mut out.attributes, self.attribute_nodes(id));
            let namespaces =
                copy_block(&mut out.namespaces, self.namespace_nodes(id));
            self.set_block_range(id, Arena::Children, children);
            self.set_block_range(id, Arena::Attributes, attributes);
            self.set_block_range(id, Arena::Namespaces, namespaces);
            stack.extend(out.children[start..].iter().rev());
        }
    }
}

/// The three block arenas being rebuilt by [`Document::compact`].
#[derive(Default)]
struct Arenas {
    children: Vec<NodeId>,
    attributes: Vec<NodeId>,
    namespaces: Vec<NodeId>,
}

fn copy_block(into: &mut Vec<NodeId>, block: &[NodeId]) -> (u32, u32) {
    let start = into.len();
    into.extend_from_slice(block);
    pack(start, block.len())
}
