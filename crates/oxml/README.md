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
  <a href="https://www.bestpractices.dev/projects/14278"><img src="https://img.shields.io/cii/level/14278?style=for-the-badge&label=OpenSSF%20Best%20Practices&logo=openssf" alt="OpenSSF Best Practices" /></a>
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
- [Capabilities in 0.0.8](#capabilities-in-008) — release inventory
- [Ecosystem comparison](#ecosystem-comparison) — what each crate does and does not do
- [Benchmarks](#benchmarks) — measured, with the method stated
- [Features](#features) — cargo feature flags

**Behaviour worth knowing**

- [Attributes are nodes](#attributes-are-nodes) — why `@lang` returns what you expect
- [Namespaces resolve by URI](#namespaces-resolve-by-uri) — the element/attribute asymmetry
- [XPath name tests resolve namespaces](#xpath-name-tests-resolve-namespaces) — and unprefixed names match no namespace
- [Number formatting](#number-formatting) — why `sum()` prints `17.49`
- [Entity expansion is bounded, and external entities are never
  fetched](#entity-expansion-is-bounded-and-external-entities-are-never-fetched)
  — expansion happens, within a budget

**Practical**

- [Library usage](#library-usage) — the full surface
- [Reading without a tree](#reading-without-a-tree) — events, and what
  streaming does and does not buy you
- [Configuration](#configuration) — limits, profiles, cargo features
- [Error reporting](#error-reporting) — offsets, line/column, carets
- [External entities and subsets](#external-entities-and-subsets) —
  supplied by the caller, never fetched
- [Encodings](#encodings) — UTF-8, UTF-16, ISO-8859-1
- [Examples](#examples) — runnable programs
- [When not to use oxml](#when-not-to-use-oxml) — honestly
- [FAQ](#faq) — twenty-one questions, answered directly
- [Development](#development) — building, testing, benchmarking
- [Security](#security) — threat model
- [Documentation](#documentation)
- [Acknowledgements](#acknowledgements)
- [License](#license)

---

## Install

```toml
[dependencies]
oxml = "0.0.8"
```

Parsing only, without the XPath engine:

```toml
[dependencies]
oxml = { version = "0.0.8", default-features = false, features = ["std"] }
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

## Capabilities in 0.0.8

**Parsing**

- Elements, attributes, text, comments, processing instructions, CDATA
- XML declaration, and a full internal-subset `DOCTYPE` parser:
  `<!ELEMENT>`, `<!ATTLIST>`, `<!ENTITY>`, `<!NOTATION>`, conditional
  sections, parameter-entity references and content models
- Namespace resolution by URI, with the `xml:` prefix bound implicitly
- Internal general entities, the five predefined entities, and numeric
  character references
- XML 1.0 fourth and fifth edition name rules, selectable per parse
- XML 1.1, including the `RestrictedChar` production
- UTF-8, UTF-16 (both byte orders, with or without a BOM) and
  ISO-8859-1, chosen by BOM or declaration
- Adjacent character data merged, so a caller never sees two text
  siblings in a row
- A pull reader over the same scanner, for callers that want events
  rather than a tree

**Tree**

- Arena-backed, index-addressed nodes; `NodeId` is `Copy` and
  pointer-sized
- Child and attribute lists as `(start, len)` ranges into shared
  vectors; element **and attribute** names interned behind a `NameId`,
  which also records the prefix each was written with
- Parent, children, descendants, text (XPath `string-value` semantics)
- Attributes and namespace declarations as first-class nodes
- `Send + Sync`, so one document serves any number of threads

**Serialisation**

- `Document::to_xml()` returns a document as XML;
  `write_xml<W: Write>()` writes it to any writer
- Escaping is applied where it changes meaning: `&`, `<` and `>` in
  text, plus `"`, tab, newline and carriage return in attribute
  values, where an unescaped newline would normalise to a space on the
  way back in
- `<?xml version="1.1"?>` is emitted only when the document needs it --
  a control character, or a prefix unbound with `xmlns:p=""`
- Every document in the conformance corpus is a fixed point: parse,
  serialise, parse again, and the trees agree

**Streaming**

- `stream::Reader` yields events without building a tree, over the
  same scanner the tree parser uses, so the two agree by construction
- `Reader::from_reader` reads from any `BufRead`, so a document larger
  than memory is readable; memory stays bounded regardless of size
- Characters and `\r\n` pairs split across a read are held back until
  whole, so the buffer size cannot change what a document means

**XPath 1.0**

- All thirteen axes: `child`, `descendant`, `descendant-or-self`,
  `parent`, `ancestor`, `ancestor-or-self`, `self`, `attribute`,
  `namespace`, `following`, `following-sibling`, `preceding`,
  `preceding-sibling`
- Abbreviations: `//`, `.`, `..`, `@`
- Node tests: name, `*`, `text()`, `comment()`, `node()`
- Predicates, including positional (`[1]`) and existential comparison
- All four value types with the specified conversions
- All 27 functions: `last`, `position`, `count`, `id`, `local-name`,
  `namespace-uri`, `name`, `string`, `concat`, `starts-with`,
  `contains`, `substring-before`, `substring-after`, `substring`,
  `string-length`, `normalize-space`, `translate`, `boolean`, `not`,
  `true`, `false`, `lang`, `number`, `sum`, `floor`, `ceiling`, `round`
- Arithmetic, comparison, boolean and union operators
- An unknown function name, or the wrong number of arguments, is a
  compile error rather than a plausible-looking empty result
- Compiled once, evaluated many times; `Send + Sync`

**Safety and limits**

- Ten configurable bounds, in three profiles
- External entities never dereferenced
- Entity expansion bounded per document, not per reference
- Recursion bounded, so no input reaches a stack overflow

**Verification**

- **All 2,557** decided W3C conformance tests pass, with
  98.9% of the 2,585-test suite reaching a decision and **zero panics**
- 466 tests and 27 doctests; 96.8% line coverage, gated in CI
- Seven fuzz targets, Miri, property tests, and a feature powerset build

**Not yet:** serialisation, mutation, XSD validation and XSLT. The
external DTD subset is supported when the
caller supplies it — see [External entities and
subsets](#external-entities-and-subsets) — but is never fetched, and
most remaining conformance failures need entity replacement text parsed
as markup rather than substituted as text.

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

**No absolute figures are published here.** The same binary on the same
machine on the same day measured 14.7 MB/s and 123.1 MB/s, the
difference being a load average above 30 on six cores. An 8× spread
means any number in that range is truthful and none is informative, so
[`doc/BENCHMARKS.md`](https://github.com/sebastienrousseau/oxml/blob/main/doc/BENCHMARKS.md)
states the method and the conditions a figure must carry, and this
table says what each benchmark isolates.

| Benchmark | What it isolates |
|---|---|
| `parse/wide_1000` | 1,000 sibling elements — sibling handling |
| `parse/deep_max` | Nesting to just inside `MAX_DEPTH` — recursion |
| `parse/attributes_1000` | 4 attributes each, namespaced — resolution |
| `xpath/compile` | Compiling `//book[@lang='en']/title` |
| `xpath/eval_descendant` | `//title` over 2,000 books |
| `xpath/eval_predicate` | `//book[@lang='en']/title` over 2,000 books |
| `xpath/eval_numeric_predicate` | A positional predicate over the same |
| `encoding/decode_utf8_borrowed` | The zero-copy path — nothing is transcoded |
| `encoding/decode_utf16_be` | UTF-16 big-endian, which must allocate |
| `encoding/decode_utf16_le` | UTF-16 little-endian |
| `encoding/decode_latin1` | ISO-8859-1 |
| `encoding/parse_bytes_utf16_be` | Decode and parse together, as a caller pays it |
| `tree/descendants_2000` | A full arena walk |
| `tree/text_of_root` | Gathering a subtree's character data |
| `tree/children_of_each` | Child lists through the shared arena |
| `tree/attribute_by_name` | Lookup by expanded name |
| `entities/expand_1000_references` | 1,000 references to one declared entity |
| `entities/literal_equivalent` | The same output written out — the control |
| `entities/nested_four_levels` | Nested expansion, inside the budget |

Compilation and evaluation are timed separately on purpose: a caller
that compiles once and evaluates many times pays only the second, and
reporting a combined figure would hide that. `entities` carries its own
control for the same reason — expansion cost is only meaningful against
the cost of the characters it produces.

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

## XPath name tests resolve namespaces

A prefix in an expression is resolved against **the expression's own
bindings**, not the document's declarations. The same prefix can mean
different things in the two, and resolving against the document would
make an expression's meaning depend on which document it ran against.

```rust
use oxml::{XPath, parse};

let doc = parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a><a>no</a></r>"#).unwrap();

let q = XPath::compile_with_namespaces("//m:a", &[("m", "urn:u")]).unwrap();
assert_eq!(q.evaluate(&doc).to_str(&doc), "yes");

// Only the URI matters. The expression may use a different prefix from
// the document, and a document that renames its prefixes still matches.
let q = XPath::compile_with_namespaces("//q:a", &[("q", "urn:u")]).unwrap();
assert_eq!(q.evaluate(&doc).to_str(&doc), "yes");
```

**An unbound prefix is a compile error**, not a silent match:

```rust
use oxml::XPath;

let err = XPath::compile("//m:a").unwrap_err();
assert!(err.message.contains("unbound namespace prefix"));
```

That matters more than it looks. Until 0.0.4 a prefixed name test
matched on its local part alone, so `//m:a` selected every `a`
whatever its namespace — a wrong answer with no error attached, which
is the worst way for a query engine to fail.

### Unprefixed names match no namespace

```rust
use oxml::{XPath, parse};

let doc = parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a><a>no</a></r>"#).unwrap();
let q = XPath::compile("//a").unwrap();
assert_eq!(q.evaluate(&doc).to_str(&doc), "no");
```

This is XPath 1.0's classic surprise and every conforming engine
behaves this way: a default namespace in the expression context does
**not** apply to node tests. If your document declares
`xmlns="urn:u"` on its root, `//item` matches nothing — bind a prefix
to that URI and write `//x:item`.

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
    // Names are interned; resolve the handle through the document.
    assert_eq!(doc.name(attr.name).unwrap().local, "id");
}

// Node kinds
assert!(matches!(doc.kind(first), Some(NodeKind::Element { .. })));

// Text, with XPath string-value semantics: comments contribute nothing
assert_eq!(doc.text(root), "x");

// Queries, compiled once
let q = XPath::compile("count(//a)").unwrap();
assert_eq!(q.evaluate(&doc).to_str(&doc), "1");
```

## Reading without a tree

`parse` builds a tree. When you only need to walk a document once —
counting, extracting, converting — `stream::Reader` hands you one
event at a time and builds nothing.

```rust
use oxml::stream::{Event, Reader};

let mut reader = Reader::new("<r><a id='1'>x</a></r>").unwrap();
let mut names = Vec::new();
while let Some(event) = reader.next_event().unwrap() {
    if let Event::StartElement { name, .. } = event {
        names.push(name.local);
    }
}
assert_eq!(names, ["r", "a"]);
```

It is the same scanner the tree parser runs, not a second
implementation, so the two accept exactly the same documents and
report the same error at the same byte offset when they don't. A test
holds them to that.

**What it saves.** Reading the 16,004-node document from the
allocation tests holds 191,967 bytes at peak against the tree's
1,809,822 — 89% less. It was 92% before the document began owning its
input: the gap narrowed because the *tree* got cheaper, not because
reading got dearer.

**What it costs.** Time. Reading a document as events takes about 1.9
times as long as parsing it into a tree, because an event owns its
text: a name and an attribute value are copied out, where the tree
keeps a range into the input it already holds. Streaming trades time
for memory, and the trade is only worth making when the memory
matters — which is the opposite of what "streaming" usually
suggests.

**Reading from a byte source.** `Reader::new` takes a `&str`, so the
document is resident either way. `Reader::from_reader` takes anything
implementing `BufRead` and keeps only the construct it is reading,
dropping what it has passed — so a file larger than memory is
readable. Measured on a 185 KB document and a 1,929 KB one, ten times
the input, both held **34,722 bytes**: the same figure to the byte.

```rust
use oxml::stream::{Event, Reader};

let mut reader = Reader::from_reader(std::io::Cursor::new(
    b"<catalogue><item>Tea</item></catalogue>".to_vec(),
))?;
let mut items = 0;
while let Some(event) = reader.next_event()? {
    if let Event::StartElement { name, .. } = event {
        if name.local == "item" { items += 1; }
    }
}
assert_eq!(items, 1);
# Ok::<(), oxml::Error>(())
```

The events are the same events: a test holds `from_reader` and
`Reader::new` to an identical sequence at buffer sizes down to one
byte, because a bug here looks like a document that parses at one
buffer size and not another.

Events carry resolved names, so a prefix is looked up in the scopes
open at that point exactly as in the tree, and `Limits` apply
identically — including the depth limit, which here bounds a `Vec`
rather than the call stack.

## Examples

Every example compiles and runs in CI, and a CI job checks that
between them they call every public function in the crate — so an API
added without an example fails the build rather than going
undocumented.

```bash
cargo run --example parse_and_query
```

| Example | What it shows |
|---|---|
| [`parse_and_query`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/parse_and_query.rs) | XPath queries and direct tree traversal side by side |
| [`walk_the_tree`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/walk_the_tree.rs) | Node kinds, parents, children, descendants, and comparing expanded names |
| [`read_attributes`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/read_attributes.rs) | Attributes by local name and by namespace, and attributes as nodes |
| [`xpath_values`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/xpath_values.rs) | The four XPath value types, conversions between them, and evaluating from a context node |
| [`handle_errors`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/handle_errors.rs) | Matching on `ErrorKind`, and drawing a caret under the offending line |
| [`apply_limits`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/apply_limits.rs) | Billion laughs, XXE, depth bounds, and the cost of each profile |
| [`external_entities`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/external_entities.rs) | Supplying external entities and subsets, an allow-list source, and why XXE cannot happen |
| [`names_and_ids`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/names_and_ids.rs) | Qualified names, DTD-declared IDs, inherited `xml:lang`, and the functions that read them |
| [`decode_bytes`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/decode_bytes.rs) | The same document in five encodings, and the two kinds of encoding failure |
| [`stream_events`](https://github.com/sebastienrousseau/oxml/blob/main/crates/oxml/examples/stream_events.rs) | Reading a document as events with no tree built, and the identical errors both entry points give |

## Configuration

Everything configurable lives in [`Limits`]. There is no global state,
no builder, and no environment variable: a `Limits` value is passed to
`parse_with` or `parse_bytes_with`, and two documents parsed on two
threads can use different ones.

```rust
use oxml::{Limits, parse_with};

// Start from a profile and adjust. `Limits` is `#[non_exhaustive]`,
// so it cannot be written out as a struct literal -- a bound added in
// a later version would otherwise be a breaking change for everyone
// who had.
let mut limits = Limits::default();
limits.max_depth = 32;
limits.max_attributes_per_element = 16;

assert!(parse_with("<a><b/></a>", limits).is_ok());
```

### The three profiles

| Field | `strict()` | `default()` | `permissive()` |
|---|---|---|---|
| `max_depth` | 64 | 256 | 256 |
| `max_attributes_per_element` | 64 | 1000 | 100000 |
| `max_attribute_size` | 8192 | 524288 | 67108864 |
| `max_name_length` | 256 | 1000 | 1048576 |
| `max_nodes` | 100000 | unbounded | unbounded |
| `max_text_length` | 1048576 | unbounded | unbounded |
| `max_entity_depth` | 4 | 10 | 40 |
| `max_entity_expansion` | 100000 | 10000000 | 1000000000 |
| `max_xpath_depth` | 32 | 256 | 1000 |
| `max_xpath_operators` | 1000 | 10000 | 1000000 |

The numbers are given unformatted because a test parses this table and
compares every cell against the profile it names. A table of plausible
figures is worse than no table: it is believed.

`permissive()` deliberately leaves `max_depth` where the default has
it. Depth is the one bound whose cost is stack rather than heap, and
the stack is the one resource a caller cannot recover from: overflow
aborts the process. Measured on this crate's own test binaries, a
2 MiB thread stack overflows at around 280 levels in a debug build, at
roughly 7,489 bytes per frame. Raising `max_depth` to 10,000 does not
give you deeper documents; it gives you a crash.

### Which profile to choose

- **`default()`** — a build tool, a test fixture, a document you
  wrote. Generous enough that no real document is refused.
- **`strict()`** — anything taking XML from the network. It costs you
  documents nested past 64 levels and entity expansions past 100 KB,
  which in practice means it costs you nothing and rejects attacks
  2,600× faster.
- **`permissive()`** — a one-off conversion of a machine-generated
  document you already trust.

### Cargo features

| Feature | Default | What it does |
|---|---|---|
| `std` | yes | `std::error::Error` for `Error`, and `String`/`Vec` from `std` |
| `xpath` | yes | The XPath 1.0 engine. Separable because the tree alone is useful |
| `libm` | no | `floor`/`ceil`/`trunc` from `libm` rather than `std`. Required to build `xpath` without `std` |

Building `xpath` with neither `std` nor `libm` is a compile error with
an explanation, not a link failure: XPath needs three floating-point
functions that `core` does not provide.

### `no_std` is checked, not claimed

`scripts/check-no-std.sh` runs in CI and builds **every** feature
combination with `std` off — none, `libm`, and `xpath,libm` — against
**every** bare-metal target: `thumbv7em-none-eabihf`,
`riscv32imac-unknown-none-elf` and `aarch64-unknown-none`. Nine builds.

It also greps for `std::` that is not behind `#[cfg(feature = "std")]`,
which the builds cannot catch on their own: a std-only call in a
combination nothing builds sits there unnoticed until someone enables
that combination.

That exists because one configuration was not enough. A `Vec` that
resolved through the `std` prelude reached `main` and broke all three
bare-metal jobs at once — the build that would have caught it was not
being run locally.

## Error reporting

An `Error` carries a byte offset and a kind, not a formatted string.
That is deliberate: the caller knows how it wants to present a
failure, and a pre-formatted message is a lossy version of the
structure it was built from.

```rust
use oxml::parse;

let input = "<config>\n  <name>ok</name>\n  <port>8080</hostname>\n</config>";
let error = parse(input).expect_err("mismatched tags");

let (line, column) = error.line_column(input);
assert_eq!((line, column), (3, 13));

// `ErrorKind` renders without the offset, for a caret that already
// shows the position.
let source = input.lines().nth(line - 1).unwrap();
println!("{line:>3} | {source}");
println!("    | {}^ {}", " ".repeat(column - 1), error.kind);
```

```text
  3 |   <port>8080</hostname>
    |             ^ </hostname> closes <port>
```

`line_column` counts in characters rather than bytes, so the column it
reports is the one an editor shows. It never panics, including when
the offset lands inside a multi-byte character or when it is given a
different document from the one that produced the error — a wrong
column is a nuisance, but a panic in the error path takes down the
process.

## External entities and subsets

oxml performs no I/O. A document that references an external entity or
an external DTD subset names a *location*, and resolving it is the
caller's decision — they have the permission model, the user, and the
context to make it.

[`parse`](https://docs.rs/oxml/latest/oxml/fn.parse.html) supplies
nothing, so an external reference expands to nothing. That is the
default and it is why XXE is structurally impossible rather than
switched off.

When you *do* have the content, hand it over:

```rust
use oxml::{Limits, parse_with_external};

let parts: &[(&str, &str)] = &[("greeting.ent", "hello")];
let doc = parse_with_external(
    r#"<!DOCTYPE d [<!ENTITY g SYSTEM "greeting.ent">]><d>&g;</d>"#,
    Limits::default(),
    &parts,
)?;
assert_eq!(doc.text(doc.root()), "hello");
# Ok::<(), oxml::Error>(())
```

Anything implementing `ExternalSource` works; a slice of
`(system identifier, content)` pairs is the simplest one. **The parser
still never opens a file** — the lookup happens in your code, where you
can see it and decide.

With content available, the rules only that content can settle are
checked: a text declaration must be well formed, must name an encoding,
must not claim `standalone`, and must not declare a version later than
the document's. Each external entity is line-ending–normalised
independently, by its own declared version.

The **external subset** works the same way. Given its content, its
declarations are parsed and merged behind the internal subset's — "the
first declaration binds", so anything declared in both takes the
internal one.

A declaration whose text contains a parameter entity reference is
**skipped as unknown** rather than reported as malformed.
`<!ELEMENT x (a,%choice;,c)>` is legal in an external subset and oxml
does not expand parameter entities, so treating it as an error would
reject valid documents. The constraints such a declaration states go
unchecked.

An entity that is **declared and never referenced is never read**. A
processor need not fetch what a document does not use, and validating
eagerly rejected a valid document whose unused entity happened to omit
its encoding.

## Encodings

`parse` takes a `&str`, which means the caller has already decided the
encoding. `parse_bytes` reads the document's own declaration and
byte-order mark instead, which is what you want for a file or an HTTP
body.

```rust
use oxml::parse_bytes;

let utf8 = b"<?xml version='1.0'?><a>hi</a>";
let doc = parse_bytes(utf8).expect("valid");
assert_eq!(doc.text(doc.root()), "hi");
```

UTF-8, UTF-16 (both byte orders, with or without a BOM) and
ISO-8859-1 are decoded. A BOM overrides a declaration that disagrees
with it. UTF-8 input is borrowed rather than copied, so the common
case costs nothing.

Two failures that look alike are kept apart, because they are not the
same problem:

- **`MalformedEncoding`** — the name breaks XML production 81
  (`encoding="UTF~8"`), or the bytes are not valid in the encoding
  they claim. The document is malformed and every conforming parser
  must reject it.
- **`UnsupportedEncoding`** — the name is legal and names an encoding
  this crate does not implement (`encoding="Shift_JIS"`). The document
  may be perfectly well-formed. Decode it with a crate that knows the
  encoding and call `parse`.

## When not to use oxml

Honestly:

- **You need XSLT.** oxml does not have it. Use `libxml`.
- **You need complete XSD validation today.** `xmlschema` is
  published, but it is early: check its own README for what is
  implemented before depending on it.
- **Raw streaming speed is what you are buying.**
  `stream::Reader::from_reader` reads from any `BufRead` and holds a
  bounded amount, so size is no longer the obstacle — but reading a
  document as events is about 1.9 times *slower* than parsing it into
  a tree here, because an event owns its text. `quick-xml` is faster
  at this and stays the right tool when throughput is the point.
- **You need XPath 2.0 or 3.1.** oxml implements 1.0.
- **Raw parse throughput is your only metric.** `quick-xml` is
  extremely well optimised and years ahead on that axis. oxml's
  argument is what you can do *after* parsing.

## FAQ

### Is it faster than quick-xml or roxmltree?

No, and the README will say so until it is. Measured against each on
the job it actually does, on the same 855 KB document:

| | oxml | reference | ratio |
|---|---|---|---|
| Events, no tree | `oxml::stream` | `quick-xml` | **0.09×** |
| Tree | `oxml::parse` | `roxmltree` | **0.32×** |

So `quick-xml` reads events about eleven times faster, and
`roxmltree` builds a tree about three times faster. The gap is
allocation and copying: `oxml` performs about 0.50 allocations per
node where a borrowed design performs almost none, and it copies the
input once so the document can own it.

Reproduce it with `cargo bench --bench comparison`. Both crates are
dev-dependencies, so this is a measurement you can rerun rather than a
claim you have to take on trust — an earlier version of this section
quoted absolute MB/s from a comparison nothing in the repository could
regenerate.

**Why a ratio and not MB/s.** An absolute figure describes the machine
as much as the code: the same binary measured 14.7 and 123.1 MB/s on
one host on one day, and the difference was load. A ratio survives
that, because contention slows both arms. The benchmark times each
implementation against its reference back to back and takes the
*fastest* run of each — the sample contention perturbed least. With
ten CPU hogs competing on a six-core machine the ratios moved by 3%
and 5%, while a median-based estimate of the same data halved. See
[doc/BENCHMARKS.md](https://github.com/sebastienrousseau/oxml/blob/main/doc/BENCHMARKS.md).

### Can it stream?

Yes, both senses of the word.

`stream::Reader::new` reads a document as events without building a
tree — 89% less held at peak on a 16,000-node document — and
`stream::Reader::from_reader` reads from any `BufRead`, keeping only
the construct in hand. A 185 KB document and a 1,929 KB one both held
34,722 bytes, so size is bounded by the largest single construct
rather than by the file.

What it is not is *faster*. Reading as events costs about 1.9 times
what parsing into a tree costs, because an event owns its text where
the tree keeps ranges into input it already holds. Streaming here
trades time for memory. If throughput is what you mean by streaming,
`quick-xml` is the tool.

### How conformant is it?

100% of decided tests in the W3C XML Conformance Test Suite, release
`xmlts20130923`, with 98.9% coverage of the 2,585 tests and zero panics.
Both numbers are published because either alone misleads: a pass rate
can be raised by skipping the hard tests.

One failure remains, and it is not about external content at all:
it needs **type-aware attribute-value normalisation**, where a
tokenized `ATTLIST` type is stripped and collapsed and a `CDATA` one
is not. Since the parser performs no I/O by construction, closing
them means the caller supplying that content, not oxml fetching it.

Entity replacement text being parsed as markup rather than substituted
as characters was the previous leader and accounted for 13; that is
done. Every remaining failure is the parser being too *permissive* —
there is no document in the suite it wrongly rejects.

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

### Why is there a `Limits` argument instead of a builder?

Because a builder implies a default that someone chose for you and a
sequence of calls you have to remember. `Limits` is a plain value: you
can store one in a config struct, send it between threads, compare two,
and see the whole policy in one place. `parse` uses `Limits::default()`
so the common case stays a single call.

### Why does `parse` take `&str` when XML is bytes?

Because deciding the encoding is a separate job from parsing, and
conflating them is how parsers end up with an encoding layer that
cannot be tested or replaced. `parse_bytes` does both when you want
that; `parse` is for when you already know.

It also keeps the fast path honest. A `&str` is already valid UTF-8,
so `parse` does no transcoding and no validation pass over the bytes.

### Is the tree mutable?

No. `Document` is built by the parser and read afterwards. Mutation and
serialisation are the same feature -- there is little point in one
without the other -- and neither is implemented. If you need to produce
XML, build the string, or use `quick-xml`'s writer.

### How much memory does a document take?

Measured at **0.50 allocations per node** — 8,076 allocations for a
16,004-node document, counted with a wrapping global allocator over the
whole of `parse` and divided by `Document::len()`. A test holds that
figure to a ceiling, so it cannot drift upward unnoticed.

The structure is a flat arena rather than a graph of `Rc`s. Child and
attribute lists are `(start, len)` ranges into shared vectors and
names are interned, so traversal is index arithmetic rather than
pointer chasing and the document is `Send + Sync` for free. That work
took the figure from 4.13 to 1.13.

The step from 1.13 to 0.50 is the document **owning its input**. Text
nodes, comments and attribute values are ranges into that string
rather than strings of their own, so a document that expands no
entities allocates nothing at all for its character data. Names had
already stopped allocating — they are slices of the input until
interning, and a repeated name costs a map probe.

Fewer than one allocation per node means most nodes cost none: what
remains is the arenas themselves, which grow by doubling and so cost a
handful of allocations for the whole document.

The trade is one copy of the input, because the document owns the
decoded and line-ending-normalised text rather than borrowing yours.
That is what lets `Document` have no lifetime parameter and
`parse_bytes` decode UTF-16, where there is no caller string to borrow
from. See "When not to use oxml" — the whole document is in memory.

### Why `NodeId` rather than references?

Because a reference into the tree borrows the tree, and almost every
useful traversal wants to hold a position while looking at something
else. `NodeId` is `Copy` and pointer-sized, and outlives any particular
borrow, so you can collect ids into a `Vec`, store them in a struct, or
return them from a function without a lifetime parameter spreading
through your code.

The cost is that a `NodeId` from one document means something different
in another. They are indices, and nothing stops you mixing them.

### Does it allocate during XPath evaluation?

Yes. Node-sets are `Vec<NodeId>`, and intermediate values are built and
dropped as the expression evaluates. Compiling is where the one-time
cost sits: `XPath::compile` parses to a syntax tree once, and the
result is document-independent, so a server can compile every
expression it knows at startup and evaluate them per request.

### What happens on a document that is not well-formed?

It is rejected, with the byte offset of the problem. There is no
recovery mode and no "lenient" flag. A parser that guesses at what a
malformed document meant produces a tree that no two implementations
agree on, and the disagreement surfaces later as a bug somewhere else.

### Is it thread-safe?

`Document` is `Send` and `Sync`; it is immutable after parsing, so any
number of threads can query one. `XPath` is likewise `Send` and `Sync`,
which is what makes compile-once-evaluate-many work across a thread
pool.

### Why is `unsafe` forbidden rather than just avoided?

Because "we don't use unsafe" is a claim about the present and
`#![forbid(unsafe_code)]` is a property of the build. CI checks the
attribute is still in the source, so removing it is a visible change in
a diff rather than a quiet one.

It does cost something. Some of the tricks that make the fastest
parsers fast are unavailable, and this crate is slower than it could be
because of it. That trade is stated rather than hidden.

### How do I report a security issue?

See [SECURITY.md](https://github.com/sebastienrousseau/oxml/blob/main/SECURITY.md). Please do not open a public issue for a
vulnerability.

## Development

```bash
cargo test --workspace --all-features   # 466 tests, plus 27 doctests
cargo test --no-default-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo bench
cargo doc --no-deps --open
```

CI runs all of the above on Linux, macOS and Windows, plus an MSRV
build and a check that `#![forbid(unsafe_code)]` is still present.

## Security

See [SECURITY.md](https://github.com/sebastienrousseau/oxml/blob/main/SECURITY.md) for reporting and the full threat model.

In summary:

- **External entities are never dereferenced.** There is no code that
  opens a file or a socket on a document's behalf, so XXE is foreclosed
  by construction rather than by a default that can be changed.
- **Entity expansion is bounded per document**, not per reference. A
  per-reference budget still lets a quadratic blowup through: a
  thousand small expansions cost a thousand times the budget.
- **`#![forbid(unsafe_code)]`** rules out memory-corruption bugs, and
  CI checks the attribute is still there.
- **Recursion is bounded** by `Limits::max_depth`, because a stack
  overflow aborts the process rather than unwinding and no caller can
  catch it.
- **The defaults are generous.** They accept every real document we
  have found, which means they are not the tightest bound available.
  A service parsing untrusted XML under load should use
  `Limits::strict`: it rejects a nine-level billion-laughs document in
  25 µs where the defaults take 66 ms.

## Documentation

- [API documentation](https://docs.rs/oxml)
- [CHANGELOG.md](https://github.com/sebastienrousseau/oxml/blob/main/CHANGELOG.md)
- [CONTRIBUTING.md](https://github.com/sebastienrousseau/oxml/blob/main/CONTRIBUTING.md)
- [GOVERNANCE.md](https://github.com/sebastienrousseau/oxml/blob/main/GOVERNANCE.md)
- [SECURITY.md](https://github.com/sebastienrousseau/oxml/blob/main/SECURITY.md)

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
