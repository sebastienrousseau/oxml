# 0007 — Own the strings; borrow later

**Status:** Accepted, and the intended fix is now implemented.
See [design/owned-input.md](../design/owned-input.md).

## Context

`roxmltree` and `quick-xml` hand back slices of the caller's input.
oxml allocates an owned `String` for every text node, attribute value,
and element name before interning.

That costs. Measured with a counting global allocator: **4.13
allocations per node**, 66,037 for a 16,002-node document. Most of the
remainder are those strings.

## Decision

Keep owning them, for now. Do not add a lifetime parameter to
`Document`.

## Reasoning

A borrowing design has been prototyped twice in this repository, and
both times the API cost landed on the caller:

`Document<'a>` infects everything that holds one. A struct storing a
parsed document grows a lifetime; a function returning one needs the
input to outlive it; a document parsed from bytes that had to be
transcoded (UTF-16, ISO-8859-1) has no input to borrow *from*, because
the decoded `String` is a temporary the parser created.

That last case is the decisive one. `parse_bytes` on a UTF-16 document
must produce an owned buffer, so a borrowing `Document` either cannot
support it or needs a second representation.

## The fix, now done

The document owns its input — a `String` inside `Document` — with
nodes holding `(start, len)` ranges into it, exactly as they already
hold ranges into the child and attribute arenas.

That removed the per-node strings without a lifetime parameter, and it
works identically for `parse` and for a transcoded `parse_bytes`,
which was the decisive case. Measured: **0.50 allocations per node**,
down from 4.13 when this ADR was written and 1.13 immediately before
the change.

The decision this ADR records — *no lifetime parameter on `Document`*
— stands, and the outcome is that oxml gets most of what a borrowing
design offers while `Document` remains a plain owned type that
outlives whatever it was parsed from.

The cost is one copy of the decoded input, against one allocation per
text node and per attribute value. It is not free, and the README says
so.

## What the number is held to

The figure is published rather than rounded down, and
`crates/oxml/tests/allocations.rs` holds it to a ceiling so it cannot
drift upward unnoticed. The ceiling came down from 1.3 to 0.6 with
this change.
