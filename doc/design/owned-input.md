<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Design note — owned input

**Status:** Planned. The remaining half of the allocation work.

## Where the allocations are

Measured with a counting global allocator over a 16,002-node document:
**66,037 allocations, 4.13 per node.**

There are two pieces.

**One — flatten and intern. Done.** Child lists and attribute lists are
`(start, len)` ranges into two shared vectors, and element names are
interned behind a `NameId`. That took the measured figure from 4.13 to
**3.13 allocations per node**.

**Two — own the input**, which is the rest of this note, plus interning
attribute names.

The owned `String`s that remain:

| Source | Roughly |
|---|---|
| Attribute names | one or two per attribute — an `Attribute` owns a full `ExpandedName` |
| Attribute values | one per attribute |
| Text node contents | one per text node |

Attribute names are the largest single remaining source: 6,000
attributes on the benchmark document account for roughly 18,000
allocations. Interning them is the same change already made for
elements, and it is separate only because `Attribute::name` is public,
so replacing it with a `NameId` is a breaking change that ripples into
the satellite crates.

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

`Document::text` currently returns `String`. Returning `&str` is a
breaking change for callers who move the result — mechanical to fix,
but it is a change and belongs in a version that says so.

Entity expansion complicates it: an attribute value containing
`&amp;` does not appear verbatim in the input, so it cannot be a range
into it. Those need a side table of expanded values, with the range
form used for the common case where no expansion occurred.

## How progress is measured

`crates/oxml/tests/allocations.rs` counts and holds the figure to a
ceiling. When this lands, the ceiling comes down and the README number
changes with it — the README publishes 4.1 today, and it publishes
whatever is true after.
