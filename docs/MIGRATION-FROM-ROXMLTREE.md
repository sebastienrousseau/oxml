<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Migrating from `roxmltree`

The closest neighbour: both build a read-only tree over a document you
already have in memory. The move is mostly mechanical.

## What you gain

- **XPath 1.0.** `roxmltree` has none, so any query is a traversal you
  wrote by hand.
- **`#![forbid(unsafe_code)]`**, checked in CI.
- **`no_std` with `alloc`.**
- **Configurable limits** — ten bounds in three profiles.

## What you give up

- **Where the text lives.** `roxmltree` hands you `&str` slices of the
  document *you* passed in and holds a lifetime to it. oxml owns its
  input and stores ranges into that, so `Document` has no lifetime
  parameter and outlives whatever you parsed from — which is also what
  lets `parse_bytes` decode UTF-16, where there is no caller string to
  borrow. The measured cost is **0.50 allocations per node**.
- **Some throughput**, for the same reason.

If your workload is "parse a document, read a few fields, drop it", and
you do not need queries, `roxmltree` remains the better fit.

## Name for name

| `roxmltree` | oxml |
|---|---|
| `Document::parse(s)` | `oxml::parse(s)` |
| `doc.root()` | `doc.root()` |
| `doc.root_element()` | `doc.root_element().unwrap()` |
| `node.tag_name().name()` | `doc.element_name(id).unwrap().local` |
| `node.tag_name().namespace()` | `doc.element_name(id).unwrap().namespace` |
| `node.attribute("x")` | `doc.attribute(id, "x")` |
| `node.attributes()` | `doc.attributes(id)` |
| `node.children()` | `doc.children(id)` |
| `node.parent()` | `doc.parent(id)` |
| `node.descendants()` | `doc.descendants()` (whole document) |
| `node.text()` | first text child — see below |
| `node.is_element()` | `doc.is_element(id)` |

## Three differences that will bite

**Nodes are ids, not references.** `roxmltree::Node` borrows the
document; `oxml::NodeId` does not. This is usually a simplification —
you can store ids without a lifetime parameter — but every accessor now
takes the document:

```rust
use oxml::parse;

let doc = parse("<r><a/></r>").unwrap();
let root = doc.root_element().unwrap();
// roxmltree: root.children().count()
assert_eq!(doc.children(root).len(), 1);
```

**`text()` means something different.** `roxmltree`'s `Node::text()`
returns the first text child. oxml's `Document::text()` returns the
XPath *string-value*: every descendant text node concatenated.

```rust
use oxml::parse;

let doc = parse("<p>a<em>b</em>c</p>").unwrap();
let p = doc.root_element().unwrap();
// roxmltree's text() would give "a"
assert_eq!(doc.text(p), "abc");
```

For the first-text-child behaviour, take the first `Text` child
explicitly.

**`descendants()` is document-wide.** `roxmltree` gives you the
descendants of a node; oxml's iterates the whole document. Filter by
walking up, or use XPath.

## A query, before and after

```rust
use oxml::{XPath, parse};

let doc = parse(r#"<r><a href="one"/><a href="two"/></r>"#).unwrap();

// roxmltree:
//   doc.descendants()
//      .filter(|n| n.has_tag_name("a"))
//      .filter_map(|n| n.attribute("href"))
//      .collect::<Vec<_>>()

let q = XPath::compile("//a/@href").unwrap();
let hrefs: Vec<_> = q.evaluate(&doc).nodes().unwrap()
    .iter().map(|&n| doc.text(n)).collect();
assert_eq!(hrefs, ["one", "two"]);
```
