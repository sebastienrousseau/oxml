// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Read attributes, including namespaced ones.
//!
//! Run with:
//!
//! ```text
//! cargo run --example read_attributes
//! ```

use oxml::{Attribute, ExpandedName, NodeKind, parse};

const DOC: &str = r#"<?xml version="1.0"?>
<order xmlns:x="urn:example:x" id="A-1" x:ref="R-9" note="two &amp; a half">
  <line sku="ABC" qty="2"/>
  <line sku="DEF" qty="1"/>
</order>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = parse(DOC)?;
    let order = doc.root_element().expect("a root element");

    // The quick way: look one up by local name.
    println!("id   = {:?}", doc.attribute(order, "id"));
    println!("ref  = {:?}", doc.attribute(order, "ref"));
    println!("gone = {:?}", doc.attribute(order, "nonexistent"));

    // Entities are already resolved, so `&amp;` is an ampersand here
    // and not something the caller has to unescape.
    println!("note = {:?}", doc.attribute(order, "note"));

    println!("\n== every attribute ==");
    for attr in doc.attributes(order) {
        let Attribute { name, value } = attr;
        // Names are interned: an `Attribute` carries a `NameId`, and a
        // document with 2,000 items and three attributes each holds
        // three names rather than six thousand. Resolve the handle
        // through the document.
        let name = doc.name(*name).expect("interned");
        match &name.namespace {
            Some(uri) => println!("  {{{uri}}}{} = {value:?}", name.local),
            None => println!("  {} = {value:?}", name.local),
        }
    }

    // `attribute` matches on the local part alone, which is ambiguous
    // when two namespaces use the same one. Compare the expanded name
    // when the distinction matters.
    println!("\n== by expanded name ==");
    let wanted = ExpandedName::qualified("urn:example:x", "ref");
    let found = doc
        .attributes(order)
        .into_iter()
        .find(|a| doc.name(a.name) == Some(&wanted))
        .map(|a| a.value.as_str());
    println!("  {{urn:example:x}}ref = {found:?}");

    // Attributes are also nodes, which is what the XPath attribute
    // axis returns. `attribute_nodes` gives their ids.
    println!("\n== as nodes ==");
    for &id in doc.attribute_nodes(order) {
        if let Some(NodeKind::Attr(a)) = doc.kind(id) {
            let name = doc.name(a.name).expect("interned");
            println!("  node {} is {}={:?}", id.index(), name.local, a.value);
        }
        // The string-value of an attribute node is its value, not the
        // text of the element carrying it.
        println!("    text(): {:?}", doc.text(id));
    }

    println!("\n== a table of lines ==");
    for &child in doc.children(order) {
        if !doc.is_element(child) {
            continue;
        }
        let sku = doc.attribute(child, "sku").unwrap_or("?");
        let qty = doc.attribute(child, "qty").unwrap_or("0");
        println!("  {sku} x{qty}");
    }
    Ok(())
}
