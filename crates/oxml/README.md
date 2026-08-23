<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

<h1 align="center">oxml</h1>

<p align="center">
  An XML toolkit for Rust — parser, tree, and XPath 1.0 — with zero
  <code>unsafe</code> code.
</p>

<p align="center">
  <a href="https://github.com/sebastienrousseau/oxml/actions"><img src="https://img.shields.io/github/actions/workflow/status/sebastienrousseau/oxml/ci.yml?style=for-the-badge&logo=github" alt="Build" /></a>
  <a href="https://crates.io/crates/oxml"><img src="https://img.shields.io/crates/v/oxml.svg?style=for-the-badge&color=fc8d62&logo=rust" alt="Crates.io" /></a>
  <a href="https://docs.rs/oxml"><img src="https://img.shields.io/badge/docs.rs-oxml-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" alt="Docs.rs" /></a>
  <a href="https://lib.rs/crates/oxml"><img src="https://img.shields.io/badge/lib.rs-oxml-orange.svg?style=for-the-badge" alt="lib.rs" /></a>
  <a href="https://scorecard.dev/viewer/?uri=github.com/sebastienrousseau/oxml"><img src="https://img.shields.io/ossf-scorecard/github.com/sebastienrousseau/oxml?style=for-the-badge&label=OpenSSF%20Scorecard&logo=openssf" alt="OpenSSF Scorecard" /></a>
</p>

---

## Contents

**Getting started**

- [Install](#install) — Cargo, source
- [Quick Start](#quick-start) — parse and query in ten lines

**The oxml ecosystem** (library + five satellite crates)

- [The oxml ecosystem](#the-oxml-ecosystem) — `oxml`, `oxml-cli`, `oxml-lsp`, `oxml-mcp`, `oxml-wasm`, `xmlschema` at a glance

**Library reference**

- [One-minute migration](#one-minute-migration) — name-for-name mapping from `sxd-xpath`, `roxmltree`, `quick-xml`, `libxml`
- [Why this approach?](#why-this-approach) — design rationale
- [Capabilities in 0.0.3](#capabilities-in-003) — release inventory
- [Ecosystem comparison](#ecosystem-comparison) — what each crate does and does not do
- [Benchmarks](#benchmarks) — measured, with the method stated
- [Features](#features) — cargo feature flags

**Behaviour worth knowing**

- [Attributes are nodes](#attributes-are-nodes) — why `@lang` returns what you expect
- [Namespaces resolve by URI](#namespaces-resolve-by-uri) — the element/attribute asymmetry
- [Number formatting](#number-formatting) — why `sum()` prints `17.49`
- [Entity expansion is not supported](#entity-expansion-is-not-supported) — and that is the point

**Practical**

- [Library usage](#library-usage) — the full surface
- [Examples](#examples) — runnable programs
- [When not to use oxml](#when-not-to-use-oxml) — honestly
- [Development](#development) — building, testing, benchmarking
- [Security](#security) — threat model
- [Documentation](#documentation)
- [Acknowledgements](#acknowledgements)
- [License](#license)

---

## Install

```toml
[dependencies]
oxml = "0.0.3"
```

Parsing only, without the XPath engine:

```toml
[dependencies]
oxml = { version = "0.0.3", default-features = false, features = ["std"] }
```

From source:

```bash
git clone https://github.com/sebastienrousseau/oxml
cd oxml
cargo test
```

**Minimum supported Rust version:** 1.86.0. Raising it is a breaking
change and appears in the changelog.

## Quick Start

```rust
use oxml::{parse, XPath};

let doc = parse(r#"
    <library>
        <book lang="en"><title>Dune</title></book>
        <book lang="fr"><title>Germinal</title></book>
    </library>
"#).unwrap();

let titles = XPath::compile("//book[@lang='en']/title").unwrap();
assert_eq!(titles.evaluate(&doc).to_str(&doc), "Dune");
```

XPath is optional. The tree stands on its own:

```rust
use oxml::parse;

let doc = parse("<a><b id='1'>text</b></a>").unwrap();
let root = doc.root_element().unwrap();
let b = doc.children(root)[0];

assert_eq!(doc.attribute(b, "id"), Some("1"));
assert_eq!(doc.text(b), "text");
```

## The oxml ecosystem

Every member ships the **same version number**. If the core is at
`0.0.X` then so is every satellite, so there is never a compatibility
table to consult. Versions advance in `0.0.1` steps along the `0.0.x`
line; `0.1.0` follows `0.0.999`.

| Crate | What it is | Status |
|---|---|---|
| [`oxml`](https://github.com/sebastienrousseau/oxml) | Core library — parser, tree, XPath 1.0 | **Available** |
| [`oxml-cli`](https://github.com/sebastienrousseau/oxml-cli) | Command-line querying and formatting | Planned |
| [`oxml-lsp`](https://github.com/sebastienrousseau/oxml-lsp) | Language server for XML documents | Planned |
| [`oxml-mcp`](https://github.com/sebastienrousseau/oxml-mcp) | Model Context Protocol server | Planned |
| [`oxml-wasm`](https://github.com/sebastienrousseau/oxml-wasm) | WebAssembly bindings | Planned |
| [`xmlschema`](https://github.com/sebastienrousseau/xmlschema) | XSD validation | Planned |

`xmlschema` keeps its existing published name rather than being
repurposed into the core: the name means XSD validation, and that is
what it will be.

## One-minute migration

### From `sxd-xpath`

`sxd-xpath` has not shipped a release since 2018. The concepts map
directly.

| `sxd-xpath` | `oxml` |
|---|---|
| `sxd_document::parser::parse(s)` | `oxml::parse(s)` |
| `sxd_xpath::evaluate_xpath(&doc, "//x")` | `XPath::compile("//x")?.evaluate(&doc)` |
| `Value::Nodeset(set)` | `Value::NodeSet(Vec<NodeId>)` |
| `value.string()` | `value.to_str(&doc)` |
| `value.number()` | `value.to_number(&doc)` |
| `value.boolean()` | `value.to_boolean()` |

The one structural difference: oxml separates *compiling* an expression
from *evaluating* it, so a query used against many documents is parsed
once.

### From `roxmltree`

| `roxmltree` | `oxml` |
|---|---|
| `Document::parse(s)` | `oxml::parse(s)` |
| `doc.root_element()` | `doc.root_element()` |
| `node.children()` | `doc.children(id)` |
| `node.attribute("k")` | `doc.attribute(id, "k")` |
| `node.text()` | `doc.text(id)` |
| — | `XPath::compile(..)` |

`roxmltree` returns node *objects* that borrow the document; oxml
returns `NodeId` handles and takes the document as a parameter. That
trade is what allows a node to know its parent without a lifetime
cycle.

### From `quick-xml`

`quick-xml` is a streaming reader and writer, not a tree. If you are
matching on `Event::Start` and maintaining your own stack to find
elements, that is the loop oxml replaces:

```rust
let xml = "<list><item id='7'>found</item><item id='8'/></list>";

// quick-xml: match on Event::Start, maintain your own stack, track
// depth, and remember which element you are inside.
//
// oxml: say what you want.
let doc = oxml::parse(xml).unwrap();
let hits = oxml::XPath::compile("//item[@id='7']").unwrap().evaluate(&doc);

assert_eq!(hits.to_str(&doc), "found");
```

Keep `quick-xml` for gigabyte streams you never want fully in memory.

### From `libxml`

`libxml` binds libxml2 through C-FFI. Migrating to oxml removes the C
toolchain, the `unsafe` blocks, and the libxml2 CVE stream — at the
cost of XSLT, which oxml does not yet have.

## Why this approach?

Rust's XML ecosystem is strong at one end and empty at the other.
Parsing is a solved problem: `quick-xml` has 379 million downloads and
is genuinely fast. What nothing maintained provides is the other half
of what `lxml` gives Python — **querying**.

The only XPath implementation on crates.io, `sxd-xpath`, last shipped
in **2018**. XSLT and XSD validation have no pure-Rust implementation
at all. So a Rust project that needs to *ask questions of* an XML
document has three options: bind libxml2 through C-FFI, depend on a
crate abandoned seven years ago, or hand-roll traversal.

oxml closes the query gap first, because that is the one people hit.

Two architectural choices motivate the design:

1. **An arena, not a pointer graph.** Nodes live in a `Vec` and are
   addressed by index. That is what lets every node know its parent
   without `Rc`, `RefCell`, or `unsafe` — the parent link is an index,
   so there is no ownership cycle for the borrow checker to reject.
   The cost is that a `NodeId` is only meaningful against the document
   that issued it; accessors return `None` rather than panicking when
   it is not.

2. **`#![forbid(unsafe_code)]`, enforced twice.** The attribute fails
   the build, and CI additionally greps for it — because the change
   that silently drops the guarantee is *deleting the attribute*, and
   a compile-time check cannot catch its own removal. Most XML crates
   with XPath support reach it through C-FFI; those `unsafe` blocks
   are usually well-vetted, but their existence makes a
   security-conscious downstream audit meaningfully harder.

## Capabilities in 0.0.3

**Parsing**

- Elements, attributes, text, comments, processing instructions, CDATA
- XML declaration and `DOCTYPE` skipping, including bracketed internal
  subsets containing `>`
- Namespace resolution by URI, with the `xml:` prefix bound implicitly
- The five predefined entities and numeric character references
- Adjacent character data merged, so a caller never sees two text
  siblings in a row

**Tree**

- Arena-backed, index-addressed nodes
- Parent, children, descendants, text (XPath `string-value` semantics)
- Attributes as first-class nodes

**XPath 1.0**

- Ten axes: `child`, `descendant`, `descendant-or-self`, `parent`,
  `ancestor`, `ancestor-or-self`, `self`, `attribute`,
  `following-sibling`, `preceding-sibling`
- Abbreviations: `//`, `.`, `..`, `@`
- Node tests: name, `*`, `text()`, `comment()`, `node()`
- Predicates, including positional (`[1]`) and existential comparison
- All four value types with the specified conversions
- 25 functions: `count`, `sum`, `position`, `last`, `string`, `number`,
  `boolean`, `not`, `true`, `false`, `concat`, `contains`,
  `starts-with`, `substring`, `string-length`, `normalize-space`,
  `local-name`, `namespace-uri`, `floor`, `ceiling`, `round`, and
  arithmetic, comparison, boolean and union operators

**Not yet:** serialisation, XSD validation, XSLT, XPath 2.0+.

## Ecosystem comparison

| Crate | Downloads | Last release | Parse | Tree | XPath | XSLT | XSD | `unsafe` |
|---|---|---|---|---|---|---|---|---|
| **`oxml`** | — | active | ✅ | ✅ | ✅ | ✗ | ✗ | **none** |
| `quick-xml` | 379M | active | ✅ | ✗ | ✗ | ✗ | ✗ | some |
| `xml-rs` | 137M | active | ✅ | ✗ | ✗ | ✗ | ✗ | none |
| `roxmltree` | 66M | active | ✅ | ✅ | ✗ | ✗ | ✗ | none |
| `xmltree` | 19M | active | ✅ | ✅ | ✗ | ✗ | ✗ | none |
| `sxd-xpath` | 2M | **2018** | ✅ | ✅ | ✅ | ✗ | ✗ | none |
| `libxml` | 2M | active | ✅ | ✅ | ✅ | ✅ | ✅ | C-FFI |
| `xot` | 218K | 2025 | ✅ | ✅ | ✗ | ✗ | ✗ | none |

Download figures from crates.io, August 2026.

## Benchmarks

Run them yourself — the numbers below are from one machine and are
useful as ratios, not absolutes:

```bash
cargo bench
```

| Benchmark | Time | What it measures |
|---|---|---|
| `parse/wide_1000` | 489 µs | 1,000 sibling elements — sibling handling |
| `parse/deep_500` | 109 µs | 500 nesting levels — recursion |
| `parse/attributes_1000` | 823 µs | 4 attributes each, namespaced — resolution |
| `xpath/compile` | 889 ns | Compiling `//book[@lang='en']/title` |
| `xpath/eval_descendant` | 245 µs | `//title` over 2,000 books |
| `xpath/eval_predicate` | 1.19 ms | `//book[@lang='en']/title` over 2,000 books |

Compilation and evaluation are timed separately on purpose: a caller
that compiles once and evaluates many times pays only the second, and
reporting a combined figure would hide that.

**The benchmarks have already earned their place.** The first
implementation deduplicated each step's results with a linear
`contains` scan, which is O(n²). `//title` over 2,000 elements took
**10.8 ms**; sorting instead brought it to **0.49 ms** — a 22×
improvement with identical results. The benchmark was written before
the optimisation, not after it.

## Features

| Flag | Default | What it does |
|---|---|---|
| `std` | ✅ | Standard library integration, including `std::error::Error`. |
| `xpath` | ✅ | The XPath 1.0 engine. Disable if you only need to parse. |

Both are additive. With neither, the crate is `no_std` and provides the
parser and tree over `alloc`.

## Attributes are nodes

XPath models attributes as nodes that are reachable from the
`attribute::` axis but are *not* children of their element. oxml does
the same: attribute nodes live in the arena, know their parent, and are
absent from `children()`.

```rust
use oxml::{parse, XPath};

let doc = parse(r#"<book lang="en"><title>Dune</title></book>"#).unwrap();

// The attribute's value, not the element's text.
let lang = XPath::compile("//book/@lang").unwrap();
assert_eq!(lang.evaluate(&doc).to_str(&doc), "en");

// And `child::` does not see it.
let root = doc.root_element().unwrap();
assert_eq!(doc.children(root).len(), 1); // just <title>
```

The first implementation returned the *owning element* from the
attribute axis, which made `string(//book/@lang)` evaluate to the
book's text. It was wrong, and silently so — which is why the test that
pins this behaviour names the bug it prevents.

## Namespaces resolve by URI

Two prefixes bound to the same URI name the same thing:

```rust
use oxml::parse;

let doc = parse(r#"<r xmlns:a="urn:x" xmlns:b="urn:x"><a:e/><b:e/></r>"#).unwrap();
let root = doc.root_element().unwrap();
let kids = doc.children(root);

assert_eq!(doc.element_name(kids[0]), doc.element_name(kids[1]));
```

An unprefixed **element** takes the default namespace. An unprefixed
**attribute** is in no namespace at all — not its element's. That
asymmetry is the classic source of namespace bugs, so it is an explicit
parameter in the parser rather than an assumption.

## Number formatting

XPath has one numeric type: IEEE 754 double. `sum()` over `9.99` and
`7.50` produces the float nearest `17.490000000000002`, and printing
every digit needed to distinguish that value is what the
specification's wording literally asks for.

No other engine does that. libxml2, Xalan and Saxon all print `17.49`,
because 15 significant digits is the point past which IEEE 754 noise
starts showing. oxml matches them:

```rust
use oxml::{parse, XPath};

let doc = parse("<r/>").unwrap();
let sum = XPath::compile("9.99 + 7.50").unwrap();
assert_eq!(sum.evaluate(&doc).to_str(&doc), "17.49");
```

Matching the ecosystem matters more here than matching the letter of a
sentence written before shortest-round-trip float printing existed.

## Entity expansion is bounded, and external entities are never fetched

Internal general entities are expanded — a conforming parser must, and a
great many valid documents depend on it. **External entities are never
dereferenced**, which forecloses XXE by construction rather than by
configuration:

```rust
let src = r#"<!DOCTYPE a [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><a>&xxe;</a>"#;
let doc = oxml::parse(src).expect("declared, so not an error");
// No file is opened. The entity expands to nothing.
assert_eq!(doc.text(doc.root()).trim(), "");
```

There is no flag to turn external fetching on, so there is no way to
configure the vulnerability back in — the difference between a parser
that is safe and one that is safe *if you remember to set the option*.

Expansion is bounded on **two** axes, because either alone is
insufficient:

```rust
use oxml::{ErrorKind, parse};

// Billion laughs: exponential nesting. Caught by the depth cap.
let mut src = String::from(r#"<!DOCTYPE lolz [<!ENTITY lol "lol">"#);
for i in 1..=9 {
    let prev = if i == 1 { "lol".to_owned() } else { format!("lol{}", i - 1) };
    src.push_str(&format!(r#"<!ENTITY lol{i} "{}">"#, format!("&{prev};").repeat(10)));
}
src.push_str("]><lolz>&lol9;</lolz>");
assert!(matches!(
    parse(&src).unwrap_err().kind,
    ErrorKind::EntityLimitExceeded
));
```

The second axis is a **per-document** character budget, which the depth
cap cannot substitute for. Referencing one 100 KB entity a thousand times
never exceeds depth 1, and an earlier per-*reference* budget let exactly
that through — 100 MB of text from 100 KB of input. `Limits` exposes
both `max_entity_depth` and `max_entity_expansion`.

## Library usage

```rust
use oxml::{parse, NodeKind, XPath};

let doc = parse("<r><a id='1'>x</a><!--c--></r>").unwrap();

// Navigation
let root = doc.root_element().unwrap();
let first = doc.children(root)[0];
assert_eq!(doc.parent(first), Some(root));
assert_eq!(doc.element_name(first).unwrap().local, "a");

// Attributes
assert_eq!(doc.attribute(first, "id"), Some("1"));
for attr in doc.attributes(first) {
    assert_eq!(attr.name.local, "id");
}

// Node kinds
assert!(matches!(doc.kind(first), Some(NodeKind::Element { .. })));

// Text, with XPath string-value semantics: comments contribute nothing
assert_eq!(doc.text(root), "x");

// Queries, compiled once
let q = XPath::compile("count(//a)").unwrap();
assert_eq!(q.evaluate(&doc).to_str(&doc), "1");
```

## Examples

```bash
cargo run --example parse_and_query
```

| Example | What it shows |
|---|---|
| [`parse_and_query`](crates/oxml/examples/parse_and_query.rs) | XPath queries and direct tree traversal side by side |

## When not to use oxml

Honestly:

- **You need XSLT.** oxml does not have it. Use `libxml`.
- **You need XSD validation today.** `xmlschema` is planned, not
  shipped.
- **You are streaming gigabytes.** oxml builds a full tree in memory.
  `quick-xml` is the right tool and will stay so.
- **You need XPath 2.0 or 3.1.** oxml implements 1.0.
- **Raw parse throughput is your only metric.** `quick-xml` is
  extremely well optimised and years ahead on that axis. oxml's
  argument is what you can do *after* parsing.

## Development

```bash
cargo test                  # 42 tests
cargo test --no-default-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo bench
cargo doc --no-deps --open
```

CI runs all of the above on Linux, macOS and Windows, plus an MSRV
build and a check that `#![forbid(unsafe_code)]` is still present.

## Security

See [SECURITY.md](SECURITY.md) for reporting and the full threat model.

In summary: entity expansion is not implemented, so XXE and
billion-laughs are foreclosed; `#![forbid(unsafe_code)]` rules out
memory-corruption bugs; and deeply nested documents are parsed
recursively, so untrusted input of unbounded depth should be parsed on
a thread with a known stack size.

## Documentation

- [API documentation](https://docs.rs/oxml)
- [CHANGELOG.md](CHANGELOG.md)
- [CONTRIBUTING.md](CONTRIBUTING.md)
- [GOVERNANCE.md](GOVERNANCE.md)
- [SECURITY.md](SECURITY.md)

## Acknowledgements

oxml exists because of work that came before it:

- **[lxml](https://lxml.de/)** — the reference for what an XML toolkit
  should offer.
- **[libxml2](https://gitlab.gnome.org/GNOME/libxml2)** — decades of
  hard-won correctness, and the yardstick for behaviour.
- **[`sxd-xpath`](https://github.com/shepmaster/sxd-xpath)** — the
  first XPath implementation in Rust.
- **[`quick-xml`](https://github.com/tafia/quick-xml)** and
  **[`roxmltree`](https://github.com/RazrFalcon/roxmltree)** — proof
  that Rust XML parsing can be both fast and safe.
- **[W3C](https://www.w3.org/TR/1999/REC-xpath-19991116/)** — for the
  XPath 1.0 specification, which this implementation follows.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you shall be dual licensed as
above, without any additional terms or conditions.

## FAQ

### Is it faster than quick-xml or roxmltree?

Not yet, and the README will say so until it is. On a 16.86 MB document,
measured on one machine: quick-xml 704 MB/s building no tree,
roxmltree 291 MB/s building a borrowed tree, `oxml` 123 MB/s. The gap is
allocation — `oxml` performs about two per node where a borrowed design
performs almost none — and closing it is in progress.

Any figure here states the machine and the method. During development
the same binary measured 14.7 and 123.1 MB/s on a loaded host, which is
why a number without its conditions is not a measurement.

### How conformant is it?

93.6% of decided tests in the W3C XML Conformance Test Suite, release
`xmlts20130923`, with 98.9% coverage of the 2,585 tests and zero panics.
Both numbers are published because either alone misleads: a pass rate
can be raised by skipping the hard tests.

Of the failures, most require the **external** DTD subset, which `oxml`
never fetches — see below.

### Why does it not fetch external entities?

Because that is the XXE vulnerability, and a parser that cannot fetch
cannot be made to leak a file. There is no option to enable it, which is
the difference between safe and *safe if you remember to set the flag*.

The cost is real and is stated rather than hidden: an error that lives
inside an external entity cannot be detected, and those account for most
of the remaining conformance failures. The specification does not
require a non-validating parser to read the external subset.

### Does it expand entities at all?

Internal ones, yes — a conforming parser must, and many valid documents
depend on it. Expansion is bounded on two axes, because either alone is
insufficient: a depth cap stops the exponential "billion laughs" shape,
and a **per-document** character budget stops the quadratic one. During
development a per-*reference* budget let a single 100 KB entity
referenced a thousand times produce 100 MB of text; both shapes are now
rejected and both have tests.

### Does it validate?

No. `oxml` checks well-formedness, including the constraints that live
inside the DTD. Validation against a schema is
[`xmlschema`](https://crates.io/crates/xmlschema), which covers XSD.

### Which XML version and edition?

XML 1.0 and 1.1, with namespaces. XML 1.0 has two incompatible editions
— the 5th relaxed the name rules and the two genuinely disagree — so
both are implemented and `Limits::edition` selects. The 5th is the
default.

### Does it really work without `std`?

Yes, and CI builds for `thumbv7em-none-eabihf`,
`riscv32imac-unknown-none-elf` and `aarch64-unknown-none` to prove it.
`cargo check --no-default-features` on a host proves nothing, because
the host still has `std` — which is exactly how `no_std` support for
`XPath` regressed unnoticed once.

`XPath` additionally needs the `libm` feature, since `floor`, `ceil` and
`trunc` live in `std` rather than `core`. Requesting `xpath` without it
fails with one clear message rather than a dozen missing-method errors.

### Is `forbid(unsafe_code)` just marketing?

It is CI-enforced, and it prevents memory-unsafety. It does **not**
prevent a parser exhausting memory or spending ten minutes in a
quadratic loop — two HIGH-severity 2026 advisories against another Rust
XML crate were exactly that, in entirely safe code. `Limits` exists for
that class, and the fuzz targets exist to find it.

### Why is my document rejected when another parser accepts it?

Most often one of: a `--` inside a comment, a literal `]]>` in content,
a missing space between attributes, `<` in an attribute value, or a
control character. All are forbidden by XML 1.0 and all are accepted by
some parsers. The error names the constraint and the byte offset.

### When should I not use it?

If you need XSLT, XQuery, or XPath 2.0+, none of which exist here — see
[Xee](https://github.com/Paligo/xee) for XPath 3.1. If you need to
resolve external entities. If you are parsing multi-gigabyte documents
where a DOM will not fit; a streaming API is planned and this is not it
yet.

