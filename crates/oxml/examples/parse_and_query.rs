// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Parse a document and query it, both with `XPath` and by walking the
//! tree directly.
//!
//! Run with:
//!
//! ```text
//! cargo run --example parse_and_query
//! ```

use oxml::{NodeKind, XPath, parse};

const CATALOGUE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<catalogue xmlns:m="urn:example:meta">
  <book lang="en" m:isbn="978-0441013593">
    <title>Dune</title>
    <author>Frank Herbert</author>
    <price currency="GBP">9.99</price>
  </book>
  <book lang="fr" m:isbn="978-2070413119">
    <title>Germinal</title>
    <author>Émile Zola</author>
    <price currency="EUR">7.50</price>
  </book>
  <!-- prices exclude shipping -->
</catalogue>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = parse(CATALOGUE)?;

    println!("== XPath ==");

    let english = XPath::compile("//book[@lang='en']/title")?;
    for node in english.evaluate(&doc).nodes().unwrap_or(&[]) {
        println!("  English title: {}", doc.text(*node));
    }

    // Compile once, evaluate many times — the compiled form is
    // document-independent.
    let total = XPath::compile("sum(//price)")?;
    println!("  Total price:   {}", total.evaluate(&doc).to_str(&doc));

    let count = XPath::compile("count(//book)")?;
    println!("  Books:         {}", count.evaluate(&doc).to_str(&doc));

    // The attribute axis yields attribute nodes, so string-value is
    // the attribute's value rather than its element's text.
    //
    // The prefix is bound here rather than read from the document: a
    // prefix in an expression resolves against the expression's own
    // bindings, so the same query works against a document that spells
    // the prefix differently. An unbound prefix is a compile error
    // rather than a match on the local part alone.
    let isbns = XPath::compile_with_namespaces(
        "//book/@m:isbn",
        &[("m", "urn:example:meta")],
    )?;
    for node in isbns.evaluate(&doc).nodes().unwrap_or(&[]) {
        println!("  ISBN:          {}", doc.text(*node));
    }

    println!("\n== Walking the tree ==");

    let root = doc.root_element().expect("a root element");
    for &book in doc.children(root) {
        let Some(NodeKind::Element { .. }) = doc.kind(book) else {
            continue;
        };
        let title = doc
            .children(book)
            .iter()
            .find(|&&c| doc.element_name(c).is_some_and(|n| n.local == "title"))
            .map(|&c| doc.text(c))
            .unwrap_or_default();
        let lang = doc.attribute(book, "lang").unwrap_or("?");
        println!("  [{lang}] {title}");
    }

    Ok(())
}
