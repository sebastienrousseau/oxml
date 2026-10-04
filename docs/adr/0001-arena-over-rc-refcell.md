# 0001 — An arena of indices, not `Rc<RefCell<Node>>`

**Status:** Accepted

## Context

A tree needs parent links. In Rust the obvious encodings are a graph of
`Rc<RefCell<Node>>` with `Weak` parents, or a flat arena where every
node is an index.

## Decision

An arena. `Document` owns a `Vec<Node>`, a `Vec<ExpandedName>` of
interned element names, and two `Vec<NodeId>` holding every child list
and attribute list concatenated. A `Node` stores `(start, len)` ranges
into those.

## Consequences

**Gained**

- `Document` is `Send + Sync` with no work: immutable after parsing,
  no interior mutability.
- `NodeId` is `Copy` and pointer-sized, so holding a position does not
  borrow the tree. No lifetime parameter spreads through caller code.
- Traversal is index arithmetic on a contiguous vector.
- Ranges rather than a `Vec` per node, plus interned names, took the
  measured figure from 4.13 to **1.13 allocations per node**. The rest
  is owned strings — see
  [design/owned-input.md](../design/owned-input.md).

**Given up**

- A `NodeId` from one document is a valid index into another and means
  something different. Nothing detects this.
- No mutation. Adding it would mean invalidating ranges, which is the
  point at which this design stops paying.

## What would change it

A requirement to build or modify documents. At that point the range
encoding becomes a liability and a different structure is warranted.
