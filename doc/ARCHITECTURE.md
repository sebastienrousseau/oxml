<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Architecture

How oxml is put together, and why it is shaped this way. For the API,
read [the rustdoc](https://docs.rs/oxml). This document is about the
decisions.

## Contents

- [The shape of the crate](#the-shape-of-the-crate)
- [The document is an arena](#the-document-is-an-arena)
- [Names are interned](#names-are-interned)
- [Parsing is recursive descent](#parsing-is-recursive-descent)
- [The encoding layer runs first](#the-encoding-layer-runs-first)
- [Limits are a value, not a mode](#limits-are-a-value-not-a-mode)
- [XPath compiles, then evaluates](#xpath-compiles-then-evaluates)
- [What is deliberately absent](#what-is-deliberately-absent)

## The shape of the crate

```
src/
├── lib.rs         re-exports, MAX_DEPTH, the README doctest anchor
├── parser.rs      recursive-descent parser; the largest module
├── tree.rs        Document, Node, NodeId, ExpandedName, Attribute
├── dtd.rs         the internal subset: ELEMENT, ATTLIST, ENTITY, …
├── encoding.rs    BOM and declaration sniffing; UTF-16 and Latin-1
├── error.rs       Error, ErrorKind, line/column reporting
├── limits.rs      Limits, Edition, the three profiles
├── names4e.rs     XML 1.0 fourth-edition name tables
└── xpath/
    ├── mod.rs     XPath, the compile/evaluate entry points
    ├── parser.rs  expression → syntax tree
    ├── eval.rs    syntax tree + document → Value
    └── float.rs   floor/ceil/trunc, shimmed for no_std
```

Each module has a single reason to change. `names4e.rs` exists because
the fourth-edition character tables are large, mechanical, and
irrelevant to anyone reading the parser.

## The document is an arena

A `Document` owns three flat vectors and hands out indices:

```rust,ignore
pub struct Document {
    nodes: Vec<Node>,
    names: Vec<ExpandedName>,   // interned; nodes hold a NameId
    child_ids: Vec<NodeId>,     // every child list, concatenated
    attr_ids: Vec<NodeId>,      // every attribute list, concatenated
    scratch: Vec<NodeId>,       // reused during construction
}
```

A `Node` holds `(start, len)` ranges into `child_ids` and `attr_ids`
rather than owning a `Vec` each. That is the difference between one
allocation per node and one allocation per node *plus one per child
list* — on a document of 16,002 nodes it is the difference between
roughly 32,000 allocations and roughly 16,000.

Three consequences follow:

1. **`NodeId` is `Copy` and pointer-sized.** Holding a position does
   not borrow the tree, so you can collect ids, store them, and return
   them without a lifetime parameter spreading through your code.
2. **Traversal is index arithmetic.** `children(id)` is a slice of a
   vector, not a pointer chase through separately allocated cells.
3. **Ids are not checked across documents.** A `NodeId` from one
   `Document` is a valid index into another and means something else
   entirely. This is the cost of the design and it is not mitigated.

### Why not `Rc<RefCell<Node>>`

Because a tree of reference-counted cells allocates once per node,
scatters those allocations across the heap, pays a borrow check at
every access, and cannot be `Send`. The arena is `Send + Sync` for
free: it is immutable after parsing and contains no interior
mutability.

## Names are interned

Element and attribute names are stored once in `Document::names`, and
nodes carry a `NameId` index. A document with 2,000 `<item>` elements
holds one `"item"` string, not 2,000.

Interning also makes namespace comparison correct by construction.
`ExpandedName` holds the namespace **URI**, not the prefix, so
`<a:x xmlns:a="u"/>` and `<b:x xmlns:b="u"/>` compare equal, which is
what the Namespaces in XML specification requires and what a
prefix-comparing parser gets wrong.

## Parsing is recursive descent

One function per production, closely following the grammar in the XML
specification. It is not the fastest possible structure — a table-driven
or SIMD-accelerated scanner would beat it — but it is the structure in
which a conformance failure can be traced to a rule.

Recursion means stack depth grows with document nesting, which is why
`Limits::max_depth` exists and why `permissive()` does not raise it.
See [SECURITY-MODEL.md](SECURITY-MODEL.md).

## The encoding layer runs first

`parse` takes a `&str`: the caller has already decided. `parse_bytes`
runs `encoding::decode` first, which inspects the byte-order mark and
the declaration, then hands a `&str` to the same parser.

Keeping the layers separate means the encoding logic can be tested on
its own — and it needed to be. It reached 78.7% coverage only because
W3C conformance documents happened to run through it; a decoder
sabotaged to substitute U+FFFD for unpaired surrogates left all 2,585
conformance tests green.

UTF-8 is returned as `Cow::Borrowed`, so the common case transcodes
nothing.

## Limits are a value, not a mode

`Limits` is a plain `Copy` struct passed to `parse_with`. No global
state, no builder, no environment variable. Two documents on two
threads can use different limits, and a limits value can be stored in
a configuration struct and compared.

It is `#[non_exhaustive]`, so a bound added later is not a breaking
change for callers who wrote out a struct literal. The supported
pattern is to start from a profile and assign the fields you care
about.

## XPath compiles, then evaluates

`XPath::compile` produces a syntax tree that is independent of any
document. `evaluate` walks it against a `Document`. The split matters
because compilation is the expensive half and evaluation is the half
you repeat: a server compiles every expression it knows at startup and
evaluates them per request, across threads, because `XPath` is
`Send + Sync`.

`evaluate_from` takes a context node, which is how a relative
expression runs against each match of an outer one.

## What is deliberately absent

- **Mutation and serialisation.** They are one feature, not two, and
  neither is implemented. The tree is built by the parser and read
  afterwards.
- **External entity resolution.** There is no code that opens a file or
  a socket on a document's behalf. This is why XXE is foreclosed by
  construction rather than by a default.
- **`unsafe`.** `#![forbid(unsafe_code)]`, checked in CI. This costs
  throughput; see [COMPARISON.md](COMPARISON.md).
- **Streaming.** The whole document is built in memory. For gigabyte
  inputs, use `quick-xml`.
