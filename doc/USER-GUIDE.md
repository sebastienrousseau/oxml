<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# User guide

The long form. The README is the tour; this is the manual.

## Contents

- [Installing](#installing)
- [Parsing](#parsing)
- [The tree](#the-tree)
- [Text](#text)
- [Attributes](#attributes)
- [Namespaces](#namespaces)
- [XPath](#xpath)
- [Errors](#errors)
- [Limits](#limits)
- [Encodings](#encodings)
- [`no_std`](#no_std)
- [Threads](#threads)
- [Recipes](#recipes)

## Installing

```toml
[dependencies]
oxml = "0.0.4"
```

Without XPath, or without `std`:

```toml
oxml = { version = "0.0.4", default-features = false, features = ["std"] }
oxml = { version = "0.0.4", default-features = false, features = ["xpath", "libm"] }
```

`xpath` without `std` needs `libm`, because XPath requires `floor`,
`ceil` and `trunc` and `core` does not provide them. Asking for one
without the other is a compile error that says so, rather than a link
failure.

## Parsing

Four entry points:

| Function | Input | Limits |
|---|---|---|
| `parse` | `&str` | Default |
| `parse_with` | `&str` | Yours |
| `parse_bytes` | `&[u8]` | Default |
| `parse_bytes_with` | `&[u8]` | Yours |

`&str` means you have decided the encoding. `&[u8]` means the document
decides, from its byte-order mark and declaration. For a file or an
HTTP body, you want the byte form.

All four return `Result<Document>`. There is no recovery mode: a
document that is not well-formed is rejected with the offset of the
problem. A parser that guesses at what a malformed document meant
produces a tree no two implementations agree on.

## The tree

A `Document` is an arena. Every node is a `NodeId` — a `Copy`,
pointer-sized index — and every accessor takes one.

```rust
use oxml::parse;

let doc = parse("<r><a/><b/></r>").unwrap();

let root = doc.root();                       // the document node
let element = doc.root_element().unwrap();   // <r>
assert_ne!(root, element);
```

The document node is not the root element. Comments and processing
instructions before `<r>` are children of the former.

| Method | Returns |
|---|---|
| `root()` | The document node |
| `root_element()` | The outermost element, if any |
| `kind(id)` | `Root`, `Element`, `Attr`, `Text`, `Comment`, `ProcessingInstruction` |
| `parent(id)` | The parent, or `None` for the document node |
| `children(id)` | A slice of child ids |
| `descendants()` | Every node, in document order |
| `is_element(id)` | Whether it is an element |
| `element_name(id)` | Its `ExpandedName`, if it is one |
| `len()` / `is_empty()` | Node count |

Because ids are indices, you can collect them freely:

```rust
use oxml::parse;

let doc = parse("<r><a/><b/><a/></r>").unwrap();
let a_elements: Vec<_> = doc
    .descendants()
    .filter(|&id| doc.element_name(id).is_some_and(|n| n.local == "a"))
    .collect();
assert_eq!(a_elements.len(), 2);
```

The cost: a `NodeId` from one document is a valid index into another and
means something else. Nothing checks this.

## Text

`text(id)` returns the XPath **string-value**: every descendant text
node concatenated, with comments and processing instructions
contributing nothing.

```rust
use oxml::parse;

let doc = parse("<p>The <em>first</em> finding.<!--note--></p>").unwrap();
let p = doc.root_element().unwrap();
assert_eq!(doc.text(p), "The first finding.");
```

Markup inside a paragraph disappears and the sentence survives, which
is almost always what you want. If you need the structure, walk the
children.

For an attribute node, the string-value is the attribute's value.

## Attributes

```rust
use oxml::parse;

let doc = parse(r#"<order id="A-1" note="two &amp; a half"/>"#).unwrap();
let order = doc.root_element().unwrap();

assert_eq!(doc.attribute(order, "id"), Some("A-1"));
// Entities are already resolved.
assert_eq!(doc.attribute(order, "note"), Some("two & a half"));
assert_eq!(doc.attribute(order, "absent"), None);

for attr in doc.attributes(order) {
    let _ = (&attr.name.local, &attr.value);
}
```

`attribute` matches on the **local name only**, which is ambiguous when
two namespaces use the same one. When that matters, compare the
expanded name:

```rust
use oxml::{ExpandedName, parse};

let doc = parse(r#"<a xmlns:x="urn:x" x:ref="R" ref="plain"/>"#).unwrap();
let a = doc.root_element().unwrap();
let wanted = ExpandedName::qualified("urn:x", "ref");
let found = doc.attributes(a).into_iter().find(|at| at.name == wanted);
assert_eq!(found.map(|at| at.value.as_str()), Some("R"));
```

Attributes are also nodes — `attribute_nodes(id)` gives their ids, which
is what the XPath `attribute` axis returns.

## Namespaces

`ExpandedName` holds the namespace **URI**, never the prefix. Two
documents using different prefixes for the same URI produce equal
names, which is what the specification requires:

```rust
use oxml::parse;

let one = parse(r#"<a:x xmlns:a="urn:u"/>"#).unwrap();
let two = parse(r#"<b:x xmlns:b="urn:u"/>"#).unwrap();
assert_eq!(
    one.element_name(one.root_element().unwrap()),
    two.element_name(two.root_element().unwrap()),
);
```

An undeclared prefix is an error, not a name containing a colon. The
`xml:` prefix is bound implicitly.

Unprefixed **attributes** are in no namespace even when a default
namespace is declared — an asymmetry with elements that comes from the
specification, not from this implementation.

## XPath

Compile once; evaluate many times.

```rust
use oxml::{XPath, parse};

let doc = parse("<shop><item price='9.99'/><item price='4.50'/></shop>").unwrap();
let total = XPath::compile("sum(//item/@price)").unwrap();
assert_eq!(total.evaluate(&doc).to_str(&doc), "14.49");
```

A compiled `XPath` is document-independent and `Send + Sync`, so a
server compiles at startup and evaluates per request across a thread
pool.

Four value types, with XPath's own conversions:

```rust
use oxml::{XPath, parse, xpath::Value};

let doc = parse("<r><a/><a/></r>").unwrap();
let nodes = XPath::compile("//a").unwrap().evaluate(&doc);
assert!(matches!(nodes, Value::NodeSet(_)));
assert_eq!(nodes.nodes().unwrap().len(), 2);
assert!(nodes.to_boolean());              // non-empty
```

A node-set's string-value is that of its **first** node in document
order. Converting a non-numeric string gives `NaN`, not an error —
XPath has no exceptions, so every conversion produces something.

`evaluate_from` runs a relative expression against a context node,
which is how you nest queries:

```rust
use oxml::{XPath, parse};

let doc = parse("<shop><item p='1'>Tea</item><item p='2'>Cocoa</item></shop>").unwrap();
let rows = XPath::compile("//item").unwrap();
let price = XPath::compile("@p").unwrap();
let mut total = 0.0;
for &node in rows.evaluate(&doc).nodes().unwrap() {
    total += price.evaluate_from(&doc, node).to_number(&doc);
}
assert_eq!(total, 3.0);
```

## Errors

An `Error` carries a byte offset and an `ErrorKind`, not a formatted
string:

```rust
use oxml::{ErrorKind, parse};

let error = parse("<a></b>").unwrap_err();
assert!(matches!(error.kind, ErrorKind::MismatchedEndTag { .. }));
let (line, column) = error.line_column("<a></b>");
assert_eq!((line, column), (1, 4));
```

`line_column` counts characters rather than bytes, so the column is the
one an editor shows. It never panics — not on an offset past the end,
not on one inside a multi-byte character, and not when given a
different document from the one that produced the error.

`ErrorKind` implements `Display` separately from `Error`, so a caret
display can print the message without repeating the offset.

## Limits

See [SECURITY-MODEL.md](SECURITY-MODEL.md) for what each bound defends
against. In short:

```rust
use oxml::{Limits, parse_with};

// Anything from a network.
assert!(parse_with("<a/>", Limits::strict()).is_ok());

// Or one field at a time. `Limits` is `#[non_exhaustive]`, so start
// from a profile rather than writing a struct literal.
let mut limits = Limits::default();
limits.max_depth = 32;
assert!(parse_with("<a><b/></a>", limits).is_ok());
```

## Encodings

```rust
use oxml::parse_bytes;

let doc = parse_bytes(b"<?xml version='1.0'?><a>hi</a>").unwrap();
assert_eq!(doc.text(doc.root()), "hi");
```

UTF-8, UTF-16 in both byte orders with or without a BOM, and
ISO-8859-1. A BOM overrides a declaration that disagrees with it. UTF-8
is borrowed rather than copied.

`MalformedEncoding` and `UnsupportedEncoding` are different failures:
the first is a malformed document that every parser must reject, the
second is a legal document in an encoding this crate lacks — decode it
yourself and call `parse`.

## `no_std`

```toml
oxml = { version = "0.0.4", default-features = false, features = ["xpath", "libm"] }
```

Needs `alloc`. CI builds for `thumbv7em-none-eabihf`,
`riscv32imac-unknown-none-elf` and `aarch64-unknown-none` on every
push, so this is verified rather than claimed.

Without `std` you lose `std::error::Error` for `Error`; everything else
is present.

## Threads

`Document` and `XPath` are both `Send + Sync`. A document is immutable
after parsing, so any number of threads can query one:

```rust
use oxml::parse;

let doc = parse("<r><a id='1'/><a id='2'/></r>").unwrap();
let root = doc.root_element().unwrap();
std::thread::scope(|scope| {
    let handles: Vec<_> = (0..4)
        .map(|_| scope.spawn(|| doc.children(root).len()))
        .collect();
    for handle in handles {
        assert_eq!(handle.join().unwrap(), 2);
    }
});
```

## Recipes

### Every value of an attribute

```rust
use oxml::{XPath, parse};

let doc = parse(r#"<r><a href="one"/><a href="two"/></r>"#).unwrap();
let q = XPath::compile("//@href").unwrap();
let values: Vec<_> = q
    .evaluate(&doc)
    .nodes()
    .unwrap()
    .iter()
    .map(|&n| doc.text(n))
    .collect();
assert_eq!(values, ["one", "two"]);
```

### Find one element by name, without XPath

```rust
use oxml::parse;

let doc = parse("<r><meta/><body>text</body></r>").unwrap();
let body = doc
    .descendants()
    .find(|&id| doc.element_name(id).is_some_and(|n| n.local == "body"));
assert_eq!(body.map(|id| doc.text(id)), Some("text".to_string()));
```

### Check well-formedness and report where it failed

```rust
use oxml::parse;

fn check(input: &str) -> Result<(), String> {
    match parse(input) {
        Ok(_) => Ok(()),
        Err(e) => {
            let (line, column) = e.line_column(input);
            Err(format!("{line}:{column}: {}", e.kind))
        }
    }
}

assert_eq!(check("<a/>"), Ok(()));
assert!(check("<a>").is_err());
```
