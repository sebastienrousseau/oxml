<!-- markdownlint-disable MD033 MD041 -->
<div align="center">

# oxml

**A pure Rust XML toolkit. Zero unsafe code. Parsing, an ergonomic tree, and XPath 1.0.**

[![Crates.io](https://img.shields.io/crates/v/oxml.svg)](https://crates.io/crates/oxml)
[![Docs.rs](https://img.shields.io/docsrs/oxml)](https://docs.rs/oxml)
[![License](https://img.shields.io/crates/l/oxml.svg)](#license)

</div>

## Why oxml exists

Rust's XML ecosystem is strong at one end and empty at the other.
`quick-xml` and `roxmltree` parse quickly and are widely used. What
nothing maintained provides is the other half of what `lxml` gives
Python — querying.

| crate | downloads | last release | XPath | XSLT | XSD |
|---|---|---|---|---|---|
| `quick-xml` | 379M | active | ✗ | ✗ | ✗ |
| `xml-rs` | 137M | active | ✗ | ✗ | ✗ |
| `roxmltree` | 66M | active | ✗ | ✗ | ✗ |
| `sxd-xpath` | 2M | **2018** | ✓ | ✗ | ✗ |
| `libxml` | 2M | active | ✓ | ✓ | ✓ (C bindings) |

The only XPath implementation on crates.io has not shipped since 2018.
XSLT and XSD validation have no pure-Rust implementation at all.

oxml closes the query gap first, because that is the one people
actually hit.

## Install

```toml
[dependencies]
oxml = "0.0.1"
```

## Quick start

```rust
use oxml::{parse, XPath};

let doc = parse(r#"
    <library>
        <book lang="en"><title>Dune</title></book>
        <book lang="fr"><title>Germinal</title></book>
    </library>
"#)?;

let titles = XPath::compile("//book[@lang='en']/title")?;
assert_eq!(titles.evaluate(&doc).to_str(&doc), "Dune");
# Ok::<(), Box<dyn std::error::Error>>(())
```

XPath is optional — the tree stands on its own:

```rust
use oxml::parse;

let doc = parse("<a><b id='1'>text</b></a>")?;
let root = doc.root_element().unwrap();
let b = doc.children(root)[0];

assert_eq!(doc.attribute(b, "id"), Some("1"));
assert_eq!(doc.text(b), "text");
# Ok::<(), oxml::Error>(())
```

## Design

**Zero `unsafe`.** `#![forbid(unsafe_code)]`, enforced at compile time.
The tree is an arena of index-addressed nodes, so parent links cost no
`Rc`, no `RefCell`, and no raw pointers.

**Safe by construction, not by configuration.** Only the five
predefined entities and numeric character references are resolved.
External and custom entities are not — which forecloses XXE and
billion-laughs outright. A parser that cannot expand them cannot be
talked into reading `/etc/passwd`, and there is no flag to get that
wrong.

**Namespace-correct.** Names compare by URI and local part, never by
prefix. An unprefixed *element* takes the default namespace; an
unprefixed *attribute* is in no namespace. That asymmetry is the
classic source of namespace bugs, so the parser makes it explicit.

**Attributes are real nodes.** They live in the arena and are reachable
from `attribute::`, but are deliberately absent from `child::` —
exactly as XPath models them. `string(//book/@lang)` returns the
attribute's value, not its element's text.

## Feature flags

| flag | default | what it does |
|---|---|---|
| `std` | ✅ | Standard library integration, including `std::error::Error`. |
| `xpath` | ✅ | The XPath 1.0 engine. Turn it off if you only need to parse. |

## Status

Early. The parser and XPath engine are implemented and tested; the
suite around them is being built out. See [CHANGELOG.md](CHANGELOG.md).

Planned, in order: XSD validation (via the `xmlschema` crate), a
serialiser, then XSLT.

## The oxml suite

Every member releases in strict lockstep at the same `0.0.X` version.

| crate | what it is |
|---|---|
| `oxml` | The core library — parser, tree, XPath. |
| `oxml-cli` | Command-line querying and formatting. |
| `oxml-wasm` | WebAssembly bindings. |
| `oxml-mcp` | Model Context Protocol server. |
| `oxml-lsp` | Language server for XML documents. |
| `xmlschema` | XSD validation. |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). By participating you agree to
the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
