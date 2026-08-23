# 0007 — Own the strings; borrow later

**Status:** Accepted

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

## The intended fix

Have the document own its input — a `String` inside `Document` — with
nodes holding `(start, len)` ranges into it, exactly as they already
hold ranges into the child and attribute arenas.

That removes the per-node strings without a lifetime parameter, and it
works identically for `parse` and for a transcoded `parse_bytes`. It is
planned and not done.

## Meanwhile

The number is published rather than rounded down: the README says 4.1,
says what the remaining allocations are, and a test holds it to a
ceiling so it cannot drift upward unnoticed.
