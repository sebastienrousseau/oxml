<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Comparison

How oxml differs from the alternatives, at the level of what they do
rather than how fast they do it. For throughput, see
[BENCHMARKS.md](BENCHMARKS.md) — and read why it publishes no figures.

Assessed 2026-08. Anything here about another project may have changed;
check before relying on it.

## Contents

- [Rust](#rust)
- [Beyond Rust](#beyond-rust)
- [What oxml does that others do not](#what-oxml-does-that-others-do-not)
- [What others do that oxml does not](#what-others-do-that-oxml-does-not)
- [Choosing](#choosing)

## Rust

| | oxml | quick-xml | roxmltree | sxd-document/xpath | libxml (bindings) |
|---|---|---|---|---|---|
| Model | Tree | Pull parser | Tree | Tree | Tree |
| XPath 1.0 | Yes | No | No | Yes | Yes |
| XSLT | No | No | No | No | Yes |
| XSD validation | Separate crate | No | No | No | Yes |
| Borrows from input | Not yet | Yes | Yes | No | No |
| `no_std` | Yes (`alloc`) | Partial | No | No | No |
| `unsafe` | Forbidden | Some | Some | Some | C library |
| C dependency | None | None | None | None | libxml2 |
| Streaming | Events, in memory | Yes, from a reader | No | No | Yes |
| Mutation / writing | No | Writing | No | Yes | Yes |

### `quick-xml`

A pull parser, and the right tool for streaming. It hands you events;
you build whatever you need. That is why it is fast — it does not
allocate a tree — and why it is more work to use for random access.

oxml's [`stream`](https://docs.rs/oxml/latest/oxml/stream/) module is
the same idea over the same scanner, and skips the tree in the same
way. The difference that remains is the input: `quick-xml` reads from
anything implementing `BufRead`, so a document need never be resident.
oxml takes a `&str`.

Use it for gigabyte documents, and for pipelines where you touch a
small part of a large file. It will stay faster than oxml at
tokenisation, and that comparison is not the interesting one: oxml's
argument is what you can do after parsing.

### `roxmltree`

The closest comparison. A read-only tree, no `unsafe`-free guarantee,
and — importantly — it **borrows from the input**, so a node's text is
a slice of the document you passed in rather than an owned `String`.

oxml reaches the same place by a different route: rather than borrow
the caller's string, the document **owns** its input and a node's text
is a range into that. It costs one copy of the document and means no
lifetime parameter, which is what makes `parse_bytes` on a UTF-16
document work at all -- there, the decoded string is a temporary with
nothing to borrow from. The measured cost is **0.50 allocations per
node**, down from 4.13.

`roxmltree` has no XPath. If you need queries, you are writing the
traversal yourself.

### `sxd-document` / `sxd-xpath`

The first XPath implementation in Rust, and the reason a lot of this is
possible to compare against at all. It uses an arena with interior
mutability, so a `Document` is not `Send`.

oxml's tree is immutable after parsing and therefore `Send + Sync`,
which is what makes compile-once-evaluate-many across a thread pool
work.

### `libxml` bindings

Wraps libxml2, so you get XSLT, XSD, XPath and decades of correctness
work — along with a C dependency, a build toolchain requirement, and
libxml2's CVE stream.

If you need XSLT, this is the answer. oxml does not have it and is not
planning to.

## Beyond Rust

Worth knowing what the ceiling looks like.

| | oxml | libxml2 (C) | Xerces (C++/Java) | encoding/xml (Go) | lxml (Python) |
|---|---|---|---|---|---|
| XPath | 1.0 | 1.0 | 1.0 | No | 1.0 |
| XSLT | No | 1.0 | Via Xalan | No | 1.0 |
| XSD | Separate crate | Yes | Yes | No | Yes |
| Streaming | Events, in memory | Yes | Yes | Yes | Yes |
| Memory safety | Guaranteed | Manual | Manual / GC | GC | Manual (C core) |

**libxml2** is the yardstick for behaviour. When oxml and libxml2
disagree about a document, the assumption is that oxml is wrong.

**Xerces** is the most complete: XSD 1.1, full DOM, streaming. It is
also enormous.

**Go's `encoding/xml`** is the interesting counter-example: no XPath, no
DTD, no schema, and a deliberate refusal to grow them. Plenty of
programs need no more than that.

## What oxml does that others do not

- **`#![forbid(unsafe_code)]` with a tree and XPath.** Not "we avoid
  unsafe" — a build property, checked in CI.
- **`no_std` with `alloc`**, including XPath. It runs on a
  microcontroller.
- **Limits as a value.** Ten bounds in three profiles, passed per
  parse, no global state.
- **XXE foreclosed structurally.** No code exists that opens a file or
  a socket. There is no option to turn resolution on, so there is no
  way to configure the vulnerability back in.
- **XML 1.0 fourth *and* fifth edition name rules**, selectable per
  parse. The two are not a widening: each admits names the other
  rejects.
- **A published conformance number with its denominator**, and a
  ratchet that fails on unreviewed improvement as well as regression.

## What others do that oxml does not

Stated plainly, because a comparison that only lists wins is an
advertisement:

- **Streaming from a reader.** `quick-xml`, libxml2, Xerces and Go
  all handle documents larger than memory. oxml's
  [`stream::Reader`](https://docs.rs/oxml/latest/oxml/stream/struct.Reader.html)
  yields events without building a tree — measured at 92% less held at
  peak than parsing the same document — but it is handed a `&str` and
  normalises it into another, so the whole document is still resident.
  Incremental I/O is the missing half.
- **Raw parse throughput.** `quick-xml` is years ahead and will stay
  there.
- **Borrowing from the input.** `roxmltree` and `quick-xml` do; oxml
  allocates owned strings.
- **XSLT.** libxml2 and lxml have it.
- **Mutation and serialisation.** oxml reads; it does not write.
- **XPath 2.0 / 3.1.** Saxon has it. oxml implements 1.0.
- **The external DTD subset.** Which is what most of oxml's remaining
  conformance failures need.

## Choosing

- Streaming gigabytes, or reading from a socket → **`quick-xml`**
- A read-only tree, no queries, maximum speed → **`roxmltree`**
- XSLT, or XSD today → **`libxml`** bindings
- XPath on a tree, in safe Rust, possibly without `std` → **`oxml`**
- Nothing but a struct out of a small document → **`serde-xml-rs`** or
  `quick-xml`'s serde support
