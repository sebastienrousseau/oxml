// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Editing the `(start, len)` blocks of a shared arena.
//!
//! Child lists, attribute lists and namespace lists each live in one
//! flat `Vec<NodeId>`, and a node holds a `(start, len)` range into
//! it. The parser writes every block exactly once, so the layout costs
//! nothing to read and nothing to build. Mutation is where it bites: a
//! block cannot grow in place when the next block starts right after
//! it.
//!
//! The first mutation API answered that by copying the block to the
//! end of the arena on every append and leaving the old one behind.
//! Appending `n` children therefore retained `n * n / 2` entries --
//! 10,000 appends held 537 MB -- and `Document::clone` copied the
//! garbage.
//!
//! This module is the one place a block changes, and it moves a block
//! only when it has to. A moved block is given spare capacity, twice
//! its length, so the appends after it are in place until that runs
//! out: amortised, each append costs a constant. Removal shifts the
//! tail down in place and never moves anything.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::mutate::Arena;
use super::{Document, NodeData, NodeId};

/// One change to a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockEdit {
    /// Add `NodeId` after the last entry.
    Append(NodeId),
    /// Drop this entry, if it is there.
    Without(NodeId),
    /// Put `NodeId` at this position; past the end means append.
    InsertAt(usize, NodeId),
}

/// The spare capacity of blocks that were moved by an edit.
///
/// Keyed by the block's start. A block the parser wrote has no entry
/// and therefore no slack, so a parsed document pays nothing for this
/// until it is mutated. An entry outlives nothing: it is dropped when
/// its block moves again or empties.
#[derive(Debug, Clone, Default)]
pub(crate) struct Caps(BTreeMap<u32, u32>);

impl Caps {
    pub(crate) const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// How many entries the block at `start` may hold without moving.
    ///
    /// Never less than its current length, whatever the table says,
    /// and always zero for an empty block: its `start` is meaningless
    /// and may be inside another block, so it must never be written
    /// into in place.
    fn of(&self, start: usize, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let start = u32::try_from(start).unwrap_or(u32::MAX);
        self.0.get(&start).map_or(len, |c| (*c as usize).max(len))
    }

    fn moved(&mut self, from: usize, to: usize, cap: usize) {
        let _ = self.0.remove(&u32::try_from(from).unwrap_or(u32::MAX));
        let _ = self.0.insert(
            u32::try_from(to).unwrap_or(u32::MAX),
            u32::try_from(cap).unwrap_or(u32::MAX),
        );
    }

    /// Record that the block at `start` has room for `len` entries.
    ///
    /// Called when a block shrinks in place, so that the entries it
    /// gave up are reused by the next append rather than forcing a
    /// move. A larger figure already on record is kept.
    fn remember(&mut self, start: usize, len: usize) {
        let start = u32::try_from(start).unwrap_or(u32::MAX);
        let len = u32::try_from(len).unwrap_or(u32::MAX);
        let entry = self.0.entry(start).or_insert(len);
        *entry = (*entry).max(len);
    }

    /// The block at `start` has lost one of its `len` entries.
    ///
    /// A block that emptied has no capacity to keep: its start is
    /// meaningless now, and may be reused by another block.
    fn shrunk(&mut self, start: usize, len: usize) {
        if len == 1 {
            let _ = self.0.remove(&u32::try_from(start).unwrap_or(u32::MAX));
        } else {
            self.remember(start, len);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

/// Pack a range for storage on a node.
pub(crate) fn pack(start: usize, len: usize) -> (u32, u32) {
    (
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(len).unwrap_or(u32::MAX),
    )
}

/// Apply `edit` to the block `range` of `arena`, returning the block's
/// new range.
///
/// The block is edited in place when it can be and moved to the end
/// of the arena, with spare capacity recorded in `caps`, when it
/// cannot. A range that does not fit the arena is treated as empty
/// rather than indexed: this crate's contract is that no input panics
/// it, and a bad range is a bug to report with a wrong answer, not a
/// crash.
pub(crate) fn relocate_block(
    arena: &mut Vec<NodeId>,
    caps: &mut Caps,
    range: (u32, u32),
    edit: BlockEdit,
) -> (u32, u32) {
    let (start, len) = (range.0 as usize, range.1 as usize);
    let (start, len) = if start.saturating_add(len) > arena.len() {
        (0, 0)
    } else {
        (start, len)
    };
    match edit {
        BlockEdit::Append(id) => insert_into(arena, caps, start, len, len, id),
        BlockEdit::InsertAt(at, id) => {
            insert_into(arena, caps, start, len, at.min(len), id)
        }
        BlockEdit::Without(id) => drop_from(arena, caps, start, len, id),
    }
}

/// Put `id` at offset `at` of the block, growing it by one.
///
/// In place when the block has spare capacity; otherwise the block is
/// copied to the end of the arena with room for twice its length, so
/// that the next appends need no copy. An empty block never has
/// capacity -- its `start` is meaningless and may be inside another
/// block -- so it always moves.
fn insert_into(
    arena: &mut Vec<NodeId>,
    caps: &mut Caps,
    start: usize,
    len: usize,
    at: usize,
    id: NodeId,
) -> (u32, u32) {
    let split = start + at;
    if len < caps.of(start, len) {
        arena.copy_within(split..start + len, split + 1);
        arena[split] = id;
        return pack(start, len + 1);
    }
    move_block_with(arena, caps, start..start + len, split, id)
}

/// Copy `block` to the end of the arena with `id` inserted at `split`
/// and room to grow, and record the room.
fn move_block_with(
    arena: &mut Vec<NodeId>,
    caps: &mut Caps,
    block: core::ops::Range<usize>,
    split: usize,
    id: NodeId,
) -> (u32, u32) {
    let len = block.len();
    let new_start = arena.len();
    let cap = (2 * len).max(4);
    arena.extend_from_within(block.start..split);
    arena.push(id);
    arena.extend_from_within(split..block.end);
    arena.resize(new_start + cap, NodeId::UNUSED);
    caps.moved(block.start, new_start, cap);
    pack(new_start, len + 1)
}

/// Take `id` out of the block, if it is there, shifting the tail down.
///
/// Nothing moves and nothing is left behind: the entry freed at the
/// end of the block is recorded as spare capacity for the next append.
fn drop_from(
    arena: &mut [NodeId],
    caps: &mut Caps,
    start: usize,
    len: usize,
    id: NodeId,
) -> (u32, u32) {
    let end = start + len;
    let Some(at) = arena[start..end].iter().position(|c| *c == id) else {
        return pack(start, len);
    };
    let hole = start + at;
    arena.copy_within(hole + 1..end, hole);
    // The vacated entry must not keep naming a node: compaction and
    // the fuzz target read whole blocks, and a stale identifier there
    // would look like a child listed twice.
    arena[end - 1] = NodeId::UNUSED;
    caps.shrunk(start, len);
    pack(start, len - 1)
}

impl Document {
    pub(super) fn block_range(
        &self,
        owner: NodeId,
        arena: Arena,
    ) -> Option<(u32, u32)> {
        let node = self.resolve(owner)?;
        match (arena, &node.data) {
            (Arena::Children, _) => Some(node.children),
            (Arena::Attributes, NodeData::Element { attributes, .. }) => {
                Some(*attributes)
            }
            (Arena::Namespaces, NodeData::Element { namespaces, .. }) => {
                Some(*namespaces)
            }
            _ => None,
        }
    }

    pub(crate) fn set_block_range(
        &mut self,
        owner: NodeId,
        arena: Arena,
        range: (u32, u32),
    ) {
        let Some(node) = self.resolve_mut(owner) else {
            return;
        };
        match arena {
            Arena::Children => node.children = range,
            Arena::Attributes => {
                if let NodeData::Element { attributes, .. } = &mut node.data {
                    *attributes = range;
                }
            }
            Arena::Namespaces => {
                if let NodeData::Element { namespaces, .. } = &mut node.data {
                    *namespaces = range;
                }
            }
        }
    }

    /// The one place a node's block changes.
    ///
    /// Returns whether `owner` has a block of that kind at all.
    pub(super) fn edit_block(
        &mut self,
        owner: NodeId,
        arena: Arena,
        edit: BlockEdit,
    ) -> bool {
        let Some(range) = self.block_range(owner, arena) else {
            return false;
        };
        let moved = match arena {
            Arena::Children => relocate_block(
                &mut self.child_ids,
                &mut self.child_caps,
                range,
                edit,
            ),
            Arena::Attributes => relocate_block(
                &mut self.attr_ids,
                &mut self.attr_caps,
                range,
                edit,
            ),
            Arena::Namespaces => {
                relocate_block(&mut self.ns_ids, &mut self.ns_caps, range, edit)
            }
        };
        self.set_block_range(owner, arena, moved);
        self.order_dirty = true;
        true
    }

    /// Drop `id` from its parent's child list, leaving the node alone.
    pub(super) fn unlink_from_parent(&mut self, id: NodeId) {
        if let Some(parent) = self.parent(id) {
            let _ = self.edit_block(
                parent,
                Arena::Children,
                BlockEdit::Without(id),
            );
        }
    }

    /// Allocate a node and append it to `parent`'s child list.
    pub(super) fn append_node(
        &mut self,
        parent: NodeId,
        data: NodeData,
    ) -> NodeId {
        let id = self.alloc_node(data, Some(parent));
        let _ = self.edit_block(parent, Arena::Children, BlockEdit::Append(id));
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u32) -> NodeId {
        NodeId {
            index: n,
            generation: 0,
        }
    }

    fn block(arena: &[NodeId], range: (u32, u32)) -> Vec<u32> {
        let (s, l) = (range.0 as usize, range.1 as usize);
        arena[s..s + l].iter().map(|n| n.index).collect()
    }

    #[test]
    fn appends_after_a_move_are_in_place() {
        let mut arena = Vec::new();
        let mut caps = Caps::new();
        let mut range = (0, 0);
        let mut moves = 0;
        for i in 0..1_000 {
            let before = range.0;
            range = relocate_block(
                &mut arena,
                &mut caps,
                range,
                BlockEdit::Append(id(i)),
            );
            if range.0 != before {
                moves += 1;
            }
        }
        assert_eq!(block(&arena, range), (0..1_000).collect::<Vec<_>>());
        assert!(moves <= 10, "{moves} moves for 1,000 appends");
        assert!(arena.len() < 4_000, "arena is {} entries", arena.len());
    }

    #[test]
    fn an_empty_block_never_writes_into_someone_elses() {
        // Two nodes with `(0, 0)` must not share a block just because
        // their ranges happen to coincide.
        let mut arena = Vec::new();
        let mut caps = Caps::new();
        let first = relocate_block(
            &mut arena,
            &mut caps,
            (0, 0),
            BlockEdit::Append(id(1)),
        );
        let second = relocate_block(
            &mut arena,
            &mut caps,
            (0, 0),
            BlockEdit::Append(id(2)),
        );
        assert_eq!(block(&arena, first), [1]);
        assert_eq!(block(&arena, second), [2]);
        assert_ne!(first.0, second.0);
    }

    #[test]
    fn removal_shifts_in_place_and_frees_capacity() {
        let mut arena = Vec::new();
        let mut caps = Caps::new();
        let mut range = (0, 0);
        for i in 0..4 {
            range = relocate_block(
                &mut arena,
                &mut caps,
                range,
                BlockEdit::Append(id(i)),
            );
        }
        let len_before = arena.len();
        let after = relocate_block(
            &mut arena,
            &mut caps,
            range,
            BlockEdit::Without(id(1)),
        );
        assert_eq!(after.0, range.0, "no move");
        assert_eq!(block(&arena, after), [0, 2, 3]);
        assert_eq!(arena.len(), len_before, "no garbage");
        let again = relocate_block(
            &mut arena,
            &mut caps,
            after,
            BlockEdit::Append(id(9)),
        );
        assert_eq!(again.0, after.0, "the freed entry is reused");
        assert_eq!(block(&arena, again), [0, 2, 3, 9]);
        let missing = relocate_block(
            &mut arena,
            &mut caps,
            again,
            BlockEdit::Without(id(42)),
        );
        assert_eq!(
            missing, again,
            "removing what is not there changes nothing"
        );
    }

    #[test]
    fn insertion_keeps_order_whether_or_not_the_block_moves() {
        let mut arena = Vec::new();
        let mut caps = Caps::new();
        let mut range = (0, 0);
        for i in [0, 2] {
            range = relocate_block(
                &mut arena,
                &mut caps,
                range,
                BlockEdit::Append(id(i)),
            );
        }
        range = relocate_block(
            &mut arena,
            &mut caps,
            range,
            BlockEdit::InsertAt(1, id(1)),
        );
        assert_eq!(block(&arena, range), [0, 1, 2]);
        // Fill the capacity, then insert at the front to force a move.
        range = relocate_block(
            &mut arena,
            &mut caps,
            range,
            BlockEdit::Append(id(3)),
        );
        range = relocate_block(
            &mut arena,
            &mut caps,
            range,
            BlockEdit::InsertAt(0, id(7)),
        );
        assert_eq!(block(&arena, range), [7, 0, 1, 2, 3]);
        range = relocate_block(
            &mut arena,
            &mut caps,
            range,
            BlockEdit::InsertAt(99, id(8)),
        );
        assert_eq!(
            block(&arena, range),
            [7, 0, 1, 2, 3, 8],
            "past the end appends"
        );
    }

    #[test]
    fn a_range_outside_the_arena_is_treated_as_empty() {
        let mut arena = Vec::new();
        let mut caps = Caps::new();
        let range = relocate_block(
            &mut arena,
            &mut caps,
            (50, 3),
            BlockEdit::Append(id(1)),
        );
        assert_eq!(block(&arena, range), [1]);
    }
}
