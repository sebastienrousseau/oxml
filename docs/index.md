---
title: oxml — Pure Rust XML Toolkit
description: A pure Rust XML toolkit with zero unsafe code — parsing, an ergonomic tree, and XPath 1.0.
hide:
  - navigation
  - toc
---

<section class="dot-hero" markdown>

# oxml

<p class="tagline">A pure Rust XML toolkit with zero unsafe code — fast streaming and DOM parsing, an ergonomic typed arena tree, and XPath 1.0 evaluation.</p>

<div class="buttons">
  <a class="primary" href="USER-GUIDE/">User Guide →</a>
  <a href="https://github.com/sebastienrousseau/oxml">GitHub</a>
  <a href="https://docs.rs/oxml">docs.rs</a>
  <a href="ARCHITECTURE/">Architecture</a>
</div>

</section>

## What's inside

<div class="grid cards" markdown>

- :material-shield-check:{ .lg .middle } **Zero `unsafe` code**

    ---

    `#![forbid(unsafe_code)]` at crate root and across all crates. No raw pointers, no unverified memory transmutes, zero memory-safety compromises.

    [→ Security Model](SECURITY-MODEL.md)

- :material-lightning-bolt:{ .lg .middle } **Arena-backed tree**

    ---

    Compact cache-friendly node arena with copyable `Node<'a>` handles. Element and attribute names are interned into atomic integers for instant comparisons.

    [→ Architecture](ARCHITECTURE.md)

- :material-code-brackets:{ .lg .middle } **XPath 1.0 engine**

    ---

    First-class XPath 1.0 evaluator supporting predicates, axis steps, functions (`count`, `contains`, `not`), and dynamic variable resolution.

    [→ User Guide](USER-GUIDE.md#xpath)

- :material-check-decagram:{ .lg .middle } **W3C conformance tested**

    ---

    Extensive validation against the official W3C XML Conformance Test Suite with tracked pass rates and documented failure analysis.

    [→ Conformance Report](CONFORMANCE.md)

- :material-swap-horizontal:{ .lg .middle } **Drop-in migration guides**

    ---

    Direct guides and migration snippets for moving from `quick-xml`, `roxmltree`, `sxd-xpath`, and `libxml2`.

    [→ Migration from roxmltree](MIGRATION-FROM-ROXMLTREE.md)

- :material-puzzle-outline:{ .lg .middle } **Modular ecosystem**

    ---

    Part of a synchronized suite: `oxml-cli`, `oxml-wasm`, `oxml-mcp`, `oxml-lsp`, `oxml-json`, and `xmlschema`.

    [→ Ecosystem Overview](ECOSYSTEM.md)

</div>

## Quick start

Add `oxml` to your Rust project:

```toml
[dependencies]
oxml = "0.0.10"
```

Parse and query an XML document:

```rust,ignore
use oxml::Document;

fn main() -> Result<(), Box<dyn std.error.Error>> {
    let xml = r#"
        <catalog>
            <book id="bk101" available="true">
                <title>XML Developer's Guide</title>
                <price currency="USD">44.95</price>
            </book>
        </catalog>
    "#;

    // Parse into an immutable, queryable Document arena
    let doc = Document::parse(xml)?;
    let root = doc.root_element();
    assert_eq!(root.tag_name(), "catalog");

    // Query nodes with XPath
    let titles = doc.xpath("//book[@available='true']/title")?;
    for node in titles {
        println!("Found book: {}", node.text_content());
    }

    Ok(())
}
```

## Where to next

- [**User Guide**](USER-GUIDE.md) — Comprehensive manual covering parsing, tree navigation, mutations, and XPath.
- [**Architecture**](ARCHITECTURE.md) — The arena model, string interning, and performance trade-offs.
- [**Comparison**](COMPARISON.md) — How oxml compares to `quick-xml`, `roxmltree`, `sxd`, and `libxml2`.
- [**Benchmarks**](BENCHMARKS.md) — Benchmarking methodology, throughput, and memory measurements.
- [**Testing & Assurance**](TESTING.md) — Fuzzing, Miri, property testing, and coverage analysis.
- [**API Reference**](https://docs.rs/oxml) — Complete Rustdoc API reference on docs.rs.

## Current release

- Release notes: [GitHub Releases](https://github.com/sebastienrousseau/oxml/releases)
- Crates.io: [crates.io/crates/oxml](https://crates.io/crates/oxml)
- Repository: [sebastienrousseau/oxml](https://github.com/sebastienrousseau/oxml)
