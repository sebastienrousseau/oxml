# 0008 — A mutable arena: generational slots and relocating blocks

**Status:** Accepted. Supersedes the "no mutation" consequence of
[0001](0001-arena-over-rc-refcell.md); the arena itself stands.

## Context

ADR 0001 chose a flat arena with `(start, len)` ranges into shared
child and attribute lists, and gave up mutation to get it: "Adding it
would mean invalidating ranges, which is the point at which this
design stops paying." Version 0.0.9 added mutation anyway, and the
first implementation confirmed the warning in four ways.

- **Appending was quadratic in memory.** A block cannot grow in place
  when the next block starts right after it, so every append copied
  the whole block to the end of the arena and left the old one as
  garbage. Appending 10,000 children retained 537 MB; 20,000 retained
  2.1 GB. `Document::clone` copied the garbage.
- **A slot could be freed twice.** `remove(attr)` freed the attribute's
  slot but left the element's attribute range pointing at it;
  `remove(element)` then walked the range and freed it again. The
  generation went odd, then even again, and the slot was on the free
  list twice: the next two allocations shared it, one identifier did
  not resolve, and one node vanished from the output.
- **Nothing was validated.** A child could be appended under a text
  node (stored, then silently dropped by the serialiser); a second
  element could be moved under the root; `remove(root)` succeeded;
  `append_element(top, None, "bad name")` serialised to something the
  parser refused.
- **Namespaces were not written.** `append_element(top, Some("urn:x"),
  "e")` serialised as `<e/>`, and an element moved out from under its
  `xmlns:m` declaration was written with an unbound prefix.

## Decision

Keep the arena. Make four changes to it.

### Slots are generational, and freed exactly once

Every slot carries a generation beside the node: even is live, odd is
removed. Removal bumps the generation, so every `NodeId` minted for
the slot stops resolving, and reuse bumps it again so the new
identifier differs from every old one. `free_slot` refuses a slot
that is already odd, which closes the double-free: whatever path
reaches a slot a second time, it is freed once and listed once.
`len()` is `nodes - free_slots`, and the fuzz target checks it
against a walk.

### Blocks move only when they must, and bring capacity with them

One function, `relocate_block`, is the only place a block changes. A
removal shifts the tail down in place and records the vacated entry as
spare capacity. An insertion writes in place while the block has
capacity; when it has none, the block is copied to the end of the
arena with room for twice its length, and the capacity is recorded in
a side table keyed by the block's start. A parsed document has no
entries in that table and pays nothing until it is mutated.

The cost is garbage proportional to the number of *moves*, which is
logarithmic in the number of appends to one block, rather than to the
number of appends. Measured: 10,000 appended children retain under
2 MB where they retained 537 MB. `Document::compact` rebuilds the
arenas in document order for a document that is mutated heavily and
then kept.

An empty block never has capacity. Its `start` is meaningless -- every
new node is born with `(0, 0)` -- and may lie inside another block, so
an in-place write there would corrupt a stranger. That rule is in the
capacity lookup, not in a caller, because the caller is the thing
that would forget it.

### Every operation is gated before it touches the arena

Three gates, in order: the identifiers resolve (`Stale`); the kinds fit
(`NotAContainer`, `NotAChild`, `NotAnElement`, `NotRemovable`,
`RootElementExists`, `WouldCycle`); the content is writable
(`InvalidName`, `InvalidText`). A name must be an `NCName`. Character
data must be expressible in some version of XML -- XML 1.1's `Char`,
so a control character is accepted and written as a reference under a
1.1 declaration, and only `NUL`, `U+FFFE` and `U+FFFF` are refused.
A comment or processing instruction is written verbatim and is held
to what may appear literally: XML 1.0's `Char`, no `--`, no `?>`.

The alternative, validating at serialisation, was rejected because it
moves the error to the wrong call. The caller that passed the bad name
is the one that can fix it.

### Namespaces are resolved when the document is written

A mutation-created name records its namespace URI and no prefix. The
serialiser keeps a scope of bindings as it descends: the parsed prefix
is used when it is bound to the right URI here; failing that, the
default namespace for an element, then any prefix in scope bound to
that URI, and finally a declaration on the element itself, with the
parsed prefix if it is free and `ns1`, `ns2`, ... otherwise. An
element in no namespace under a default namespace is written with
`xmlns=""`. `declare_namespace` exists to choose the prefix and the
element it is declared on; it is never required.

The parse → serialise fixed point over the W3C corpus is unchanged by
this: a parsed document carries every declaration it needs as namespace
nodes, so the scope only confirms what the parser resolved.

## Consequences

**Gained**

- Retained memory per append is O(1) amortised; replacing a text or
  attribute value overwrites in place, so 1,000 replacements of a
  10 KB value retain one value rather than 10 MB.
- A document built by hand parses back to the same expanded names,
  namespaces included.
- Every invalid operation is an error at the call. The fuzz target
  asserts that no sequence of operations produces a document that
  fails to reparse.
- Document order is available for a mutated document:
  `descendants_of` walks the tree in pre-order, and `document_order`
  caches a position per slot behind a dirty flag, so `Document` stays
  `Send + Sync` without interior mutability.

**Given up**

- Arena order is no longer document order after a move or a slot
  reuse. `nodes()` says so in its name; `descendants()` keeps its old
  name and its old behaviour for compatibility.
- `len()` counts live nodes, so an index can exceed it.
- `document_order` needs `&mut self` to refresh; a reader holding
  `&Document` sees the cached order and `is_order_dirty()`.
- A `BTreeMap` per arena for capacities, empty on a parsed document.

## What would change it

A workload that mutates far more than it reads. The block layout is
chosen for reading: contiguous children, one allocation per document
rather than per node. A linked list per node would make every edit
O(1) with no garbage at all, at the cost of the locality the parser
and `XPath` evaluator were built around. Nothing measured so far asks
for that trade.
