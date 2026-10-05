<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Design note — owned input

**Status:** Done. Measured at **0.50 allocations per node**, down
from 1.13.

## Where the allocations are

Measured with a counting global allocator over a 16,004-node document:
**8,076 allocations, 0.50 per node** — from 18,073 and 1.13 before
this landed.

There are two pieces.

**One — flatten and intern. Done.** Child lists and attribute lists are
`(start, len)` ranges into two shared vectors, and element *and*
attribute names are interned behind a `NameId`. That took the measured
figure from 4.13 to **1.13 allocations per node**.

Worth recording how: interning by itself moved the figure by nothing.
The name was resolved into a freshly allocated `ExpandedName` and only
then looked up, so the allocation had already happened. Resolving to
borrowed parts and looking up by those is what actually removed it.

**Two — own the input.** Done, and the rest of this note describes
how it went.

The owned `String`s that remain:

| Source | Roughly |
|---|---|
| Attribute values | one per attribute |
| Text node contents | one per text node |

Names are already gone: they borrow the input until interning, so a
repeated name allocates nothing at all. What is left is exactly the two
things this note is about.

## The change

`Document` owns its input:

```rust,ignore
pub struct Document {
    input: String,              // the decoded document
    nodes: Vec<Node>,
    // NodeKind::Text(Range) rather than NodeKind::Text(String)
}
```

Text nodes and attribute values become `(start, len)` into `input`,
exactly as child lists already are into `child_ids`. `text()` returns
`&str` borrowed from the document rather than an owned `String`.

## Why not a lifetime parameter

`Document<'a>` borrowing the caller's `&str` is the design `roxmltree`
uses and it was rejected — see
[ADR 0007](../adr/0007-owned-strings-for-now.md). The short version:
`parse_bytes` on a UTF-16 or ISO-8859-1 document has nothing to borrow
from, because the decoded string is a temporary the parser made.

Owning the input handles both entry points identically and keeps
`Document` free of lifetimes.

## What it breaks

`Document::text` was expected to return `&str`. It does not, and
cannot: it is `XPath`'s string-value, which *concatenates* a node's
descendants, and a concatenation of several text nodes is not a slice
of anything. It still returns `String`.

What changed instead is `Document::kind`, which now returns a
**borrowed view** — `NodeKind<'_>` whose `Text` and `Comment` carry
`&str` and whose `Attr` carries `Attribute<'_>` — resolved from the
stored ranges on access. Storage and view are separate types
(`NodeData` and `NodeKind`) because the stored form must not borrow:
the document owns both the nodes and the text they point into, and a
node holding a `&str` into its own document would be
self-referential.

That turned out to be the *less* disruptive change. Callers written as
`Some(NodeKind::Text(t)) => t.trim()` still compile, because `&str`
and `&String` both have `trim`. What breaks is code that `clone()`s
the payload expecting a `String`, which now needs `to_owned()`.

Entity expansion complicated it as predicted: an attribute value
containing `&amp;` does not appear verbatim in the input, so it cannot
be a range into it. Those go in a side table, with the range form used
for the common case. The accumulator that decides between them is
`parser::Run`, which stays a range until something — an expansion, or
a `CDATA` delimiter splitting a run into non-contiguous pieces —
forces it to materialise.

## How progress is measured

`crates/oxml/tests/allocations.rs` counts and holds the figure to a
ceiling, which came down from 1.3 to 0.6 when this landed. The README
publishes 0.50.

The throughput effect is smaller than the allocation effect and, on
this machine, not separable from noise: the `tree` ratio against
`roxmltree` moved from a median of 0.323 to 0.351 across six runs
either side, which is inside the ±8% spread the ratio itself shows
run to run. The allocation count is deterministic and is the honest
headline; the speed claim is not made.
