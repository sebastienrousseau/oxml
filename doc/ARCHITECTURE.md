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

A `Document` owns flat vectors and hands out indices:

```rust,ignore
pub struct Document {
    nodes: Vec<Node>,
    names: Vec<ExpandedName>,   // element names, interned
    child_ids: Vec<NodeId>,     // every child list, concatenated
    attr_ids: Vec<NodeId>,      // every attribute list, concatenated
    scratch: Vec<NodeId>,       // reused while building
}
```

A `Node` holds a `(start, len)` range into `child_ids` rather than
owning a `Vec`, and an element holds one into `attr_ids`. That removed
one allocation per element with children and one per element with
attributes: the measured figure went from **4.13 to 3.13 allocations
per node** on a 16,002-node document, and interning names by borrowed
parts took it to **1.13**. Having the document own its input, so text
nodes, comments and attribute values became ranges into it too, took
it to **0.50**.

Children need the `scratch` stack because a node's children are only
known to be complete when its end tag arrives; they are copied into
`child_ids` as one contiguous block at that point. Attributes need no
such stack, because they are pushed consecutively while reading the
start tag and are already contiguous.

`Document::with_capacity` sizes the arena from a single byte-scan for
`<`, since every element, comment and processing instruction begins
with one. Counting runs at memory speed; the reallocate-and-copy it
avoids does not.

> **Still to do.** Text and attribute values are owned `String`s. See
> [design/owned-input.md](design/owned-input.md).

Three consequences follow from the arena:

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
nodes carry a `NameId`. A document with 2,000 `<item>` elements holds one `"item"`,
not 2,000. The index is keyed on the local part, so a document with
many distinct names does not degrade to a linear scan of the table.

Attribute names share the same table, so an element `<item>` and an
attribute `item="…"` in the same namespace resolve to the same handle
and compare as a `u32`.

Interning alone did **not** reduce allocations, and measuring is the
only reason that was noticed. The name was resolved into a freshly
allocated `ExpandedName` and only *then* looked up, so the allocation
had already happened — interning reduced what the document retained
and left the figure unmoved at 3.13. Resolving to borrowed parts and
looking up by those took it to 2.63, and deleting the now-dead resolve
that still ran for every element took it to **2.25**: a repeated name
costs a map probe and nothing else.

Clippy found that one, as an unused variable. It had been allocating
an `ExpandedName` per element for no reason since the interning went
in, and the measurement alone would not have located it.

The largest single step came last and was the simplest:
`parse_name_unchecked` copied each name out of the input with
`to_owned()`. Every name in a document is already a slice of the
input, and interning discards the copy immediately, so that was one
allocation per element *and* per attribute for nothing. Returning a
borrowed `&'a str` took the figure from 2.25 to **1.13** -- half the
remaining allocations, from deleting two words. Character data went
the same way afterwards: see [design/owned-input.md](design/owned-input.md).

Either way, `ExpandedName` holds the namespace **URI** rather than the
prefix, which makes namespace comparison correct by construction.
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

## Mutation and serialisation

They are one feature, not two: a tree that can be changed is only
useful if what comes out parses back to the same tree. Both live on
the same arena the parser builds.

**Slots are generational.** Every arena slot carries a generation
beside its node, even for live and odd for removed. `remove` bumps it,
so every `NodeId` minted for the slot stops resolving instead of
addressing whatever occupies the slot next; reuse bumps it again. A
slot is freed exactly once whatever path reaches it, and `len()`
counts live nodes.

**Blocks relocate with spare capacity.** Children, attributes and
namespace nodes are `(start, len)` blocks in three shared arenas, which
is what makes reading cheap and is why a block cannot grow in place.
One function edits blocks: removal shifts the tail down in place, and
an insertion writes in place while the block has capacity. When it has
none the block is copied to the end of its arena with room for twice
its length, so appends are O(1) amortised in memory rather than
quadratic -- the first version retained 537 MB for 10,000 appended
children. `Document::compact` rebuilds the arenas without the garbage
that moves leave behind. Details and measurements are in
[adr/0008](adr/0008-generational-slots-and-block-relocation.md).

**Every operation is gated.** Identifiers must resolve, kinds must fit
(only the root and elements take children, only one element sits
under the root, the root is never removed, an attribute node is never
a child), and content must be XML the serialiser can write: names are
`NCName`s, text is anything some XML version can express. An error
comes back from the call that caused it, not from the round trip.

**Document order is cached, not assumed.** The parser appends in
pre-order, so for a parsed document arena order is document order; a
move or a slot reuse breaks that. `descendants_of` walks children in
pre-order, and `document_order` computes a position per slot behind a
dirty flag that every mutation sets. The cache is a plain `Vec`, so
`Document` stays `Send + Sync` with no interior mutability.

**The serialiser resolves namespaces as it descends.** A parsed name
keeps the prefix it was written with; a name the mutation API created
has only its namespace URI. The writer keeps a scope of bindings and
uses the stored prefix when it is bound here, else a prefix in scope
for that URI, else declares one on the element. The output is a fixed
point over the W3C corpus and parses back to the same expanded names
for a built document. The XML version is declared only when a C0
control character or a prefix undeclaration requires 1.1.

## What is deliberately absent

- **External entity resolution.** There is no code that opens a file or
  a socket on a document's behalf. This is why XXE is foreclosed by
  construction rather than by a default.
- **`unsafe`.** `#![forbid(unsafe_code)]`, checked in CI. This costs
  throughput; see [COMPARISON.md](COMPARISON.md).
- **Streaming from a reader.** [`stream::Reader`](../crates/oxml/src/stream.rs)
  yields events without building a tree, but it is handed a `&str`, so
  the document is resident even though its tree is not. For inputs
  larger than memory, or for reading from a socket, use `quick-xml`.
