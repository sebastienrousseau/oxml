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

Both resolve namespaces by URI, and both resolve a prefix in an
expression against bindings you supply rather than against the
document.

`sxd-xpath` registers prefixes on a `Context`. oxml takes them at
compile time:

```rust
use oxml::{XPath, parse};

let doc = parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a><a>no</a></r>"#).unwrap();

// sxd: Context::new().set_namespace("m", "urn:u")
let q = XPath::compile_with_namespaces("//m:a", &[("m", "urn:u")]).unwrap();
assert_eq!(q.evaluate(&doc).to_str(&doc), "yes");
```

An unbound prefix is a compile error in both. An unprefixed name test
matches only nodes in no namespace, in both -- that is XPath 1.0, not
a choice either made.

Before 0.0.4 oxml matched a prefixed name on its local part alone, so
`//m:a` selected every `a` regardless of namespace. If you are porting
from a version that old, expressions that appeared to work may have
been returning more than you thought.
