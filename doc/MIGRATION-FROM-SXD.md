<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Migrating from `sxd-document` / `sxd-xpath`

The nearest functional equivalent: a tree plus XPath 1.0. The APIs
differ more than the capabilities do.

## What you gain

- **`Send + Sync` documents.** `sxd-document` uses an arena with
  interior mutability, so a `Package` is not `Send`. oxml's tree is
  immutable after parsing, so one document serves a thread pool and one
  compiled expression can be evaluated from many threads at once.
- **`#![forbid(unsafe_code)]`** and `no_std` support.
- **Configurable limits**, and external entities that are never
  fetched.
- **A published conformance number** with its denominator.

## What you give up

- **Mutation.** `sxd-document` can build and modify a tree; oxml
  cannot. If you construct XML, stay.
- **Variables and custom functions** in XPath, if you use them.

## Name for name

| `sxd-*` | oxml |
|---|---|
| `sxd_document::parser::parse(s)` | `oxml::parse(s)` |
| `package.as_document()` | the `Document` itself |
| `doc.root()` | `doc.root()` |
| `sxd_xpath::evaluate_xpath(&doc, "…")` | `XPath::compile("…")?.evaluate(&doc)` |
| `Value::Nodeset` | `Value::NodeSet` |
| `value.string()` | `value.to_str(&doc)` |
| `value.number()` | `value.to_number(&doc)` |
| `value.boolean()` | `value.to_boolean()` |
| `Factory::new().build(…)` | `XPath::compile(…)` |

## Compile once

`evaluate_xpath` parses the expression every call. oxml separates the
two, which matters when the same query runs repeatedly:

```rust
use oxml::{XPath, parse};

let doc = parse("<r><a/><a/></r>").unwrap();

// sxd: evaluate_xpath(&doc, "count(//a)")  -- parses every time
let q = XPath::compile("count(//a)").unwrap();   // once
assert_eq!(q.evaluate(&doc).to_str(&doc), "2");  // many
```

A compiled `XPath` is document-independent, so it can be built at
startup and shared.

## Conversions need the document

`sxd-xpath`'s `Value::string()` takes no arguments. oxml's `to_str`
takes the document, because a node-set's string-value is the
string-value of a node, and nodes are indices:

```rust
use oxml::{XPath, parse};

let doc = parse("<r><a>text</a></r>").unwrap();
let v = XPath::compile("//a").unwrap().evaluate(&doc);
assert_eq!(v.to_str(&doc), "text");
assert!(v.to_boolean());          // no document needed
```

`to_boolean` is the exception: a node-set's truth is its emptiness,
which the value already knows.

## Namespaces

Both resolve namespaces by URI **in the tree**. They differ in
expressions, and the difference can give you a wrong answer quietly.

`sxd-xpath` requires you to register prefixes on a `Context`, and an
unregistered prefix is an error. oxml does not resolve prefixes in a
name test at all: `//x:item` matches on the local part alone, so it
selects every `item` regardless of namespace.

```rust
use oxml::{XPath, parse};

let doc = parse(r#"<r xmlns:x="urn:u"><x:item>A</x:item><item>B</item></r>"#).unwrap();

// Both of these select BOTH elements. The prefix is ignored.
for expr in ["//x:item", "//item"] {
    let v = XPath::compile(expr).unwrap().evaluate(&doc);
    assert_eq!(v.nodes().unwrap().len(), 2, "{expr}");
}

// To select by namespace, test it explicitly.
let ns = XPath::compile("//*[namespace-uri()='urn:u']").unwrap();
assert_eq!(ns.evaluate(&doc).nodes().unwrap().len(), 1);
```

Until this is fixed, filter with `namespace-uri()` rather than writing
a prefix, because a prefixed name test does not mean what it looks
like.
